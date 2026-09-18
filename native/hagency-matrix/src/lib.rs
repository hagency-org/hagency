//! Bounded authenticated account/room observations and scoped SDK event intake.
//! Host-owned frozen sends require verified crypto. Explicit fresh-account
//! enrollment establishes anchored identities and original recipient sessions;
//! general account recovery and production service cutover remain separate.
mod attachments;
pub use attachments::AttachmentHandle;
mod collector;
mod config;
mod enrollment;
mod event_batch;
mod http;
pub use http::UploadResponse;
mod intake;
mod media_download;
mod media_upload;
mod outgoing;
mod receive;
mod upload;
pub use upload::{FilePublicationAdmissionFailure, FilePublicationOperation};
pub use upload::{StagedUpload, UploadAdmissionFailure, UploadOperation};
mod provisioning;
mod sdk;
mod token_provision;
mod wire;
pub use collector::{Collector, ObservationSummary};
pub use config::{HostConfig, HostIdentity, HostRoom, Limits};
pub use http::RequestPacing;
pub use intake::{HostIntakePlan, IntakeStatus, IntakeSummary};
pub use media_download::{MediaDownloadError, MediaDownloadLimits, MediaDownloader, MediaId};
pub use media_upload::{
    MediaUploadError, MediaUploadLimits, MediaUploader, UploadAttempt, UploadState,
};
pub use outgoing::{OutgoingState, OutgoingSummary};
pub use provisioning::{ProvisionedAgent, TokenProvisioningHost};
pub use receive::{ReceiveError, ReceivedAttachment, ReceivedScope};
pub use token_provision::{
    ApplicationServiceCredential, ProvisionedTokenAccount, TokenAccountProvision,
};
pub use tokio_util::sync::CancellationToken;
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("invalid host Matrix configuration")]
    Config,
    #[error("Matrix collector or SDK owner is busy")]
    Busy,
    #[error("Matrix operation cancelled")]
    Cancelled,
    #[error("Matrix operation timed out")]
    Timeout,
    #[error("Matrix transport request failed")]
    Transport,
    #[error("Matrix redirect refused")]
    Redirect,
    #[error("Matrix headers exceed bounds or use unsupported framing")]
    Headers,
    #[error("Matrix body exceeds its byte limit")]
    BodyTooLarge,
    #[error("Matrix response is not one bounded unambiguous JSON document")]
    InvalidJson,
    #[error("Matrix response does not satisfy the observation contract")]
    Wire,
    #[error("Matrix authenticated account or device differs from host binding")]
    Identity,
    #[error("Matrix recipient verification changed or is unavailable")]
    Recipients,
    #[error("Matrix generation is stale or unavailable")]
    Generation,
    #[error("Matrix authentication refused")]
    Unauthorized,
    #[error("Matrix service returned HTTP {0}")]
    Remote(u16),
    #[error("protected SDK state is unavailable; no keys were reset")]
    Storage,
    #[error("SDK outcome is unknown; retained state requires host inspection")]
    OutcomeUnknown,
    #[error("Matrix durable or observation capacity is exhausted")]
    Capacity,
    #[error("Matrix receipt identity was replayed with different content")]
    Conflict,
    #[error("Matrix event intake retains unsupported or quarantined custody")]
    Unsupported,
    #[error("room snapshot is unsafe and was refused: {0}")]
    UnsafeSnapshot(String),
    #[error("domain authority rejected the observation")]
    Domain,
}
impl From<hagency_store::Error> for Error {
    fn from(e: hagency_store::Error) -> Self {
        match e {
            hagency_store::Error::Generation => Self::Generation,
            hagency_store::Error::OutcomeUnknown => Self::OutcomeUnknown,
            hagency_store::Error::Capacity => Self::Capacity,
            hagency_store::Error::Busy => Self::Busy,
            hagency_store::Error::Conflict => Self::Conflict,
            // A safety refusal is neither domain authority nor a wire-format
            // failure; it is its own word, carrying the digest-bound reason.
            hagency_store::Error::UnsafeSnapshot(reason) => Self::UnsafeSnapshot(reason),
            _ => Self::Domain,
        }
    }
}

#[cfg(test)]
extern crate self as hagency_matrix;

mod approval_batch;

mod approval_intake;
pub use approval_intake::{
    ApprovalCollector, ApprovalCustodyStage, ApprovalCustodyStatus, ApprovalIntakeSummary,
    ApprovalServiceTurn, HostApprovalConfig, HostApprovalPlan,
};

mod approval_delivery;
pub use approval_delivery::{
    PrivateApprovalDeliveryStage, PrivateApprovalDeliveryState, PrivateApprovalDeliveryStatus,
    PrivateApprovalDeliverySummary,
};
