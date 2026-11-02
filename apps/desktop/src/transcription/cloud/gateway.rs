//! The WordInk gateway provider: an OpenAI-compatible `/v1/audio/transcriptions`
//! at a URL the person sets, with a bearer device token. The audio goes to
//! whatever host that is, so plain http is refused unless it stays on this
//! machine.

use super::CloudError;

pub const TRANSCRIPTIONS_PATH: &str = "/v1/audio/transcriptions";

/// The transcription endpoint for the gateway at `base` (its base URL, or the
/// full endpoint). Only https is allowed, except http to `localhost` or
/// `127.0.0.1`. A URL carrying a user name or password is refused: it would
/// put a credential in a setting, and `http://localhost@host` style URLs
/// are how a remote host passes for a local one.
pub fn endpoint(base: &str) -> Result<String, CloudError> {
    let base = base.trim();
    if base.is_empty() {
        return Err(CloudError::NoGateway);
    }
    let mut url = url::Url::parse(base).map_err(|_| CloudError::BadGatewayUrl)?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(CloudError::BadGatewayUrl);
    }
    match url.scheme() {
        "https" => {}
        "http" if matches!(url.host_str(), Some("localhost" | "127.0.0.1")) => {}
        "http" => return Err(CloudError::GatewayNotSecure),
        _ => return Err(CloudError::BadGatewayUrl),
    }
    if url.host_str().is_none() {
        return Err(CloudError::BadGatewayUrl);
    }
    url.set_query(None);
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/').to_string();
    let path = if path.ends_with("/audio/transcriptions") {
        path
    } else if path.ends_with("/v1") {
        format!("{path}/audio/transcriptions")
    } else {
        format!("{path}{TRANSCRIPTIONS_PATH}")
    };
    url.set_path(&path);
    Ok(url.to_string())
}
