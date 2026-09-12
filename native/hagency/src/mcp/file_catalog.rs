use serde_json::{Value, json};

pub(super) fn tools() -> Vec<Value> {
    vec![
        json!({
            "name":"send_file",
            "description":"Admit a relative workspace file for the original encrypted conversation. Reuse the same call_id only with identical selection and metadata. Queued is durable admission, not delivery; outcome_unknown never permits recapture or another send. File delivery does not mark the task Done.",
            "inputSchema":{
                "type":"object",
                "properties":{
                    "call_id":{"type":"string","minLength":1,"maxLength":128,"description":"Stable request identifier, at most 128 UTF-8 bytes"},
                    "path":{"type":"string","minLength":1,"maxLength":4096,"description":"Relative workspace file, at most 4096 UTF-8 bytes and 32 components; no links or traversal"},
                    "filename":{"type":["string","null"],"minLength":1,"maxLength":255,"description":"Optional safe display filename, at most 255 UTF-8 bytes; defaults to the last path component"},
                    "caption":{"type":["string","null"],"maxLength":1000,"description":"Optional caption, at most 1000 UTF-8 bytes"}
                },
                "required":["call_id","path"],"additionalProperties":false
            },
            "annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":true,"openWorldHint":true}
        }),
        json!({
            "name":"get_file_delivery",
            "description":"Read the safe status of one original delivery using this helper's existing credential. Historical status grants no current source or send authority and never retries an effect. Delivered is distinct from canonical task Done.",
            "inputSchema":{
                "type":"object",
                "properties":{"delivery_id":{"type":"string","minLength":1,"maxLength":128}},
                "required":["delivery_id"],"additionalProperties":false
            },
            "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        }),
    ]
}
