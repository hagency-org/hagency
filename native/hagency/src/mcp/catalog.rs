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
            "Read the assigned canonical task, or by id a task this session's dispatches created",
            json!({}),
            vec![],
        ),
        (
            "list_tasks",
            "List the assigned task and the tasks this session's dispatches created (delegations), read-only, in id order; page with after and limit",
            json!({"after":{"type":"string","maxLength":128,"description":"Return tasks whose id sorts after this one; omit for the first page"},"limit":{"type":"integer","minimum":1,"maximum":100,"description":"Page size, 20 when omitted"}}),
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
        (
            "get_approval",
            "Read the approval derived from the assigned task (never an approval id)",
            json!({}),
            vec![],
        ),
        (
            "read_conversation",
            "Read the frozen room discussion for this dispatch, with speaker identities. Start with offset 0 and follow next until null. No room or agent can be selected. Reading alone never completes the task.",
            json!({"offset":{"type":"integer","minimum":0,"description":"Part offset; 0 first, then the previous page's next"}}),
            vec![],
        ),
        (
            "consume_approval",
            "Apply the owner's decision on the approval derived from the assigned task; at-most-once through the stable call_id",
            json!({}),
            vec![],
        ),
    ] {
        let mut properties = extra.as_object().unwrap().clone();
        let mut fields = vec![];
        match name {
            // The list names no task; a read by id defaults to the assigned task.
            "list_tasks" => {}
            "get_task" => {
                properties.insert("id".into(), json!({"type":"string","minLength":1,"maxLength":128,"description":"Task ID; the assigned task when omitted, or a task this session's dispatches created"}));
            }
            _ => {
                properties.insert("id".into(), id.clone());
                fields.push("id");
            }
        }
        let read = matches!(
            name,
            "get_task" | "list_tasks" | "get_approval" | "read_conversation"
        );
        if !read {
            properties.insert("call_id".into(), call.clone());
            fields.push("call_id");
        }
        fields.extend(required);
        tools.push(json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":fields,"additionalProperties":false},"annotations":{"readOnlyHint":read,"destructiveHint":!read,"idempotentHint":true,"openWorldHint":false}}));
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
