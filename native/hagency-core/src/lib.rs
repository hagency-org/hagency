//! Shared native domain contracts. No HTTP, database, process or Matrix SDK IO.
pub mod allocation;
pub mod attachments;
pub mod authority;
pub mod canonical;
pub mod ceiling;
pub mod conversations;
pub mod custody;
pub mod execution;
pub mod graphs;
pub mod messages;
pub mod peers;
pub mod project;
pub mod qualification;
pub mod replies;
pub mod task_intents;
pub mod tasks;
pub mod workflows;

pub const JSON_SAFE_MAX: u64 = 9_007_199_254_740_991;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct InvalidInput(pub &'static str);

pub mod ingress;

pub mod approvals;

pub mod completions;

pub mod uploads;

pub mod file_delivery;

pub mod received_files;
