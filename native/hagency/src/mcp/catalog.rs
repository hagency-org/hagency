use serde_json::{Value, json};
pub(super) fn list(file_tools: bool, receive_tools: bool) -> Value {
    let id = json!({"type":"string","minLength":1,"maxLength":128,"description":"Exact assigned canonical task ID"});
    let call = json!({"type":"string","minLength":1,"maxLength":512,"description":"Stable mutation ID; reuse only with identical task operation content"});
    let mut tools = Vec::new();
    for (name, description, extra, required) in [
        (
            "complete_task_with_reply",
            "Explicitly mark verified work Done and hold final text for the original room until owner cleanup. This retires execution; stop using tools afterwards. Task-only transition remains available when no user reply is needed.",
            json!({"body":{"type":"string","minLength":1,"maxLength":32768}}),
            vec!["body"],
        ),
        (
            "get_task",
            "Read the assigned canonical task",
            json!({}),
            vec![],
        ),
        (
            "accept_task",
            "Accept the assigned task when its canonical state permits",
            json!({}),
            vec![],
        ),
        (
            "transition_task",
            "Explicitly transition the assigned task; blocked requires waiting reason and until",
            json!({"status":{"type":"string","enum":["accepted","in_progress","blocked","done"]},"waiting_reason":{"type":"string","maxLength":1024},"waiting_until":{"type":"string","maxLength":64}}),
            vec!["status"],
        ),
        (
            "comment_task",
            "Append a canonical task comment",
            json!({"text":{"type":"string","minLength":1,"maxLength":8192}}),
            vec!["text"],
        ),
        (
            "update_task_execution",
            "Refresh task heartbeat or set/clear waiting metadata without changing status",
            json!({"heartbeat":{"type":"boolean"},"waiting_reason":{"type":["string","null"],"maxLength":1024},"waiting_until":{"type":["string","null"],"maxLength":64}}),
            vec![],
        ),
    ] {
        let mut properties = extra.as_object().unwrap().clone();
        properties.insert("id".into(), id.clone());
        let mut fields = vec!["id"];
        if name != "get_task" {
            properties.insert("call_id".into(), call.clone());
            fields.push("call_id");
        }
        fields.extend(required);
        tools.push(json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":fields,"additionalProperties":false},"annotations":{"readOnlyHint":name=="get_task","destructiveHint":name!="get_task","idempotentHint":true,"openWorldHint":false}}));
    }
    tools.extend(super::coordination_catalog::tools());
    if file_tools {
        tools.extend(super::file_catalog::tools());
    }
    if receive_tools {
        tools.extend(super::receive_catalog::tools());
    }
    json!({"tools":tools})
}
