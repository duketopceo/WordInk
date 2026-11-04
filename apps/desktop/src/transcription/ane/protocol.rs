//! The JSON-lines protocol spoken with the resident worker shim
//! (`resources/ane/ane_worker.py`): request encoding and reply decoding, with
//! the `id` echo and size caps checked here so the supervisor can trust what
//! it gets.

use std::path::Path;

use serde::Deserialize;
use serde_json::json;

/// The shim, embedded; the supervisor writes it into its private directory.
pub(crate) const SHIM_SOURCE: &str = include_str!("../../../resources/ane/ane_worker.py");
/// Highest protocol the shim speaks.
pub(crate) const PROTOCOL: u32 = 1;
/// A reply line longer than this is a protocol violation.
pub(crate) const MAX_REPLY_LINE: usize = 1 << 20;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Request<'a> {
    Hello {
        id: u64,
    },
    Transcribe {
        id: u64,
        wav: &'a Path,
        deadline_ms: u64,
    },
    Shutdown {
        id: u64,
    },
}

impl Request<'_> {
    pub(crate) fn id(&self) -> u64 {
        match *self {
            Request::Hello { id } | Request::Transcribe { id, .. } | Request::Shutdown { id } => id,
        }
    }

    /// One line, newline included.
    pub(crate) fn encode(&self) -> String {
        let value = match *self {
            Request::Hello { id } => json!({"id": id, "op": "hello"}),
            Request::Transcribe {
                id,
                wav,
                deadline_ms,
            } => {
                json!({"id": id, "op": "transcribe", "wav": wav, "deadline_ms": deadline_ms})
            }
            Request::Shutdown { id } => json!({"id": id, "op": "shutdown"}),
        };
        format!("{value}\n")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Reply {
    Hello {
        pid: Option<u32>,
    },
    Transcript {
        text: String,
        pipeline_ms: Option<f64>,
    },
    Done,
    Failed {
        code: String,
        message: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ProtocolError {
    TooLong,
    NotJson,
    /// The reply answers a different request.
    WrongId {
        expected: u64,
        got: Option<u64>,
    },
    Unsupported(u32),
    Malformed(&'static str),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLong => write!(f, "reply line too long"),
            Self::NotJson => write!(f, "reply was not JSON"),
            Self::WrongId { expected, got } => write!(f, "reply id {got:?}, expected {expected}"),
            Self::Unsupported(version) => write!(f, "worker protocol {version} is not supported"),
            Self::Malformed(what) => write!(f, "malformed reply: {what}"),
        }
    }
}

#[derive(Deserialize)]
struct Wire {
    id: Option<u64>,
    ok: bool,
    #[serde(default)]
    transcript: Option<String>,
    #[serde(default)]
    pipeline_ms: Option<f64>,
    #[serde(default)]
    protocol: Option<u32>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// Decode the reply to `request`.
pub(crate) fn decode(line: &str, request: &Request<'_>) -> Result<Reply, ProtocolError> {
    if line.len() > MAX_REPLY_LINE {
        return Err(ProtocolError::TooLong);
    }
    let wire: Wire = serde_json::from_str(line).map_err(|_| ProtocolError::NotJson)?;
    let expected = request.id();
    // A failure the shim could not attribute (a malformed request) has no id.
    if wire.id != Some(expected) && !(wire.id.is_none() && !wire.ok) {
        return Err(ProtocolError::WrongId {
            expected,
            got: wire.id,
        });
    }
    if !wire.ok {
        return Ok(Reply::Failed {
            code: wire.code.unwrap_or_else(|| "error".to_string()),
            message: wire.message.unwrap_or_default(),
        });
    }
    match request {
        Request::Hello { .. } => match wire.protocol {
            Some(PROTOCOL) => Ok(Reply::Hello { pid: wire.pid }),
            Some(other) => Err(ProtocolError::Unsupported(other)),
            None => Err(ProtocolError::Malformed("hello without a protocol")),
        },
        Request::Transcribe { .. } => match wire.transcript {
            Some(text) => Ok(Reply::Transcript {
                text,
                pipeline_ms: wire.pipeline_ms,
            }),
            None => Err(ProtocolError::Malformed("transcribe without a transcript")),
        },
        Request::Shutdown { .. } => Ok(Reply::Done),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WAV: &str = "/run/clip.wav";

    fn transcribe(id: u64) -> Request<'static> {
        Request::Transcribe {
            id,
            wav: Path::new(WAV),
            deadline_ms: 8000,
        }
    }

    #[test]
    fn requests_encode_as_one_json_line() {
        let line = transcribe(7).encode();
        assert!(line.ends_with('\n') && line.matches('\n').count() == 1);
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["op"], "transcribe");
        assert_eq!(value["id"], 7);
        assert_eq!(value["wav"], WAV);
        assert_eq!(value["deadline_ms"], 8000);
        assert_eq!(
            Request::Hello { id: 1 }.encode(),
            "{\"id\":1,\"op\":\"hello\"}\n"
        );
    }

    #[test]
    fn replies_decode_for_each_request_kind() {
        let reply = decode(
            r#"{"id":7,"ok":true,"transcript":"hi there","pipeline_ms":576}"#,
            &transcribe(7),
        );
        assert_eq!(
            reply,
            Ok(Reply::Transcript {
                text: "hi there".into(),
                pipeline_ms: Some(576.0)
            })
        );
        let hello = decode(
            r#"{"id":1,"ok":true,"protocol":1,"pid":42}"#,
            &Request::Hello { id: 1 },
        );
        assert_eq!(hello, Ok(Reply::Hello { pid: Some(42) }));
        let failed = decode(
            r#"{"id":7,"ok":false,"code":"refused","message":"no"}"#,
            &transcribe(7),
        );
        assert_eq!(
            failed,
            Ok(Reply::Failed {
                code: "refused".into(),
                message: "no".into()
            })
        );
        assert_eq!(
            decode(r#"{"id":3,"ok":true}"#, &Request::Shutdown { id: 3 }),
            Ok(Reply::Done)
        );
    }

    #[test]
    fn a_reply_for_another_request_is_rejected() {
        let wrong = decode(r#"{"id":6,"ok":true,"transcript":"x"}"#, &transcribe(7));
        assert_eq!(
            wrong,
            Err(ProtocolError::WrongId {
                expected: 7,
                got: Some(6)
            })
        );
        let success_without_id = decode(r#"{"ok":true,"transcript":"x"}"#, &transcribe(7));
        assert_eq!(
            success_without_id,
            Err(ProtocolError::WrongId {
                expected: 7,
                got: None
            })
        );
    }

    #[test]
    fn an_unattributed_failure_still_decodes_as_a_failure() {
        let reply = decode(
            r#"{"id":null,"ok":false,"code":"too_large","message":"long"}"#,
            &transcribe(7),
        );
        assert!(matches!(reply, Ok(Reply::Failed { code, .. }) if code == "too_large"));
    }

    #[test]
    fn malformed_and_oversized_replies_are_rejected() {
        assert_eq!(
            decode("not json", &transcribe(1)),
            Err(ProtocolError::NotJson)
        );
        assert!(matches!(
            decode(r#"{"id":1,"ok":true}"#, &transcribe(1)),
            Err(ProtocolError::Malformed(_))
        ));
        let huge = format!(
            r#"{{"id":1,"ok":true,"transcript":"{}"}}"#,
            "x".repeat(MAX_REPLY_LINE)
        );
        assert_eq!(decode(&huge, &transcribe(1)), Err(ProtocolError::TooLong));
        assert_eq!(
            decode(
                r#"{"id":1,"ok":true,"protocol":9}"#,
                &Request::Hello { id: 1 }
            ),
            Err(ProtocolError::Unsupported(9))
        );
    }

    #[test]
    fn the_embedded_shim_is_the_one_on_disk() {
        assert!(SHIM_SOURCE.contains("PROTOCOL = 1"));
    }
}
