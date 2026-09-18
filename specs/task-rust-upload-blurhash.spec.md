spec: task
name: "Accept Palpo upload metadata without changing upload custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, media]
---

## Intent

Accept the nullable string blurhash member returned by Palpo's upload endpoint.
The isolated live probe returned HTTP200 with content_uri and blurhash:null;
the native exact-single-member parser currently refuses that response.

## Constraints

- Share response validation between actual HTTP acceptance and protected reopen.
- Require one valid content_uri and at most one optional null/string blurhash.
- Preserve strict duplicate-key rejection, the 4096-byte body bound, full EOF,
  header validation, exact original bytes and digests, and unknown-field refusal.
- Metadata confers no delivery, authorization or safe-retry authority.
- Preserve previous uncertain attempts; never repair them using another response.
- Cargo tests use only local fixtures. Live qualification remains explicit opt-in.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/http.rs
- native/hagency-matrix/src/upload_custody.rs
- native/hagency-matrix/tests/media_upload.rs
- native/hagency-matrix/tests/upload_custody/mod.rs
- native/hagency-matrix/tests/fixtures/palpo-upload-response.json
- native/hagency/src/file_service/pipeline.rs
- specs/task-rust-upload-blurhash.spec.md
- knowledge/decisions/adr-167-upload-blurhash.md
- docs/agent-knowledge.md
- docs/plan.md
- docs/progress.md
- docs/design/native-execution-parity.md

## Acceptance Criteria

Scenario: Optional metadata preserves exact upload response evidence
  Test: native_matrix_upload_blurhash_response
  Given an actual local TLS response with null or string blurhash metadata
  When encrypted upload completes
  Then the exact bounded response bytes and their digest are retained
  And a second send is refused

Scenario: Accepted Palpo responses survive protected SDK reopen
  Test: native_matrix_upload_blurhash_custody_reopen
  Given an original accepted response containing Palpo blurhash metadata
  When the original SDK store closes and reopens
  Then historical inspection returns the same accepted receipt without HTTP

Scenario: Malformed or ambiguous metadata is never acceptance
  Test: native_matrix_upload_response_bounds
  Given duplicate metadata or content identity, wrong metadata types or unknown fields
  When upload response parsing completes
  Then no acceptance is exposed and the possible write remains non-retryable

## Out of Scope

General unknown-field compatibility, blurhash rendering, recovery of a discarded
response, automatic retries, provider changes or production cutover.
