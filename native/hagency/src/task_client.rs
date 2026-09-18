//! Native maintenance of one already-assigned task through the scoped API.
use hagency_core::{
    JSON_SAFE_MAX,
    project::identifier,
    tasks::{MutationResult, RunnerCapability, Task, TaskMutation, TaskState, TextPatch, text},
};
use serde::{Deserialize, Serialize};
use std::{io::Read, net::SocketAddr, path::PathBuf, time::Duration};

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

/// Default inherited context or one explicit private original owned record.
/// No Debug/Serialize, secret getter or reload exists on the loaded context.
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
        let mut context = Self::from_inherited_values(
            &get("HAGENCY_RUNNER_API_ADDR", 128)?,
            &get("HAGENCY_RUNNER_CAPABILITY", 4096)?,
            &get("HAGENCY_TASK_ID", 128)?,
        )?;
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
    fn from_inherited_values(
        address: &str,
        credential: &str,
        task_id: &str,
    ) -> Result<Self, Error> {
        if address.len() > 128 || credential.len() > 4096 || task_id.len() > 128 {
            return Err(Error::Invalid);
        }
        let address = address.parse().map_err(|_| Error::Invalid)?;
        let value: serde_json::Value =
            serde_json::from_str(credential).map_err(|_| Error::Invalid)?;
        if value.get("profile").is_none() {
            return Self::new(
                address,
                serde_json::from_str(credential).map_err(|_| Error::Invalid)?,
                task_id.into(),
            );
        }
        let reference: ContextReference =
            serde_json::from_str(credential).map_err(|_| Error::Invalid)?;
        if reference.profile != "retained_task_context_v1"
            || !context_digest(&reference.context_id)
            || task_id != format!("context_{}", reference.context_id)
            || !reference.path.is_absolute()
            || reference.path.file_name().and_then(|v| v.to_str())
                != Some(format!("context-{}.json", reference.context_id).as_str())
            || reference.path.canonicalize().ok().as_deref() != Some(reference.path.as_path())
        {
            return Err(Error::Invalid);
        }
        hagency_store::task_context::validate_root(reference.path.parent().ok_or(Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
        let file =
            hagency_store::private::open(&reference.path, false).map_err(|_| Error::Invalid)?;
        if file.metadata().map_err(|_| Error::Invalid)?.len() > 4096 {
            return Err(Error::Invalid);
        }
        let mut bytes = Vec::new();
        file.take(4097)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Invalid)?;
        if bytes.len() > 4096 {
            return Err(Error::Invalid);
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
        if value
            .get("capability")
            .and_then(|v| v.as_object())
            .is_none_or(|v| v.len() != 4)
        {
            return Err(Error::Invalid);
        }
        let record: ContextRecord = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
        if record.version != 1
            || record.context_id != reference.context_id
            || !context_digest(&record.scope)
        {
            return Err(Error::Invalid);
        }
        Self::new(address, record.capability, record.task_id)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextReference {
    profile: String,
    path: PathBuf,
    context_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextRecord {
    version: u8,
    context_id: String,
    scope: String,
    task_id: String,
    capability: RunnerCapability,
}
fn context_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
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

#[cfg(test)]
mod retained_tests {
    use super::*;
    use hagency_store::private;
    use serde_json::{Value, json};
    use std::io::{Seek, Write};
    #[test]
    fn native_task_client_retained_context() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("contexts");
        private::directory(&directory).unwrap();
        let directory = directory.canonicalize().unwrap();
        let id = "a".repeat(64);
        let path = directory.join(format!("context-{id}.json"));
        let marker = format!("context_{id}");
        let reference = json!({"profile":"retained_task_context_v1","path":path,"context_id":id});
        let encoded = serde_json::to_string(&reference).unwrap();
        let cap = RunnerCapability {
            dispatch_id: "dispatch".into(),
            runner_id: "runner".into(),
            fence: 1,
            secret: "b".repeat(64),
        };
        let record = json!({"version":1,"context_id":id,"scope":"c".repeat(64),"task_id":"original_task","capability":cap});
        let bytes = serde_json::to_vec(&record).unwrap();
        let load = |value: &Value, task: &str| {
            Context::from_inherited_values(
                "127.0.0.1:12345",
                &serde_json::to_string(value).unwrap(),
                task,
            )
        };
        assert_eq!(load(&reference, &marker).err(), Some(Error::Invalid));
        assert!(!path.exists());
        private::write_new(&path, &bytes).unwrap();
        let cached = load(&reference, &marker).unwrap();
        assert_eq!(cached.task_id(), "original_task");
        assert!(json!(cached.capability) == json!(cap));
        assert_eq!(
            load(&reference, "original_task").err(),
            Some(Error::Invalid)
        );
        for (field, value) in [
            ("profile", json!("unknown")),
            ("context_id", json!("A".repeat(64))),
            ("extra", json!(true)),
            ("path", json!("relative")),
        ] {
            let mut changed = reference.clone();
            changed[field] = value;
            assert_eq!(load(&changed, &marker).err(), Some(Error::Invalid));
        }
        let mut file = private::open(&path, false).unwrap();
        for (field, value) in [
            ("version", json!(2)),
            ("context_id", json!("d".repeat(64))),
            ("scope", json!("bad")),
            ("task_id", json!("not a task")),
            ("extra", json!(true)),
            (
                "capability",
                json!({"dispatch_id":"dispatch","runner_id":"runner","fence":1,"secret":"b".repeat(64),"extra":true}),
            ),
        ] {
            let mut changed = record.clone();
            changed[field] = value;
            file.set_len(0).unwrap();
            file.rewind().unwrap();
            file.write_all(&serde_json::to_vec(&changed).unwrap())
                .unwrap();
            assert_eq!(load(&reference, &marker).err(), Some(Error::Invalid));
        }
        for changed in [b"partial".to_vec(), vec![b'x'; 4097]] {
            file.set_len(0).unwrap();
            file.rewind().unwrap();
            file.write_all(&changed).unwrap();
            assert_eq!(load(&reference, &marker).err(), Some(Error::Invalid));
        }
        assert_eq!(cached.task_id(), "original_task");
        assert!(json!(cached.capability) == json!(cap));
        file.set_len(0).unwrap();
        file.rewind().unwrap();
        file.write_all(&bytes).unwrap();
        drop(file);
        assert_eq!(
            Context::from_inherited_values("192.0.2.1:12345", &encoded, &marker).err(),
            Some(Error::Invalid)
        );
        let direct = Context::from_inherited_values(
            "127.0.0.1:12345",
            &serde_json::to_string(&cap).unwrap(),
            "original_task",
        )
        .unwrap();
        assert_eq!(direct.task_id(), "original_task");
        #[cfg(unix)]
        {
            use std::fs;
            use std::os::unix::fs::{PermissionsExt, symlink};
            let alias = directory.join("alias");
            symlink(&path, &alias).unwrap();
            let mut changed = reference.clone();
            changed["path"] = json!(alias);
            assert_eq!(load(&changed, &marker).err(), Some(Error::Invalid));
            fs::hard_link(&path, directory.join("linked")).unwrap();
            assert_eq!(load(&reference, &marker).err(), Some(Error::Invalid));
            fs::remove_file(directory.join("linked")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(load(&reference, &marker).err(), Some(Error::Invalid));
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(load(&reference, &marker).err(), Some(Error::Invalid));
        }
    }
}
