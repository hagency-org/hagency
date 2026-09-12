//! Native maintenance of one already-assigned task through the scoped API.
use hagency_core::{
    JSON_SAFE_MAX,
    project::identifier,
    tasks::{MutationResult, RunnerCapability, Task, TaskMutation, TaskState, TextPatch, text},
};
use serde::Serialize;
use std::{net::SocketAddr, time::Duration};

pub(crate) mod completion;
pub(crate) mod coordination;
pub(crate) mod files;
pub(crate) mod received;
mod transport;
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("invalid native command or runner context")]
    Invalid,
    #[error("native runner service unavailable")]
    Unavailable,
    #[error("operation outcome unknown; inspect or retry identical call ID and content")]
    Unknown,
    #[error("native runner request was refused (HTTP {0})")]
    Refused(u16),
    #[error("native runner response is invalid or exceeds its limit")]
    Response,
}

/// Host-provisioned inherited context. No Debug/Serialize or on-disk format.
pub struct Context {
    address: SocketAddr,
    capability: RunnerCapability,
    task_id: String,
    file_tools: bool,
    receive_tools: bool,
}
impl Context {
    pub fn new(
        address: SocketAddr,
        capability: RunnerCapability,
        task_id: String,
    ) -> Result<Self, Error> {
        if !address.ip().is_loopback()
            || address.port() == 0
            || matches!(address,SocketAddr::V6(v) if v.scope_id()!=0 || v.flowinfo()!=0)
            || capability.fence == 0
            || capability.fence > JSON_SAFE_MAX
            || capability.secret.len() != 64
            || !capability.secret.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Invalid);
        }
        for id in [&capability.dispatch_id, &capability.runner_id, &task_id] {
            identifier(id, 128).map_err(|_| Error::Invalid)?;
        }
        Ok(Self {
            address,
            capability,
            task_id,
            file_tools: false,
            receive_tools: false,
        })
    }
    pub(crate) fn task_id(&self) -> &str {
        &self.task_id
    }
    pub(crate) fn file_tools(&self) -> bool {
        self.file_tools
    }
    pub(crate) fn receive_tools(&self) -> bool {
        self.receive_tools
    }
    pub fn from_env() -> Result<Self, Error> {
        let get = |name, max| {
            std::env::var(name)
                .ok()
                .filter(|v| v.len() <= max)
                .ok_or(Error::Invalid)
        };
        let address = get("HAGENCY_RUNNER_API_ADDR", 128)?
            .parse()
            .map_err(|_| Error::Invalid)?;
        let capability = serde_json::from_str(&get("HAGENCY_RUNNER_CAPABILITY", 4096)?)
            .map_err(|_| Error::Invalid)?;
        let mut context = Self::new(address, capability, get("HAGENCY_TASK_ID", 128)?)?;
        context.file_tools =
            match std::env::var(hagency_runtime::codex::session::TaskMcp::FILE_TOOLS_ENV) {
                Ok(value) if value == "1" => true,
                Err(std::env::VarError::NotPresent) => false,
                _ => return Err(Error::Invalid),
            };
        context.receive_tools =
            match std::env::var(hagency_runtime::codex::session::TaskMcp::RECEIVE_TOOLS_ENV) {
                Ok(value) if value == "1" => true,
                Err(std::env::VarError::NotPresent) => false,
                _ => return Err(Error::Invalid),
            };
        Ok(context)
    }
}

#[derive(clap::Subcommand)]
pub enum Command {
    /// Read the task assigned to this runner.
    Get,
    /// Record a heartbeat of the task already started by the host dispatch.
    Start,
    Heartbeat,
    Wait {
        #[arg(long)]
        reason: String,
        #[arg(long)]
        until: String,
    },
    Resume,
    Done,
    Comment {
        #[arg(long)]
        text: String,
    },
}
impl Command {
    fn operation(&self) -> Result<Option<TaskMutation>, Error> {
        Ok(Some(match self {
            Self::Get => return Ok(None),
            Self::Start | Self::Heartbeat => TaskMutation::Execution {
                heartbeat: true,
                waiting_reason: TextPatch::Missing,
                waiting_until: TextPatch::Missing,
            },
            Self::Wait { reason, until } => {
                text(reason, 1024).map_err(|_| Error::Invalid)?;
                text(until, 64).map_err(|_| Error::Invalid)?;
                TaskMutation::Transition {
                    status: TaskState::Blocked,
                    waiting_reason: Some(reason.clone()),
                    waiting_until: Some(until.clone()),
                }
            }
            Self::Resume | Self::Done => TaskMutation::Transition {
                status: if matches!(self, Self::Resume) {
                    TaskState::InProgress
                } else {
                    TaskState::Done
                },
                waiting_reason: None,
                waiting_until: None,
            },
            Self::Comment { text: body } => {
                text(body, 8192).map_err(|_| Error::Invalid)?;
                TaskMutation::Comment { text: body.clone() }
            }
        }))
    }
}

#[derive(Serialize)]
pub struct Output {
    pub task: Task,
    pub call_id: Option<String>,
    pub replayed: bool,
}

pub async fn run(
    context: &Context,
    command: &Command,
    call_id: Option<&str>,
    deadline: Duration,
) -> Result<Output, Error> {
    run_operation(context, command.operation()?, call_id, deadline).await
}

pub(crate) async fn run_operation(
    context: &Context,
    operation: Option<TaskMutation>,
    call_id: Option<&str>,
    deadline: Duration,
) -> Result<Output, Error> {
    let bytes = transport::request(
        context,
        transport::Operation::Task {
            operation: operation.as_ref(),
            call_id,
        },
        deadline,
    )
    .await?;
    let (task, replayed) = if operation.is_some() {
        let result: MutationResult = serde_json::from_slice(&bytes).map_err(|_| Error::Unknown)?;
        (result.task, result.replayed)
    } else {
        (
            serde_json::from_slice::<Task>(&bytes).map_err(|_| Error::Response)?,
            false,
        )
    };
    if task.id != context.task_id {
        return Err(if operation.is_some() {
            Error::Unknown
        } else {
            Error::Response
        });
    }
    Ok(Output {
        task,
        call_id: call_id.map(str::to_owned),
        replayed,
    })
}
