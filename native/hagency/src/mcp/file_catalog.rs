use serde_json::{Value, json};

pub(super) const PENDING_GUIDANCE: &str = "Queued and outcome_unknown are nonterminal: neither confirms delivery or failure. Continue read-only get_file_delivery calls for this same delivery_id, spaced about one second apart, within the original task deadline and current authority. Do not call send_file again, recapture the file, or extend the deadline. Stop polling on delivered, failed, lost authority, or deadline expiry. If still unknown at expiry, report unresolved delivery rather than claiming success or definite failure. File delivery does not mark the task Done.";

pub(super) fn tools() -> Vec<Value> {
    vec![
        json!({
            "name":"send_file",
            "description":"Admit a relative workspace file for the original encrypted conversation. Reuse the same call_id only with identical selection and metadata. Queued is durable admission, not delivery. Queued and outcome_unknown are nonterminal: inspect the same delivery_id with get_file_delivery within the original task deadline; never recapture or send again to clear unknown. File delivery does not mark the task Done.",
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
            "description":"Read the safe status of one original delivery using this helper's existing credential. Queued and outcome_unknown can settle later; neither is a terminal failure. Inspect the same delivery_id about once per second within the original task deadline and authority, stopping on delivered, failed, lost authority or expiry. Historical status grants no current source or send authority and never retries an effect. Delivered is distinct from canonical task Done.",
            "inputSchema":{
                "type":"object",
                "properties":{"delivery_id":{"type":"string","minLength":1,"maxLength":128}},
                "required":["delivery_id"],"additionalProperties":false
            },
            "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        }),
    ]
}
