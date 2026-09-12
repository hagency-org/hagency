//! A single SQLite owner on a dedicated bounded worker; no IO in async handlers.
pub use domain::resource_configuration::{
    CeilingChange, ProfileChange, ResourceConfigurationAccess, ResourceConfigurationCommand,
    ResourceConfigurationResult,
};
mod database;
mod domain;
mod domain_worker;
pub mod private;
pub use domain::PrivateApprovalCard;
pub use domain::PublishedCatalog;
pub use domain::resource_publication::{
    ResourcePublicationAccess, ResourcePublicationCommand, ResourcePublicationResult,
    ResourcePublicationRetirement, resource_publication_revision,
};
pub use domain::uploads::UploadSettlement;
pub use domain::{
    AttachmentTicket, CeilingReport, DomainRepository, Effect, EffectOutcome, EffectState,
    KnownTokens, MAX_ENGAGEMENT_USAGE_PERIODS, MAX_ENGAGEMENT_USAGE_SOURCES,
    MAX_SOURCE_USAGE_RECEIPTS, MAX_USAGE_PERIODS, MAX_USAGE_RECEIPTS, MAX_USAGE_SOURCES,
    OwnedClaimProfile, OwnedClaimRoom, OwnedCompletion, OwnedDispatchScope, OwnedFailure,
    OwnedObservation, SourceUsage, UploadAdmission, UploadClaim, UploadIdentity, UploadPreparation,
    UploadSend, UsageCeiling, UsageEvidence, UsagePeriod, UsagePeriodKind, UsageReceipt,
    UsageReport, UsageSource, UsageSummary,
};
pub use domain_worker::DomainStore;
pub mod outbound;
mod repository;
mod shutdown;
mod worker;
pub use repository::Repository;
pub use shutdown::{
    NativeWriterObservation, NativeWriterUnavailable, ShutdownOutcome, ShutdownSnapshot,
};
pub use worker::Store;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("local management session is expired or retired")]
    LocalAuthority,
    #[error("runner capability is missing, stale or outside the task scope")]
    RunnerAuthority,
    #[error("session or resource requires inspected recovery")]
    Quarantined,
    #[error("invalid input: {0}")]
    Invalid(#[from] hagency_core::InvalidInput),
    #[error("request identifier was reused with different content")]
    Conflict,
    #[error("registration generation differs from its durable binding")]
    Generation,
    #[error("domain object does not exist")]
    NotFound,
    #[error("selected resource is withdrawn or does not qualify")]
    Unqualified,
    #[error("selected resource or declared shared seat has insufficient capacity")]
    InsufficientCapacity,
    #[error("cannot allocate against an agent with no declared ceiling")]
    NoCeiling,
    #[error("{message}")]
    OverCommit { message: String },
    #[error("domain operation is not valid in its current state")]
    State,
    #[error("state is owned by another process")]
    Locked,
    #[error("state format is corrupt or newer than this binary")]
    Schema,
    #[error("state must be an owner-private directory containing regular files")]
    Private,
    #[error("private state on this platform has not passed the native permission gate")]
    PlatformUnavailable,
    #[error("durable store capacity is exhausted; pending records were retained")]
    Capacity,
    #[error("worker queue is full; retry the same request identifier")]
    Busy,
    #[error("worker stopped; inspect or retry the same request identifier")]
    Unavailable,
    #[error("processing outcome is unknown; reconcile the original command before retrying")]
    OutcomeUnknown,
    #[error("storage error")]
    Sqlite(#[from] rusqlite::Error),
    #[error("filesystem error")]
    Io(#[from] std::io::Error),
    #[error("serialization error")]
    Json(#[from] serde_json::Error),
}

// Separate file-event custody; no Matrix acknowledgement proof constructor.
pub use domain::file_delivery::{
    FileDeliveryAdmission, FileDeliveryIdentity, FileDeliverySettlement, FilePublicationClaim,
    FilePublicationSend,
};

// Original local cache association; none of these values is SDK/file proof.
pub use domain::received_files::{
    ReceiveAdmission, ReceiveIdentity, ReceiveReservation, ReceiveWrite,
};

// Original one-shot router response authority; no native application proof.
pub use domain::{
    ApprovalResponseGrant, ApprovalResponseObservation, ApprovalResponseState,
    ApprovalResponseSummary,
};

pub use domain::{OwnedApprovalScope, OwnedApprovalStatus};

pub use domain::accounts::{
    ACCOUNT_PROFILE, AccountChoice, AccountEnrollmentAccess, AccountEnrollmentCommand,
    AccountState, ManagedAccount, ManagedLaunch,
};
