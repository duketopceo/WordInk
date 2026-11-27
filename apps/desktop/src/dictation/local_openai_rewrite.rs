//! Local OpenAI-compatible chat completions for dictation post-processing
//! (`mlx_vlm.server`, `llama-server`, etc.). No OpenCode dependency.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};

use crate::net;

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 1],
    temperature: f32,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

#[derive(Deserialize)]
struct ApiError {
    error: Option<ApiErrorBody>,
}

#[derive(Deserialize)]
struct ApiErrorBody {
    message: Option<String>,
}

/// Calls `POST {base_url}/v1/chat/completions` with one user message.
pub fn complete_cancellable(
    prompt: &str,
    base_url: &str,
    model: &str,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<String> {
    let started = Instant::now();
    if cancelled.load(Ordering::Acquire) {
        return Err(eyre!("local post-processing was cancelled"));
    }
    let path = chat_completions_url(base_url)?;
    let body = serde_json::to_string(&ChatRequest {
        model,
        messages: [ChatMessage {
            role: "user",
            content: prompt,
        }],
        temperature: 0.0,
    })?;
    let remaining = remaining_deadline(started, deadline)?;
    if cancelled.load(Ordering::Acquire) {
        return Err(eyre!("local post-processing was cancelled"));
    }
    let agent = agent_for_url(&path, remaining);
    let response = agent
        .post(&path)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .send(body.as_bytes())
        .map_err(|error| map_transport_error(&path, error, started, deadline, cancelled))?;
    let status = response.status();
    let bytes = response
        .body_mut()
        .with_config()
        .limit(512 * 1024)
        .read_to_vec()
        .map_err(|error| eyre!("couldn't read the local model response: {error}"))?;
    if cancelled.load(Ordering::Acquire) {
        return Err(eyre!("local post-processing was cancelled"));
    }
    if !status.is_success() {
        let message = serde_json::from_slice::<ApiError>(&bytes)
            .ok()
            .and_then(|error| error.error)
            .and_then(|error| error.message)
            .unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned());
        return Err(eyre!(
            "local post-processing failed (HTTP {}): {message}",
            status.as_u16()
        ));
    }
    let parsed: ChatResponse = serde_json::from_slice(&bytes)
        .wrap_err("local model returned an invalid chat completion response")?;
    let Some(choice) = parsed.choices.first() else {
        return Err(eyre!("local model returned no completion choice"));
    };
    Ok(choice.message.content.clone())
}

fn chat_completions_url(base_url: &str) -> Result<String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err(eyre!("local post-processing base URL is empty"));
    }
    let base = trimmed.trim_end_matches('/');
    Ok(format!("{base}/v1/chat/completions"))
}

fn agent_for_url(url: &str, deadline: Duration) -> ureq::Agent {
    let builder = |config: net::Config| {
        config
            .timeout_global(Some(deadline.max(Duration::from_millis(200))))
            .http_status_as_error(false)
    };
    if url_starts_with_loopback(url) {
        net::loopback_agent(builder)
    } else {
        net::agent(url, builder)
    }
}

fn url_starts_with_loopback(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host())
        .is_some_and(|host| match host {
            url::Host::Domain(name) => {
                name == "localhost"
                    || name == "127.0.0.1"
                    || name == "::1"
                    || name == "[::1]"
            }
            url::Host::Ipv4(address) => address.is_loopback(),
            url::Host::Ipv6(address) => address.is_loopback(),
        })
}

fn remaining_deadline(started: Instant, deadline: Duration) -> Result<Duration> {
    let remaining = deadline.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        Err(eyre!(
            "local post-processing exceeded {} seconds",
            deadline.as_secs()
        ))
    } else {
        Ok(remaining)
    }
}

fn map_transport_error(
    url: &str,
    error: ureq::Error,
    started: Instant,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> color_eyre::Report {
    if cancelled.load(Ordering::Acquire) {
        return eyre!("local post-processing was cancelled");
    }
    if started.elapsed() >= deadline {
        return eyre!(
            "local post-processing exceeded {} seconds",
            deadline.as_secs()
        );
    }
    eyre!("couldn't reach {url}: {}", net::unreachable(url, &error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn serve_chat(response_body: &str) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        let body = response_body.to_owned();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            assert!(request_line.contains("POST"));
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
            }
            let mut posted = String::new();
            reader.read_to_string(&mut posted).unwrap();
            let posted: serde_json::Value = serde_json::from_str(&posted).unwrap();
            assert_eq!(posted["model"], "gemma4-cleanup");
            assert_eq!(posted["messages"][0]["role"], "user");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len(),
                body
            )
            .unwrap();
        });
        (base, server)
    }

    #[test]
    fn chat_completion_returns_assistant_text() {
        let (base, server) = serve_chat(
            r#"{"choices":[{"message":{"content":"Hello, Alex."}}]}"#,
        );
        let text = complete_cancellable(
            "Say hello.",
            &base,
            "gemma4-cleanup",
            Duration::from_secs(5),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(text, "Hello, Alex.");
        server.join().unwrap();
    }

    #[test]
    fn empty_base_url_is_rejected() {
        let error = complete_cancellable(
            "x",
            "   ",
            "m",
            Duration::from_secs(1),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(error.to_string().contains("base URL"));
    }

    #[test]
    fn chat_completions_path_is_appended() {
        assert_eq!(
            chat_completions_url("http://127.0.0.1:8080/").unwrap(),
            "http://127.0.0.1:8080/v1/chat/completions"
        );
    }
}