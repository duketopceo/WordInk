//! WordInk dictation core.
//!
//! A sans-I/O engine: it owns session state, audio processing and provider
//! protocol logic, and emits effects that a host (browser or desktop) performs.
//! It holds no clocks, sockets, threads or files. Time is measured by the
//! amount of audio the host pushes.
//!
//! ```
//! use wordink_core::providers::{Groq, Provider};
//! use wordink_core::{Effect, Event, Mode, Session, SessionConfig};
//!
//! let provider = Provider::Groq(Groq::relay("https://example.com/api/wordink"));
//! let mut s = Session::new(SessionConfig::new(Mode::PushToTalk, provider));
//! assert!(s.handle(Event::Press).contains(&Effect::RequestMic));
//! s.handle(Event::MicGranted { sample_rate: 48_000 });
//! let _levels = s.push_audio(&[0.0; 128]);
//! // Perform these (an HTTP request here), then report results as events.
//! let _effects = s.handle(Event::Release);
//! ```

pub mod audio;
pub mod effects;
pub mod error;
pub mod providers;
pub mod session;

pub use effects::{Effect, Event, HostResult, HttpBody, HttpRequest, Part, PartValue, WsData};
pub use error::ErrorCode;
pub use session::{Mode, Session, SessionConfig, State};
