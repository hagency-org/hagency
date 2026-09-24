//! Exact factual probe association. No permission response or execution API.
use hagency_runtime::codex::{
    approval::ApprovalRequest,
    session::{ItemPhase, ObservationKind, ToolKind, ToolResult},
};
use std::path::{Path, PathBuf};

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn double_quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$")
            .replace('`', "\\`")
    )
}

pub struct Probe {
    cwd: PathBuf,
    command: String,
    renderings: Vec<String>,
    item_id: Option<String>,
    multiple: bool,
    unrelated: bool,
    invalidated: bool,
    attempted: bool,
    completed: bool,
    failed: bool,
    approval: bool,
    observed_commands: usize,
    observed_other_tools: usize,
}
pub struct Witness {
    pub valid: bool,
    pub attempted: bool,
    pub completed: bool,
    pub failed: bool,
    pub approval: bool,
    pub observed_commands: usize,
    pub observed_other_tools: usize,
}
impl Probe {
    pub fn new(cwd: &Path, target: &Path) -> Self {
        let command = format!(
            "printf qualified > {}",
            quote(target.to_str().unwrap_or_default())
        );
        let mut renderings = vec![command.clone()];
        // Exact full renderings only. No substring, basename or target-only
        // fallback: an unknown pinned renderer must remain an unqualified run.
        for (shell, flag) in [
            ("/bin/bash", "-lc"),
            ("/bin/bash", "-c"),
            ("/bin/sh", "-c"),
            ("/bin/zsh", "-lc"),
            ("/bin/zsh", "-c"),
        ] {
            for quoted in [quote(&command), double_quote(&command)] {
                renderings.push(format!("{shell} {flag} {quoted}"));
            }
        }
        Self {
            cwd: cwd.into(),
            command,
            renderings,
            item_id: None,
            multiple: false,
            unrelated: false,
            invalidated: false,
            attempted: false,
            completed: false,
            failed: false,
            approval: false,
            observed_commands: 0,
            observed_other_tools: 0,
        }
    }
    pub fn command(&self) -> &str {
        &self.command
    }
    fn item(&mut self, id: &str) {
        match &self.item_id {
            None => self.item_id = Some(id.into()),
            Some(original) if original != id => self.multiple = true,
            _ => {}
        }
    }
    pub fn observe(&mut self, kind: &ObservationKind) {
        match kind {
            ObservationKind::Invalidated => self.invalidated = true,
            ObservationKind::Tool(tool) => {
                if tool.phase() == ItemPhase::Active {
                    if tool.kind() == ToolKind::Command {
                        self.observed_commands += 1;
                    } else {
                        self.observed_other_tools += 1;
                    }
                }
                self.item(tool.id());
                if tool.kind() != ToolKind::Command
                    || !self
                        .renderings
                        .iter()
                        .any(|command| tool.matches_command(command, &self.cwd))
                {
                    self.unrelated = true;
                    return;
                }
                self.attempted = true;
                self.completed |= tool.result() == ToolResult::Completed;
                self.failed |= tool.result() == ToolResult::Failed;
            }
            _ => {}
        }
    }
    pub fn observe_approval(&mut self, request: &ApprovalRequest) {
        self.item(request.item_id());
        let params = request.params();
        let matches = request.method() == "item/commandExecution/requestApproval"
            && params
                .get("networkApprovalContext")
                .is_none_or(|value| value.is_null())
            && params
                .get("command")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|command| self.renderings.iter().any(|original| original == command))
            && params.get("cwd").and_then(serde_json::Value::as_str) == self.cwd.to_str();
        if matches {
            self.approval = true;
        } else {
            self.unrelated = true;
        }
    }
    pub fn witness(&self) -> Witness {
        let valid =
            !self.multiple && !self.unrelated && !self.invalidated && self.item_id.is_some();
        Witness {
            valid,
            attempted: valid && self.attempted,
            completed: valid && self.completed,
            failed: valid && self.failed,
            approval: valid && self.approval,
            observed_commands: self.observed_commands,
            observed_other_tools: self.observed_other_tools,
        }
    }
}
