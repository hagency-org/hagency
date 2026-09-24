use super::{ControlOutcome, Error, Message, Operation, Phase, SessionDriver};
use crate::claude::{TaskMcp, task_mcp::SERVER};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::Instant,
};

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin, E: AsyncRead + Unpin> SessionDriver<R, W, E> {
    /// Bind once, after the host's actual dispatch/context acknowledgement. This
    /// does not manufacture that authority or expose a generic MCP control API.
    pub async fn bind_task_mcp(&mut self, helper: TaskMcp) -> Result<(), Error> {
        let operation = Operation::new(self, Phase::Ready)?;
        let result = operation.driver.bind_task_inner(helper).await;
        operation.finish(result)
    }
    async fn bind_task_inner(&mut self, helper: TaskMcp) -> Result<(), Error> {
        if self.task_mcp_attempted || self.wire.has_buffered_input() {
            return Err(Error::State);
        }
        self.task_mcp_attempted = true;
        // All three correlated responses share one original lifetime/deadline.
        let until = self.wire.event_deadline();
        let before = self
            .task_control(
                "hagency-claude-mcp-before-1",
                json!({"subtype":"mcp_status"}),
                until,
            )
            .await?;
        if !servers(&before)?.is_empty() {
            return Err(Error::TaskWriterStartup);
        }
        let added = self
            .task_control(
                "hagency-claude-mcp-bind-1",
                json!({"subtype":"mcp_set_servers","servers":{SERVER:helper.server()}}),
                until,
            )
            .await?;
        if added.as_object().is_none_or(|o| o.len() != 3)
            || added["added"] != json!([SERVER])
            || added["removed"] != json!([])
            || added["errors"] != json!({})
        {
            return Err(Error::TaskWriterStartup);
        }
        let after = self
            .task_control(
                "hagency-claude-mcp-after-1",
                json!({"subtype":"mcp_status"}),
                until,
            )
            .await?;
        let [server] = servers(&after)?.as_slice() else {
            return Err(Error::TaskWriterStartup);
        };
        if server["name"] != SERVER || server["status"] != "connected" {
            return Err(Error::TaskWriterStartup);
        }
        let tools = server["tools"].as_array().ok_or(Error::TaskWriterStartup)?;
        let expected = helper.tools().into_iter().collect::<BTreeSet<_>>();
        if tools.len() != expected.len() {
            return Err(Error::TaskWriterStartup);
        }
        let actual = tools
            .iter()
            .map(|v| v["name"].as_str().ok_or(Error::TaskWriterStartup))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if actual != expected {
            return Err(Error::TaskWriterStartup);
        }
        self.task_mcp = Some(helper);
        Ok(())
    }
    async fn task_control(
        &mut self,
        id: &str,
        request: Value,
        until: Instant,
    ) -> Result<Value, Error> {
        self.wire
            .send(crate::claude::control(id, request)?, until)
            .await?;
        match self.wire.next(until).await?.message {
            Message::ControlResponse {
                request_id,
                outcome,
            } if request_id == id => match outcome {
                ControlOutcome::Success(value) => Ok(value),
                ControlOutcome::Refused => Err(Error::TaskWriterStartup),
            },
            _ => Err(Error::Identity),
        }
    }
}
fn servers(value: &Value) -> Result<&Vec<Value>, Error> {
    if value.as_object().is_none_or(|o| o.len() != 1) {
        return Err(Error::TaskWriterStartup);
    }
    value["mcpServers"]
        .as_array()
        .filter(|v| v.len() <= 16)
        .ok_or(Error::TaskWriterStartup)
}
