spec: task
name: "Retain exact checked upload response body under the original attempt"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, media]
---

## Intent

Preserve exact complete upload-response body bytes and their SHA256 for a future
protected acceptance journal without inventing provenance from reserialized JSON.

## Constraints

### Must
- Construct sealed response evidence only from the actual bounded authenticated-origin HTTP path after status framing EOF and JSON MXC validation.
- Retain the exact at-most4096-byte response body checked MediaId and body SHA256 under the existing finite UploadAttempt permit.
- Preserve current media_id convenience behavior cancellation deadlines and nonrearmable possible-write state.
- Keep whitespace and escape differences in original accepted JSON visible in the body digest.
- Expose only borrowed host getters and distinguish observed historical evidence from successful current execution.

### Must Not
- Do not add raw public constructors Clone Debug serde routes or authority setters to response evidence.
- Do not weaken transport bounds accept truncated data retry POST or expose file keys.
- Do not add journal persistence domain integration service activation or live calls.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/http.rs
- native/hagency-matrix/src/media_upload.rs
- native/hagency-matrix/src/lib.rs
- native/hagency-matrix/tests/media_upload.rs
- knowledge/decisions/adr-083-native-upload-response-custody.md
- specs/task-rust-upload-response-custody.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Exact response body survives JSON projection
  Level: integration
  Test Double: real local TLS upload and actual encrypted source custody
  Test: native_matrix_upload_response_evidence
  Given valid JSON bodies with different whitespace and string escapes
  When the configured homeserver returns each complete body
  Then checked media identifiers agree but exact body bytes and SHA256 differ
  And sealed response evidence cannot be cloned serialized or constructed from model data

Scenario: Invalid or incomplete responses never acquire acceptance evidence
  Level: integration
  Test Double: real local TLS framing and malformed response vectors
  Test: native_matrix_upload_response_bounds
  Given oversized duplicate-key malformed or truncated response bodies
  When transport validation refuses the response
  Then no checked response exists and the original attempt remains nonrearmable

Scenario: Attempt custody remains bounded across cancellation and capacity
  Level: integration
  Test Double: actual local TLS peer with held requests and original attempt permits
  Test: native_matrix_upload_cancellation_custody
  Given a cancelled dropped or timed-out upload request
  When the caller inspects its original attempt
  Then no unverified response is exposed and possible-write custody is preserved

Scenario: Retained successful responses share the original finite capacity
  Level: integration
  Test Double: actual local TLS upload with shared attempt and transfer permits
  Test: native_matrix_upload_capacity
  Given completed uploads whose attempt handles remain held
  When another attempt is admitted
  Then capacity remains held until the original response owner is dropped

Scenario: Transport scope and refusal behavior remain unchanged
  Level: integration
  Test Double: real local TLS origin authentication and response status fixtures
  Test: native_matrix_upload_ciphertext_origin
  Given an actual encrypted original source and configured origin
  When its single POST completes
  Then the token reaches only that origin and no plaintext or key is uploaded

## Out of Scope

Persisted acceptance journal restored-media uploader domain settlement file-event
sending model or MCP tools physical workspace provisioning and production cutover.
