//! Schemas describe only operations implemented by task_client::coordination.
use serde_json::{Value, json};
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn string(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}
fn ids() -> Value {
    json!({"type":"array","items":string(128),"minItems":1,"maxItems":64,"uniqueItems":true})
}
pub(super) fn tools() -> Vec<Value> {
    let id = string(128);
    let call = string(512);
    let node_id = string(255);
    let seq = json!({"type":"integer","minimum":0,"maximum":9007199254740991_u64});
    let limit = json!({"type":"integer","minimum":1,"maximum":32,"default":8});
    let revision = json!({"type":"integer","minimum":0,"maximum":9007199254740990_u64});
    let definition = object(
        json!({"title":string(255),"description":{"type":"string","maxLength":4096},"priority":{"type":"string","enum":["p0","p1","p2","p3"]},"granularity":{"type":"string","enum":["epic","task","subtask"]},"labels":{"type":"array","items":string(64),"maxItems":32,"uniqueItems":true},"parent_id":{"type":["string","null"],"maxLength":128,"description":"When present must equal the assigned task ID"}}),
        &["title"],
    );
    let condition = object(
        json!({"dep":string(512),"path":string(512),"field":string(512),"op":string(512),"eq":{},"neq":{},"in":{},"value":{}}),
        &[],
    );
    let node = object(
        json!({"id":node_id,"assignee":{"type":"string","minLength":1,"maxLength":128,"description":"Exact internal participant session ID returned by open_conversation, not an engagement ID"},"description":string(4000),"depends_on":{"type":"array","items":node_id,"maxItems":128,"uniqueItems":true},"condition":{"anyOf":[condition,{"type":"null"}]}}),
        &["id", "assignee", "description"],
    );
    let graph = object(
        json!({"label":string(4000),"nodes":{"type":"array","items":node,"minItems":1,"maxItems":128}}),
        &["label", "nodes"],
    );
    let outcome = json!({"oneOf":[object(json!({"kind":{"const":"complete"},"result":{}}),&["kind","result"]),object(json!({"kind":{"const":"failed"},"error":string(4000)}),&["kind","error"])]});
    let entries = [
        (
            "delegate_task",
            "Delegate canonical work to an active engagement in this project. Source sequences must belong to this dispatch/task; omitted root uses the canonical source. This creates pending work, not a host dispatch.",
            true,
            object(
                json!({"call_id":call,"assignee_engagement":id,"root_sequence":{"type":["integer","null"],"minimum":1,"maximum":9007199254740991_u64},"input_sequences":{"type":"array","items":{"type":"integer","minimum":1,"maximum":9007199254740991_u64},"maxItems":100},"definition":definition}),
                &["call_id", "assignee_engagement", "definition"],
            ),
        ),
        (
            "open_conversation",
            "Open an internal conversation with active project engagements. The creator is host-derived; returned participant session IDs address peer messages and graph nodes.",
            true,
            object(
                json!({"call_id":call,"label":string(255),"participant_engagements":ids()}),
                &["call_id", "label", "participant_engagements"],
            ),
        ),
        (
            "get_conversation",
            "Read a currently authorized internal conversation and its exact participant sessions.",
            false,
            object(json!({"conversation_id":id}), &["conversation_id"]),
        ),
        (
            "update_conversation_members",
            "Creator-only revision-checked membership change. Removed sessions lose authority.",
            true,
            object(
                json!({"conversation_id":id,"call_id":call,"expected_revision":revision,"participant_engagements":ids()}),
                &[
                    "conversation_id",
                    "call_id",
                    "expected_revision",
                    "participant_engagements",
                ],
            ),
        ),
        (
            "close_conversation",
            "Creator-only revision-checked close of an internal conversation.",
            true,
            object(
                json!({"conversation_id":id,"call_id":call,"expected_revision":revision}),
                &["conversation_id", "call_id", "expected_revision"],
            ),
        ),
        (
            "send_peer_message",
            "Send durable scoped input to exact current participant session IDs. Requests/responses wake recipients; notifications do not. This does not start a process.",
            true,
            object(
                json!({"call_id":call,"conversation_id":id,"recipient_session_ids":ids(),"kind":{"type":"string","enum":["request","response","notification"]},"priority":{"type":"string","enum":["normal","high","urgent"]},"summary":string(1024),"body":{"type":"string","maxLength":32768},"data":{}}),
                &[
                    "call_id",
                    "conversation_id",
                    "recipient_session_ids",
                    "kind",
                    "summary",
                ],
            ),
        ),
        (
            "read_peer_inbox",
            "Read a bounded page of peer inputs owned by this dispatch. Use the last returned message sequence as after; returns messages.",
            false,
            object(json!({"after":seq,"limit":limit}), &[]),
        ),
        (
            "create_graph",
            "Create a bounded acyclic task graph in an authorized conversation. Assignees are exact participant session IDs. Canonical readiness controls node work; this does not start processes.",
            true,
            object(
                json!({"call_id":call,"conversation_id":id,"definition":graph}),
                &["call_id", "conversation_id", "definition"],
            ),
        ),
        (
            "get_graph",
            "Creator-only graph state and task/session bindings; results are hydrated separately.",
            false,
            object(json!({"graph_id":id}), &["graph_id"]),
        ),
        (
            "list_graphs",
            "Read a bounded page of graphs owned by the creator session. Use the last graph ID as after; returns graphs.",
            false,
            object(
                json!({"after":{"type":"string","maxLength":128},"limit":limit}),
                &[],
            ),
        ),
        (
            "cancel_graph",
            "Creator-only graph cancellation. Started work remains in host stop custody until inspected.",
            true,
            object(
                json!({"graph_id":id,"call_id":call}),
                &["graph_id", "call_id"],
            ),
        ),
        (
            "report_graph_result",
            "Report only this dispatch's exact graph node. Complete requires its canonical task already Done at the current execution epoch; failed requires canonical Blocked and fences further worker authority. No report grant is minted.",
            true,
            object(
                json!({"graph_id":id,"call_id":call,"node_id":node_id,"outcome":outcome}),
                &["graph_id", "call_id", "node_id", "outcome"],
            ),
        ),
        (
            "read_graph_dependencies",
            "Read a bounded page of dependency references permitted to the current node or graph creator; returns dependencies. Result values require a separate read.",
            false,
            object(
                json!({"graph_id":id,"after":seq,"limit":limit}),
                &["graph_id"],
            ),
        ),
        (
            "read_graph_dependency",
            "Hydrate one bounded committed dependency result under exact current node or creator scope. This operation does not mutate even though its native route uses POST.",
            false,
            object(
                json!({"graph_id":id,"node_id":node_id}),
                &["graph_id", "node_id"],
            ),
        ),
    ];
    entries.into_iter().map(|(name,description,mutation,schema)|json!({"name":name,"description":description,"inputSchema":schema,"annotations":{"readOnlyHint":!mutation,"destructiveHint":mutation,"idempotentHint":true,"openWorldHint":false}})).collect()
}
