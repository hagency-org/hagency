//! Host-only bridge from retained process custody to one bounded Codex session.
//! No server route constructs this type; sandbox/dispatch qualification is open.
use hagency_platform::SupervisedReport;
use std::io;

mod session;
pub use session::OwnedSession;

/// Exact platform observations, deliberately distinct from upstream completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cleanup {
    Pending,
    Observed(SupervisedReport),
    Unknown { kind: io::ErrorKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StartError {
    #[error("invalid host runner settings")]
    Settings,
    #[error("native runner piped platform is unavailable")]
    Unsupported,
    /// The existing supervisor may have lost startup acknowledgement after spawn.
    /// Absence of a returned owner is never proof that no child briefly executed.
    #[error("native runner startup or IO handoff failed; cleanup must be inspected")]
    Uncertain {
        kind: io::ErrorKind,
        cleanup: Cleanup,
    },
}
