//! Typed runner tools. Paths, methods, capability and actor scope are never tool input.
use super::{Context, Error, transport};
use hagency_core::{
    JSON_SAFE_MAX,
    conversations::*,
    graphs::validate_result,
    peers::*,
    project::identifier,
    task_intents::{Delegation, IntentResult},
    tasks::text,
    workflows::*,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::time::Duration;

pub(crate) const NAMES: &[&str] = &[
    "delegate_task",
    "open_conversation",
    "get_conversation",
    "update_conversation_members",
    "close_conversation",
    "send_peer_message",
    "read_peer_inbox",
    "create_graph",
    "get_graph",
    "list_graphs",
    "cancel_graph",
    "report_graph_result",
    "read_graph_dependencies",
    "read_graph_dependency",
];
fn page_limit() -> usize {
    8
}
#[derive(Deserialize)]
#[serde(tag = "name", content = "arguments", deny_unknown_fields)]
pub(crate) enum Command {
    #[serde(rename = "delegate_task")]
    Delegate(Delegation),
    #[serde(rename = "open_conversation")]
    Open(ConversationRequest),
    #[serde(rename = "get_conversation")]
    Conversation { conversation_id: String },
    #[serde(rename = "update_conversation_members")]
    Members {
        conversation_id: String,
        call_id: String,
        expected_revision: u64,
        participant_engagements: Vec<String>,
    },
    #[serde(rename = "close_conversation")]
    Close {
        conversation_id: String,
        call_id: String,
        expected_revision: u64,
    },
    #[serde(rename = "send_peer_message")]
    Send(PeerSend),
    #[serde(rename = "read_peer_inbox")]
    Inbox {
        #[serde(default)]
        after: u64,
        #[serde(default = "page_limit")]
        limit: usize,
    },
    #[serde(rename = "create_graph")]
    Create(WorkflowRequest),
    #[serde(rename = "get_graph")]
    Graph { graph_id: String },
    #[serde(rename = "list_graphs")]
    Graphs {
        #[serde(default)]
        after: String,
        #[serde(default = "page_limit")]
        limit: usize,
    },
    #[serde(rename = "cancel_graph")]
    Cancel { graph_id: String, call_id: String },
    #[serde(rename = "report_graph_result")]
    Report {
        graph_id: String,
        call_id: String,
        node_id: String,
        outcome: WorkflowOutcome,
    },
    #[serde(rename = "read_graph_dependencies")]
    Dependencies {
        graph_id: String,
        #[serde(default)]
        after: u64,
        #[serde(default = "page_limit")]
        limit: usize,
    },
    #[serde(rename = "read_graph_dependency")]
    Dependency { graph_id: String, node_id: String },
}
impl Command {
    pub(crate) fn parse(name: &str, arguments: Value) -> Result<Self, Error> {
        serde_json::from_value(json!({"name":name,"arguments":arguments}))
            .map_err(|_| Error::Invalid)
    }
    pub(super) fn mutates(&self) -> bool {
        !matches!(
            self,
            Self::Conversation { .. }
                | Self::Inbox { .. }
                | Self::Graph { .. }
                | Self::Graphs { .. }
                | Self::Dependencies { .. }
                | Self::Dependency { .. }
        )
    }
    pub(super) fn validate(&self, context: &Context) -> Result<(), Error> {
        let id = |s: &str| identifier(s, 128).map_err(|_| Error::Invalid);
        let call = |s: &str| identifier(s, 512).map_err(|_| Error::Invalid);
        let page = |after: u64, limit: usize| {
            if after <= JSON_SAFE_MAX && (1..=32).contains(&limit) {
                Ok(())
            } else {
                Err(Error::Invalid)
            }
        };
        match self {
            Self::Delegate(v) => {
                call(&v.call_id)?;
                id(&v.assignee_engagement)?;
                v.definition.validate().map_err(|_| Error::Invalid)?;
                if v.definition
                    .parent_id
                    .as_deref()
                    .is_some_and(|id| id != context.task_id())
                    || v.input_sequences.len() > 100
                    || v.input_sequences
                        .iter()
                        .any(|n| *n == 0 || *n > JSON_SAFE_MAX)
                    || v.root_sequence.is_some_and(|n| n == 0 || n > JSON_SAFE_MAX)
                {
                    return Err(Error::Invalid);
                }
                Ok(())
            }
            Self::Open(v) => v.validate().map_err(|_| Error::Invalid),
            Self::Members {
                conversation_id, ..
            }
            | Self::Close {
                conversation_id, ..
            } => {
                id(conversation_id)?;
                self.change()
                    .unwrap()
                    .validate()
                    .map_err(|_| Error::Invalid)
            }
            Self::Conversation { conversation_id } => id(conversation_id),
            Self::Send(v) => v.validate().map_err(|_| Error::Invalid),
            Self::Inbox { after, limit } => page(*after, *limit),
            Self::Create(v) => v.validate().map_err(|_| Error::Invalid),
            Self::Graph { graph_id } => id(graph_id),
            Self::Graphs { after, limit } => {
                if !after.is_empty() {
                    id(after)?;
                }
                page(0, *limit)
            }
            Self::Cancel { graph_id, call_id } => {
                id(graph_id)?;
                call(call_id)
            }
            Self::Report {
                graph_id,
                call_id,
                node_id,
                outcome,
            } => {
                id(graph_id)?;
                call(call_id)?;
                text(node_id, 255).map_err(|_| Error::Invalid)?;
                match outcome {
                    WorkflowOutcome::Complete { result } => {
                        validate_result(result).map_err(|_| Error::Invalid)
                    }
                    WorkflowOutcome::Failed { error } => {
                        text(error, 4000).map_err(|_| Error::Invalid)
                    }
                }
            }
            Self::Dependencies {
                graph_id,
                after,
                limit,
            } => {
                id(graph_id)?;
                page(*after, *limit)
            }
            Self::Dependency { graph_id, node_id } => {
                id(graph_id)?;
                text(node_id, 255).map_err(|_| Error::Invalid)
            }
        }
    }
    fn change(&self) -> Option<ConversationChange> {
        match self {
            Self::Members {
                call_id,
                expected_revision,
                participant_engagements,
                ..
            } => Some(ConversationChange {
                call_id: call_id.clone(),
                expected_revision: *expected_revision,
                action: ConversationAction::Members {
                    participant_engagements: participant_engagements.clone(),
                },
            }),
            Self::Close {
                call_id,
                expected_revision,
                ..
            } => Some(ConversationChange {
                call_id: call_id.clone(),
                expected_revision: *expected_revision,
                action: ConversationAction::Close {},
            }),
            _ => None,
        }
    }
    pub(super) fn wire(&self) -> Result<(String, Vec<u8>, &'static str), Error> {
        let (path, body) = match self {
            Self::Delegate(v) => ("delegations".into(), Some(json!(v))),
            Self::Open(v) => ("conversations".into(), Some(json!(v))),
            Self::Conversation { conversation_id } => {
                (format!("conversations/{conversation_id}"), None)
            }
            Self::Members {
                conversation_id, ..
            }
            | Self::Close {
                conversation_id, ..
            } => (
                format!("conversations/{conversation_id}/operations"),
                Some(json!(self.change().unwrap())),
            ),
            Self::Send(v) => ("peer-messages".into(), Some(json!(v))),
            Self::Inbox { after, limit } => {
                (format!("peer-inbox?after={after}&limit={limit}"), None)
            }
            Self::Create(v) => ("graphs".into(), Some(json!(v))),
            Self::Graph { graph_id } => (format!("graphs/{graph_id}"), None),
            Self::Graphs { after, limit } => (format!("graphs?after={after}&limit={limit}"), None),
            Self::Cancel { graph_id, call_id } => (
                format!("graphs/{graph_id}/cancel"),
                Some(json!(WorkflowCancel {
                    call_id: call_id.clone()
                })),
            ),
            Self::Report {
                graph_id,
                call_id,
                node_id,
                outcome,
            } => (
                format!("graphs/{graph_id}/results"),
                Some(json!(WorkflowResultRequest {
                    call_id: call_id.clone(),
                    node_id: node_id.clone(),
                    outcome: outcome.clone()
                })),
            ),
            Self::Dependencies {
                graph_id,
                after,
                limit,
            } => (
                format!("graphs/{graph_id}/dependencies?after={after}&limit={limit}"),
                None,
            ),
            Self::Dependency { graph_id, node_id } => (
                format!("graphs/{graph_id}/dependencies"),
                Some(json!(DependencyRequest {
                    node_id: node_id.clone()
                })),
            ),
        };
        let method = if body.is_some() { "POST" } else { "GET" };
        Ok((
            format!("/api/native/v1/runner/{path}"),
            body.map(|v| serde_json::to_vec(&v))
                .transpose()
                .map_err(|_| Error::Invalid)?
                .unwrap_or_default(),
            method,
        ))
    }
    fn project(&self, context: &Context, value: Value) -> Result<Value, ()> {
        // Decode and reserialize the existing bounded DTO projections, never relay arbitrary
        // server fields. Match response scope before the receipt becomes tool output.
        fn decode<T: DeserializeOwned>(value: Value) -> Result<T, ()> {
            serde_json::from_value(value).map_err(|_| ())
        }
        fn encode<T: Serialize>(value: T) -> Result<Value, ()> {
            serde_json::to_value(value).map_err(|_| ())
        }
        match self {
            Self::Delegate(_) => encode(decode::<IntentResult>(value)?),
            Self::Open(_) => encode(decode::<ConversationResult>(value)?),
            Self::Conversation { conversation_id } => {
                let v = decode::<Conversation>(value)?;
                if v.id != *conversation_id {
                    return Err(());
                }
                encode(v)
            }
            Self::Members {
                conversation_id, ..
            }
            | Self::Close {
                conversation_id, ..
            } => {
                let v = decode::<ConversationResult>(value)?;
                if v.conversation.id != *conversation_id {
                    return Err(());
                }
                encode(v)
            }
            Self::Send(_) => encode(decode::<PeerReceipt>(value)?),
            Self::Inbox { after, limit } => {
                let v = decode::<Vec<PeerInboxItem>>(value)?;
                if v.len() > *limit
                    || (v.first().is_some_and(|i| i.message.sequence <= *after)
                        || v.windows(2)
                            .any(|w| w[0].message.sequence >= w[1].message.sequence))
                {
                    return Err(());
                }
                Ok(json!({"messages":v}))
            }
            Self::Create(req) => {
                let v = decode::<WorkflowReceipt>(value)?;
                if v.workflow.conversation_id != req.conversation_id {
                    return Err(());
                }
                encode(v)
            }
            Self::Cancel { graph_id, .. } => {
                let v = decode::<WorkflowReceipt>(value)?;
                if v.workflow.id != *graph_id {
                    return Err(());
                }
                encode(v)
            }
            Self::Graph { graph_id } => {
                let v = decode::<WorkflowView>(value)?;
                if v.id != *graph_id {
                    return Err(());
                }
                encode(v)
            }
            Self::Graphs { after, limit } => {
                let v = decode::<Vec<WorkflowSummary>>(value)?;
                if v.len() > *limit
                    || (v.first().is_some_and(|g| g.id <= *after)
                        || v.windows(2).any(|w| w[0].id >= w[1].id))
                {
                    return Err(());
                }
                Ok(json!({"graphs":v}))
            }
            Self::Report {
                graph_id, node_id, ..
            } => {
                let v = decode::<WorkflowResultReceipt>(value)?;
                if v.graph_id != *graph_id
                    || v.node_id != *node_id
                    || v.task_id != context.task_id()
                {
                    return Err(());
                }
                encode(v)
            }
            Self::Dependencies { after, limit, .. } => {
                let v = decode::<Vec<DependencyRef>>(value)?;
                if v.len() > *limit
                    || (v.first().is_some_and(|i| i.sequence <= *after)
                        || v.windows(2).any(|w| w[0].sequence >= w[1].sequence))
                {
                    return Err(());
                }
                Ok(json!({"dependencies":v}))
            }
            Self::Dependency { node_id, .. } => {
                let v = decode::<DependencyValue>(value)?;
                if v.dependency.node_id != *node_id {
                    return Err(());
                }
                encode(v)
            }
        }
    }
}
pub(crate) async fn run(
    context: &Context,
    command: &Command,
    deadline: Duration,
) -> Result<Value, Error> {
    let bytes = transport::request(
        context,
        transport::Operation::Coordination(command),
        deadline,
    )
    .await?;
    let failure = if command.mutates() {
        Error::Unknown
    } else {
        Error::Response
    };
    let value = crate::mcp::json::json(&bytes).map_err(|_| failure)?;
    command.project(context, value).map_err(|_| failure)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_mcp_coordination_catalog_arguments() {
        let cases = [
            (
                "delegate_task",
                json!({"call_id":"d","assignee_engagement":"agent","definition":{"title":"Work"}}),
            ),
            (
                "open_conversation",
                json!({"call_id":"o","label":"Work","participant_engagements":["agent"]}),
            ),
            ("get_conversation", json!({"conversation_id":"c"})),
            (
                "update_conversation_members",
                json!({"conversation_id":"c","call_id":"m","expected_revision":0,"participant_engagements":["agent"]}),
            ),
            (
                "close_conversation",
                json!({"conversation_id":"c","call_id":"x","expected_revision":0}),
            ),
            (
                "send_peer_message",
                json!({"call_id":"s","conversation_id":"c","recipient_session_ids":["session"],"kind":"request","summary":"Work"}),
            ),
            ("read_peer_inbox", json!({})),
            (
                "create_graph",
                json!({"call_id":"g","conversation_id":"c","definition":{"label":"Work","nodes":[{"id":"n","assignee":"session","description":"Work"}]}}),
            ),
            ("get_graph", json!({"graph_id":"g"})),
            ("list_graphs", json!({})),
            ("cancel_graph", json!({"graph_id":"g","call_id":"x"})),
            (
                "report_graph_result",
                json!({"graph_id":"g","call_id":"r","node_id":"n","outcome":{"kind":"complete","result":{"capability":"opaque result data is not authority"}}}),
            ),
            ("read_graph_dependencies", json!({"graph_id":"g"})),
            (
                "read_graph_dependency",
                json!({"graph_id":"g","node_id":"n"}),
            ),
        ];
        assert_eq!(
            cases
                .iter()
                .map(|(name, _)| *name)
                .collect::<std::collections::BTreeSet<_>>(),
            NAMES.iter().copied().collect()
        );
        for (name, args) in cases {
            assert!(Command::parse(name, args.clone()).is_ok(), "{name}");
            for field in [
                "actor",
                "capability",
                "path",
                "method",
                "url",
                "session_id",
                "registration",
                "report_grant",
            ] {
                let mut extra = args.clone();
                extra[field] = json!("untrusted");
                assert!(Command::parse(name, extra).is_err(), "{name}: {field}");
            }
            if args.get("call_id").is_some() {
                let mut missing = args.clone();
                missing.as_object_mut().unwrap().remove("call_id");
                assert!(
                    Command::parse(name, missing).is_err(),
                    "{name}: missing call ID"
                );
            }
        }
    }
}
