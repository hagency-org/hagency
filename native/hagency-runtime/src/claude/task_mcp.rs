use super::Error;
use crate::task_mcp::{Profile, TASK_MCP_TOOLS, owned_task_tools};
use serde_json::{Value, json};
use std::path::PathBuf;
pub(crate) const SERVER: &str = "hagency_task_writer";

/// Fixed helper only. No credential, arbitrary args/config, Debug or serde API.
pub struct TaskMcp {
    profile: Profile,
}
impl TaskMcp {
    pub fn new(executable: PathBuf, task_id: String) -> Result<Self, Error> {
        Profile::new(executable, task_id, None)
            .map(|profile| Self { profile })
            .map_err(|_| Error::Input)
    }
    pub fn with_file_tools(mut self) -> Self {
        self.profile = self.profile.with_file_tools();
        self
    }
    pub fn with_receive_tools(mut self) -> Self {
        self.profile = self.profile.with_receive_tools();
        self
    }
    pub(crate) fn tools(&self) -> Vec<&'static str> {
        owned_task_tools(self.profile.file_tools, self.profile.receive_tools)
    }
    pub(crate) fn guidance(&self) -> String {
        self.profile.guidance()
    }
    pub(crate) fn server(&self) -> Value {
        let mut env = serde_json::Map::new();
        if self.profile.file_tools {
            env.insert(Profile::FILE_TOOLS_ENV.into(), json!("1"));
        }
        if self.profile.receive_tools {
            env.insert(Profile::RECEIVE_TOOLS_ENV.into(), json!("1"));
        }
        // Current capability or retained-context reference is inherited. Never
        // interpolate its value into control JSON or argv. Cwd is the parent's.
        json!({"type":"stdio","command":self.profile.executable,"args":["mcp","--owned-task-profile"],
            "env":env,"timeout":5000,"alwaysLoad":true})
    }
}
/// `may_write` comes from the dispatch lease, as in the TS launch path. This
/// profile does not itself grant a lease or enable managed-account admission.
pub fn task_arguments(model: &str, may_write: bool) -> Result<Vec<String>, Error> {
    let mut args = super::arguments_for_workspace(model, may_write)?;
    args.extend(
        [
            "--strict-mcp-config",
            "--mcp-config={\"mcpServers\":{}}",
            "--setting-sources=",
            "--disable-slash-commands",
            "--no-chrome",
            "--no-session-persistence",
        ]
        .map(str::to_owned),
    );
    // Port the existing managed Claude ask rules. Auto mode must not silently
    // bypass the owner's decision for GitHub operations or publishing commits.
    args.push(format!(
        "--settings={}",
        json!({"disableAllHooks":true,
        "permissions":{"ask":["Bash(gh *)","Bash(git push *)"]}})
    ));
    args.push(format!(
        "--allowedTools={}",
        TASK_MCP_TOOLS
            .iter()
            .map(|t| format!("mcp__{SERVER}__{t}"))
            .collect::<Vec<_>>()
            .join(",")
    ));
    Ok(args)
}
