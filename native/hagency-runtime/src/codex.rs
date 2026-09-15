//! Codex 0.153.4 App Server JSONL. Protocol observations are untrusted data.
pub mod approval;
mod connection;
mod json;
pub mod session;
pub mod transport;
mod wire;

pub use connection::{Connection, Event, Phase, TurnScope};
pub use wire::{Decoder, Message, RequestId, RpcError, encode};

/// Includes every byte preceding LF (including CR for CRLF input).
pub const MAX_FRAME_BYTES: usize = 1_048_576;
pub const MAX_DEPTH: usize = 64;
pub const MAX_PENDING: usize = 32;
pub const MAX_SERVER_IDS: usize = 1024;
pub const PARTIAL_FRAME_MS: u64 = 10_000;
pub const MAX_REQUEST_MS: u64 = 1_200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("Codex protocol connection is closed")]
    Closed,
    #[error("invalid Codex protocol state")]
    State,
    #[error("invalid Codex protocol envelope")]
    Envelope,
    #[error("Codex protocol capacity exceeded")]
    Capacity,
    #[error("Codex protocol request identity mismatch")]
    Identity,
    #[error("Codex protocol deadline exceeded")]
    Timeout,
    #[error("Codex protocol clock moved backwards")]
    Clock,
    #[error("Codex protocol ended with incomplete data or pending work")]
    UnexpectedEof,
    #[error("Codex protocol transport failed")]
    Transport,
}

fn text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
