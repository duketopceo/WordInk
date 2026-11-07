//! "Connect OpenRouter": OpenRouter's OAuth PKCE flow over plain HTTP (there
//! is no Rust SDK; the first-party ones are TypeScript and Python).
//!
//! 1. Make a code verifier and its S256 challenge.
//! 2. Listen briefly on `127.0.0.1:<random port>/callback`.
//! 3. Open `https://openrouter.ai/auth?callback_url=..&code_challenge=..&code_challenge_method=S256`.
//! 4. OpenRouter redirects the browser to the callback with `?code=`.
//! 5. POST `{code, code_verifier, code_challenge_method}` to
//!    `https://openrouter.ai/api/v1/auth/keys` for `{key}`.
//!
//! The key goes to the key store (`keys::store`) and nowhere else: it is
//! never logged, printed or returned for display.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};
use sha2::{Digest, Sha256};

pub const AUTH_URL: &str = "https://openrouter.ai/auth";
pub const KEYS_URL: &str = "https://openrouter.ai/api/v1/auth/keys";
/// The name OpenRouter's dashboard shows for the key's app.
pub const APP_NAME: &str = "WordInk";
const APP_URL: &str = "https://wordink.app";
/// How long the callback listener waits for the person to approve.
pub const WAIT: Duration = Duration::from_secs(300);

/// Where the flow talks, so a test can point it at a fake server.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub auth_url: String,
    pub keys_url: String,
    pub wait: Duration,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            auth_url: AUTH_URL.into(),
            keys_url: KEYS_URL.into(),
            wait: WAIT,
        }
    }
}

fn base64url(bytes: &[u8]) -> String {
    crate::transcription::cloud::openrouter::base64(bytes)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

/// A fresh `(verifier, challenge)`: 32 random bytes as the verifier (43
/// characters of base64url) and its SHA-256 as the challenge.
pub fn pkce_pair() -> Result<(String, String)> {
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|error| eyre!("no randomness: {error}"))?;
    let verifier = base64url(&random);
    let challenge = challenge_for(&verifier);
    Ok((verifier, challenge))
}

pub fn challenge_for(verifier: &str) -> String {
    base64url(&Sha256::digest(verifier.as_bytes()))
}

pub fn authorize_url(endpoints: &Endpoints, callback: &str, challenge: &str) -> String {
    let mut url = url::Url::parse(&endpoints.auth_url).expect("a valid auth URL");
    url.query_pairs_mut()
        .append_pair("callback_url", callback)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    url.into()
}

/// The `code` of a callback request line (`GET /callback?code=x HTTP/1.1`).
pub fn parse_callback(request_line: &str) -> Result<String> {
    let target = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| eyre!("an empty callback request"))?;
    let url = url::Url::parse(&format!("http://127.0.0.1{target}"))?;
    if url.path() != "/callback" {
        return Err(eyre!("not the callback path"));
    }
    if let Some((_, error)) = url.query_pairs().find(|(name, _)| name == "error") {
        return Err(eyre!("OpenRouter sign-in failed: {error}"));
    }
    url.query_pairs()
        .find(|(name, _)| name == "code")
        .map(|(_, code)| code.into_owned())
        .filter(|code| !code.is_empty())
        .ok_or_else(|| eyre!("the callback carried no code"))
}

fn reply(stream: &mut TcpStream, status: &str, body: &str) {
    let page = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(page.as_bytes());
}

/// Waits on `listener` for the callback and returns its code. Other requests
/// (a favicon, a probe) are answered 404 and ignored until `wait` runs out.
pub fn wait_for_code(listener: &TcpListener, wait: Duration) -> Result<String> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + wait;
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let mut buffer = [0u8; 4096];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let head = String::from_utf8_lossy(&buffer[..read]);
                let line = head.lines().next().unwrap_or_default();
                match parse_callback(line) {
                    Ok(code) => {
                        reply(
                            &mut stream,
                            "200 OK",
                            "<h3>WordInk is connected to OpenRouter.</h3><p>You can close this tab.</p>",
                        );
                        return Ok(code);
                    }
                    Err(error) if line.contains("/callback") => {
                        reply(
                            &mut stream,
                            "400 Bad Request",
                            "<h3>Sign-in did not complete.</h3>",
                        );
                        return Err(error);
                    }
                    Err(_) => reply(&mut stream, "404 Not Found", ""),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(eyre!("timed out waiting for OpenRouter sign-in"));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Exchanges the code for a key at `endpoints.keys_url`.
pub fn exchange(
    agent: &ureq::Agent,
    endpoints: &Endpoints,
    code: &str,
    verifier: &str,
) -> Result<String> {
    let body = serde_json::json!({
        "code": code,
        "code_verifier": verifier,
        "code_challenge_method": "S256",
    })
    .to_string();
    let mut response = agent
        .post(&endpoints.keys_url)
        .header("Content-Type", "application/json")
        .header("HTTP-Referer", APP_URL)
        .header("X-Title", APP_NAME)
        .send(body.as_bytes())
        .map_err(|error| eyre!("OpenRouter did not accept the sign-in: {error}"))?;
    let text = response.body_mut().read_to_string()?;
    let value: serde_json::Value = serde_json::from_str(&text)?;
    value["key"]
        .as_str()
        .filter(|key| !key.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| eyre!("OpenRouter's reply carried no key"))
}

/// The whole flow against `endpoints`; `open` shows the sign-in URL (the
/// browser) and `agent` makes the exchange. Returns the key.
pub fn run(
    endpoints: &Endpoints,
    agent: &ureq::Agent,
    open: impl FnOnce(&str) -> Result<()>,
) -> Result<String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let callback = format!(
        "http://127.0.0.1:{}/callback",
        listener.local_addr()?.port()
    );
    let (verifier, challenge) = pkce_pair()?;
    open(&authorize_url(endpoints, &callback, &challenge))?;
    let code = wait_for_code(&listener, endpoints.wait)?;
    exchange(agent, endpoints, &code, &verifier)
}

