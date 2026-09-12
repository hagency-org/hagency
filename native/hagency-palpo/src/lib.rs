//! Bounded outbound Palpo transport. Host-only configuration and custody do not
//! grant domain approval or prove authenticated Matrix event provenance.
//! `Adapter::run_with_resources` also publishes current canonical resource data;
//! the executable must explicitly own and configure that loop before activation.
mod adapter;
mod config;
mod http;
mod wire;

pub use adapter::{Adapter, Step};
pub use config::{HostConfig, Limits};
pub use tokio_util::sync::CancellationToken;

/// Deliberately excludes dependency errors, endpoint URLs and remote body text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("invalid host transport configuration")]
    Config,
    #[error("transport lane is already active")]
    Busy,
    #[error("transport operation was cancelled; durable custody was retained")]
    Cancelled,
    #[error("transport deadline elapsed; external outcome may be unknown")]
    Timeout,
    #[error("transport request failed; external outcome may be unknown")]
    Transport,
    #[error("redirect response was refused")]
    Redirect,
    #[error("response headers exceed bounds or use unsupported framing")]
    Headers,
    #[error("response body exceeds its byte limit")]
    BodyTooLarge,
    #[error("response is not one bounded unambiguous JSON document")]
    InvalidJson,
    #[error("response does not match the configured v2 delivery scope")]
    Wire,
    #[error("transport generation is stale")]
    Generation,
    #[error("transport authentication was refused")]
    Unauthorized,
    #[error("remote service returned HTTP {0}")]
    Remote(u16),
    #[error("durable custody rejected the operation; inspect original work")]
    Custody,
    #[error("durable custody writer is unavailable; inspect original work")]
    Unavailable,
    #[error("durable custody outcome is unknown; inspect original work")]
    OutcomeUnknown,
    #[error("durable custody capacity is exhausted; existing work was retained")]
    Capacity,
    #[error("durable identity was replayed with different content")]
    Conflict,
}
impl Error {
    fn retryable(self) -> bool {
        matches!(
            self,
            Self::Busy | Self::Timeout | Self::Transport | Self::Remote(429 | 500..=599)
        )
    }
}
impl From<hagency_store::Error> for Error {
    fn from(value: hagency_store::Error) -> Self {
        use hagency_store::Error as E;
        match value {
            E::Busy => Self::Busy,
            E::Generation => Self::Generation,
            E::Capacity => Self::Capacity,
            E::Conflict => Self::Conflict,
            E::Unavailable => Self::Unavailable,
            E::OutcomeUnknown => Self::OutcomeUnknown,
            _ => Self::Custody,
        }
    }
}
