//! Host-only, single-operation integration. Production availability is false:
//! directory provisioning, effective sandbox and owner approval IO remain gates.
mod approval;
mod host;
mod operation;
mod registration;
mod usage;
mod workspace;
#[cfg(any(test, feature = "test-diagnostics"))]
pub use approval::diagnostics;
pub use approval::{ApprovalHost, ApprovalNotice, ApprovalRequests};
pub use host::{Host, Limits};
pub use operation::{
    Failure, Operation, Protocol, Report, RuntimeObservation, RuntimeStage,
    RuntimeWriteObservation, Settlement, SettlementCause,
};
pub use registration::{LaunchAck, RegistrationError, WorkspaceRegistration};
pub use usage::{UsageFailure, UsageStatus};
pub use workspace::{
    StartedWorkspace, WorkspaceError, WorkspaceReceive, WorkspaceReceiveError, ordinary_launch_path,
};
#[cfg(test)]
#[path = "../tests/support/reply_loss.rs"]
mod reply_loss;
#[cfg(test)]
#[path = "../../hagency-store/tests/common/mod.rs"]
mod test_common;

#[cfg(test)]
#[path = "../tests/support/approval_loss.rs"]
mod approval_loss;