/// Signs in through the browser and saves the key with `save` (the key
/// store). Blocks up to [`WAIT`]: call it from a worker or the CLI.
pub fn connect(
    save: impl FnOnce(&str, &str) -> Result<crate::keys::KeySource>,
) -> Result<crate::keys::KeySource> {
    let endpoints = Endpoints::default();
    let agent = crate::net::agent(&endpoints.keys_url, |config| {
        config.timeout_global(Some(Duration::from_secs(30)))
    });
    let key = run(
        &endpoints,
        &agent,
        crate::desktop::opencode_catalog::open_url,
    )?;
    let source = save("openrouter", &key)?;
    crate::transcription::cloud::refresh_key_in_background();
    Ok(source)
}

/// Disconnect: forget the stored key.
pub fn disconnect() {
    crate::keys::remove("openrouter");
    crate::transcription::cloud::refresh_key_in_background();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn the_challenge_is_the_s256_of_the_verifier() {
        // RFC 7636 appendix B.
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let (verifier, challenge) = pkce_pair().unwrap();
        assert_eq!(verifier.len(), 43);
        assert_eq!(challenge_for(&verifier), challenge);
    }

    #[test]
    fn the_callback_is_parsed_and_errors_are_named() {
        assert_eq!(
            parse_callback("GET /callback?code=abc HTTP/1.1").unwrap(),
            "abc"
        );
        assert!(parse_callback("GET /callback HTTP/1.1").is_err());
        assert!(parse_callback("GET /callback?error=access_denied HTTP/1.1").is_err());
        assert!(parse_callback("GET /other?code=abc HTTP/1.1").is_err());
    }

    /// A fake OpenRouter key endpoint: records the request, answers `reply`.
    fn fake_keys_server(reply: &'static str) -> (String, Arc<Mutex<String>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!(
            "http://127.0.0.1:{}/api/v1/auth/keys",
            listener.local_addr().unwrap().port()
        );
        let seen = Arc::new(Mutex::new(String::new()));
        let record = seen.clone();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut data = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                data.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&data).to_string();
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let length = head
                        .to_ascii_lowercase()
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: ").map(str::to_owned))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if body.len() >= length {
                        break;
                    }
                }
            }
            *record.lock().unwrap() = String::from_utf8_lossy(&data).to_string();
            let page = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
            stream.write_all(page.as_bytes()).unwrap();
        });
        (url, seen)
    }

    fn endpoints(keys_url: String) -> Endpoints {
        Endpoints {
            auth_url: "https://openrouter.example/auth".into(),
            keys_url,
            wait: Duration::from_secs(10),
        }
    }

    #[test]
    fn the_full_flow_exchanges_the_code_with_the_verifier_for_a_key() {
        let (keys_url, seen) = fake_keys_server(r#"{"key":"sk-or-v1-fake","user_id":"u"}"#);
        let endpoints = endpoints(keys_url);
        let agent = crate::net::loopback_agent(|config| config);
        let authorize = Arc::new(Mutex::new(String::new()));
        let capture = authorize.clone();
        let key = run(&endpoints, &agent, move |url| {
            *capture.lock().unwrap() = url.to_owned();
            let parsed = url::Url::parse(url).unwrap();
            let callback = parsed
                .query_pairs()
                .find(|(name, _)| name == "callback_url")
                .unwrap()
                .1
                .into_owned();
            // The "browser" returns to the loopback callback with a code.
            std::thread::spawn(move || {
                let agent = crate::net::loopback_agent(|config| config);
                agent
                    .get(&format!("{callback}?code=the-code"))
                    .call()
                    .unwrap();
            });
            Ok(())
        })
        .unwrap();
        assert_eq!(key, "sk-or-v1-fake");

        let url = url::Url::parse(&authorize.lock().unwrap()).unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["code_challenge_method"], "S256");
        assert!(query["callback_url"].starts_with("http://127.0.0.1:"));
        assert!(query["callback_url"].ends_with("/callback"));

        let request = seen.lock().unwrap().clone();
        let body: serde_json::Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["code"], "the-code");
        assert_eq!(
            challenge_for(body["code_verifier"].as_str().unwrap()),
            query["code_challenge"],
            "the verifier matches the challenge sent to the browser"
        );
        assert!(request.contains("X-Title: WordInk") || request.contains("x-title: WordInk"));
    }

    #[test]
    fn a_reply_without_a_key_and_a_timeout_are_errors() {
        let (keys_url, _) = fake_keys_server(r#"{"error":"nope"}"#);
        let agent = crate::net::loopback_agent(|config| config);
        let error = exchange(&agent, &endpoints(keys_url), "c", "v").unwrap_err();
        assert!(error.to_string().contains("no key"));

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let error = wait_for_code(&listener, Duration::from_millis(150)).unwrap_err();
        assert!(error.to_string().contains("timed out"));
    }
}
