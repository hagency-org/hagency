//! Codex projection of the shared host-only task-helper descriptor.
use super::{Error, Settings};
pub use crate::task_mcp::{COORDINATION_TOOLS, TASK_MCP_ENV, TASK_MCP_TOOLS};
use serde_json::{Value, json};
use std::path::PathBuf;
pub struct TaskMcp {
    profile: crate::task_mcp::Profile,
}
impl TaskMcp {
    pub const FILE_TOOLS_ENV: &'static str = crate::task_mcp::Profile::FILE_TOOLS_ENV;
    pub const RECEIVE_TOOLS_ENV: &'static str = crate::task_mcp::Profile::RECEIVE_TOOLS_ENV;
    pub fn new(
        executable: PathBuf,
        task_id: String,
        system_root: Option<String>,
    ) -> Result<Self, Error> {
        crate::task_mcp::Profile::new(executable, task_id, system_root)
            .map(|profile| Self { profile })
            .map_err(|_| Error::Settings)
    }
    pub fn with_file_tools(mut self) -> Self {
        self.profile = self.profile.with_file_tools();
        self
    }
    pub fn with_receive_tools(mut self) -> Self {
        self.profile = self.profile.with_receive_tools();
        self
    }
    pub fn with_coordination_tools(mut self) -> Self {
        self.profile = self.profile.with_coordination_tools();
        self
    }
    pub(super) fn config(&self, cwd: &str) -> Value {
        let mut environment = TASK_MCP_ENV.to_vec();
        let mut tools = TASK_MCP_TOOLS.to_vec();
        if self.profile.file_tools {
            environment.push(Self::FILE_TOOLS_ENV);
            tools.extend(["send_file", "get_file_delivery"]);
        }
        if self.profile.receive_tools {
            environment.push(Self::RECEIVE_TOOLS_ENV);
            tools.extend(["list_received_files", "receive_file"]);
        }
        if self.profile.coordination_tools {
            tools.extend(COORDINATION_TOOLS);
        }
        let mut config = json!({
            "mcp_servers.hagency_task_writer": {
                "command":self.profile.executable,"args":["mcp"],"cwd":cwd,
                "env_vars":environment,"enabled":true,"required":true,
                "startup_timeout_sec":5,"tool_timeout_sec":5,
                "supports_parallel_tool_calls":false,"enabled_tools":tools
            },
            "shell_environment_policy.inherit":"none",
            "shell_environment_policy.ignore_default_excludes":false,
            "shell_environment_policy.experimental_use_profile":false
        });
        // ADR-021 parity: these exact tools only reach the inherited current
        // dispatch/task writer, which revalidates capability/fence/lease on
        // each call. Never use a server-wide mode or approve optional tools.
        for tool in TASK_MCP_TOOLS {
            config["mcp_servers.hagency_task_writer"]["tools"][tool] =
                json!({"approval_mode":"approve"});
        }
        // ADR-021 names comment_task among its task-maintenance tools. The other
        // coordination tools reach other sessions: optional, never pre-approved.
        if self.profile.coordination_tools {
            config["mcp_servers.hagency_task_writer"]["tools"]["comment_task"] =
                json!({"approval_mode":"approve"});
        }
        if let Some(root) = &self.profile.system_root {
            config["shell_environment_policy.set.SystemRoot"] = root.clone().into();
        }
        config
    }
    pub(super) fn guidance(&self) -> String {
        self.profile.guidance()
    }
}
impl Settings {
    pub fn with_task_mcp(mut self, helper: TaskMcp) -> Self {
        self.task_mcp = Some(helper);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_task_mcp_receive_configuration() {
        let root = std::env::temp_dir();
        let baseline = Settings::new(root.clone(), "offline".into(), "medium".into()).unwrap();
        for (send, receive) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut helper =
                TaskMcp::new(root.join("helper"), "task_assigned".into(), None).unwrap();
            let mut tools = TASK_MCP_TOOLS.to_vec();
            let mut env = TASK_MCP_ENV.to_vec();
            if send {
                helper = helper.with_file_tools();
                tools.extend(["send_file", "get_file_delivery"]);
                env.push(TaskMcp::FILE_TOOLS_ENV);
            }
            if receive {
                helper = helper.with_receive_tools();
                tools.extend(["list_received_files", "receive_file"]);
                env.push(TaskMcp::RECEIVE_TOOLS_ENV);
            }
            let settings = Settings::new(root.clone(), "offline".into(), "medium".into())
                .unwrap()
                .with_task_mcp(helper);
            let params = settings.thread_request(None);
            let mcp = &params["config"]["mcp_servers.hagency_task_writer"];
            assert_eq!(mcp["enabled_tools"], json!(tools));
            assert_eq!(mcp["env_vars"], json!(env));
            assert!(mcp.get("env").is_none());
            assert!(mcp.get("default_tools_approval_mode").is_none());
            assert_eq!(
                mcp["tools"].as_object().unwrap().len(),
                TASK_MCP_TOOLS.len()
            );
            for tool in TASK_MCP_TOOLS {
                assert_eq!(mcp["tools"][tool], json!({"approval_mode":"approve"}));
            }
            for tool in [
                "send_file",
                "get_file_delivery",
                "list_received_files",
                "receive_file",
            ] {
                assert!(mcp["tools"].get(tool).is_none());
            }
            assert_eq!(params["approvalPolicy"], "on-request");
            assert_eq!(params["sandbox"], "workspace-write");
            assert_eq!(params["config"]["shell_environment_policy.inherit"], "none");
            assert_eq!(
                params["config"]["sandbox_workspace_write.network_access"],
                false
            );
            assert_eq!(
                params["config"]["sandbox_workspace_write.writable_roots"],
                json!([])
            );
            assert_eq!(
                settings.turn_request("thread", "input".into())["sandboxPolicy"],
                baseline.turn_request("thread", "input".into())["sandboxPolicy"]
            );
            let guidance = params["developerInstructions"].as_str().unwrap();
            assert_eq!(guidance.contains("list_received_files"), receive);
            assert_eq!(guidance.contains("send_file"), send);
            if receive {
                assert!(guidance.contains("untrusted user input"));
                assert!(guidance.contains("Receiving a file does not mark the task Done"));
            }
        }
    }
    /// ADR180: the coordination group is an explicit host opt-in. Only
    /// comment_task (ADR-021 task maintenance) is pre-approved; every tool that
    /// reaches another session stays an owner-approved optional tool.
    #[test]
    fn native_task_mcp_coordination_configuration() {
        let root = std::env::temp_dir();
        let helper = |coordination: bool| {
            let helper =
                TaskMcp::new(root.join("native-helper"), "task_assigned".into(), None).unwrap();
            let helper = if coordination {
                helper.with_coordination_tools()
            } else {
                helper
            };
            Settings::new(root.clone(), "offline".into(), "medium".into())
                .unwrap()
                .with_task_mcp(helper)
                .thread_request(None)
        };
        let off = helper(false);
        let mcp = &off["config"]["mcp_servers.hagency_task_writer"];
        assert_eq!(mcp["enabled_tools"], json!(TASK_MCP_TOOLS));
        assert!(
            !off["developerInstructions"]
                .as_str()
                .unwrap()
                .contains("delegate_task")
        );

        let on = helper(true);
        let mcp = &on["config"]["mcp_servers.hagency_task_writer"];
        let mut tools = TASK_MCP_TOOLS.to_vec();
        tools.extend(COORDINATION_TOOLS);
        assert_eq!(mcp["enabled_tools"], json!(tools));
        // No marker and no new environment: the helper already serves these.
        assert_eq!(mcp["env_vars"], json!(TASK_MCP_ENV));
        assert_eq!(mcp["args"], json!(["mcp"]));
        assert!(mcp.get("default_tools_approval_mode").is_none());
        let approved = mcp["tools"].as_object().unwrap();
        assert_eq!(approved.len(), TASK_MCP_TOOLS.len() + 1);
        assert_eq!(approved["comment_task"], json!({"approval_mode":"approve"}));
        for tool in COORDINATION_TOOLS.iter().filter(|t| **t != "comment_task") {
            assert!(
                !approved.contains_key(*tool),
                "{tool} must stay owner-approved"
            );
        }
        // Graph tools stay out of the profile.
        for tool in ["create_graph", "get_graph", "list_graphs", "cancel_graph"] {
            assert!(!tools.contains(&tool));
        }
        let guidance = on["developerInstructions"].as_str().unwrap();
        assert!(guidance.contains("delegate_task") && guidance.contains("comment_task"));
        assert_eq!(on["approvalPolicy"], "on-request");
    }
    #[test]
    fn native_task_mcp_host_configuration() {
        let root = std::env::temp_dir();
        let settings = Settings::new(root.clone(), "offline".into(), "medium".into())
            .unwrap()
            .with_task_mcp(
                TaskMcp::new(root.join("native-helper"), "task_assigned".into(), None).unwrap(),
            );
        let params = settings.thread_request(None);
        let config = &params["config"];
        let helper = &config["mcp_servers.hagency_task_writer"];
        assert_eq!(helper["args"], json!(["mcp"]));
        assert_eq!(helper["env_vars"], json!(TASK_MCP_ENV));
        assert!(helper.get("env").is_none());
        assert!(helper.get("url").is_none());
        assert!(helper.get("default_tools_approval_mode").is_none());
        assert_eq!(helper["enabled_tools"], json!(TASK_MCP_TOOLS));
        assert_eq!(config["shell_environment_policy.inherit"], "none");
        assert_eq!(params["approvalPolicy"], "on-request");
        assert_eq!(params["sandbox"], "workspace-write");
        assert!(
            params["developerInstructions"]
                .as_str()
                .unwrap()
                .contains("task_assigned")
        );
        assert_eq!(
            settings.turn_request("thread", "input".into())["sandboxPolicy"]["networkAccess"],
            false
        );
        let file_settings = Settings::new(root.clone(), "offline".into(), "medium".into())
            .unwrap()
            .with_task_mcp(
                TaskMcp::new(root.join("native-helper"), "task_assigned".into(), None)
                    .unwrap()
                    .with_file_tools(),
            );
        let file_params = file_settings.thread_request(None);
        let mut expected_config = config.clone();
        let mut environment = TASK_MCP_ENV.to_vec();
        environment.push(TaskMcp::FILE_TOOLS_ENV);
        let mut tools = TASK_MCP_TOOLS.to_vec();
        tools.extend(["send_file", "get_file_delivery"]);
        expected_config["mcp_servers.hagency_task_writer"]["env_vars"] = json!(environment);
        expected_config["mcp_servers.hagency_task_writer"]["enabled_tools"] = json!(tools);
        assert_eq!(file_params["config"], expected_config);
        assert_eq!(file_params["approvalPolicy"], params["approvalPolicy"]);
        assert_eq!(file_params["sandbox"], params["sandbox"]);
        assert_eq!(
            file_settings.turn_request("thread", "input".into())["sandboxPolicy"],
            settings.turn_request("thread", "input".into())["sandboxPolicy"]
        );
        assert!(
            file_params["developerInstructions"]
                .as_str()
                .unwrap()
                .contains("File delivery does not mark the canonical task Done")
        );
        assert!(
            !params["developerInstructions"]
                .as_str()
                .unwrap()
                .contains("send_file")
        );
        assert!(TaskMcp::new("relative".into(), "task".into(), None).is_err());
        assert!(TaskMcp::new(root.join("helper"), "task\nignore".into(), None).is_err());
        assert!(TaskMcp::new(root.join("helper"), "task".into(), Some("relative".into())).is_err());
    }
}
