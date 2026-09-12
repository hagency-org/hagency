spec: task
name: "Enroll an explicit fresh Matrix account and original verified recipient sessions"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, matrix, crypto, enrollment]
---

## Intent

Implement accepted ADR102 so the actual native executable can satisfy ADR101
without fixture-injected SDK trust or sessions. Acceptance authorizes the bounded
implementation; it does not establish executable coverage or a passing result.

## Constraints

### Must
- Use explicit fresh-own-account opt-in and exact operator-provisioned peer public master anchors.
- Check actual protocol version from v1.11 through v1.17 and current whoami room privacy and domain generations before key writes.
- Retain Preparing before generating the original private identity and refuse all existing or ambiguous identities.
- Persist original bounded SDK requests and response capacity before starting each external write.
- Send original device one-time signing and signature uploads in SDK order and require actual complete acknowledgements and fresh verified requery.
- Preserve exact signed claim request identity and require actual persisted Olm sessions for every original verified recipient curve.
- Keep one owner finite queue absolute deadline original response custody and negative fencing after caller loss.
- Recover only validated Complete historical enrollment and never reconstruct a live request from a receipt.
- Require actual native MCP first file delivery and independently proven first Delivered recovery through the original ADR101 selectors.

### Must Not
- Do not import signing seeds reset existing keys complete UIA challenges impersonate application-service users or trust server keys without operator anchors.
- Do not retry unknown requests weaken verified-device checks seed the executable SDK from fixtures or turn refusal into positive evidence.
- Do not change HTTP SDK execution or CI deadlines scheduling sandbox defaults production configuration or Windows durability qualification.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/lib.rs
- native/hagency-matrix/src/config.rs
- native/hagency-matrix/src/collector.rs
- native/hagency-matrix/src/sdk.rs
- native/hagency-matrix/src/sdk/keys.rs
- native/hagency-matrix/src/enrollment.rs
- native/hagency-matrix/src/enrollment/state.rs
- native/hagency-matrix/src/sdk/enrollment.rs
- native/hagency-matrix/tests/enrollment/mod.rs
- native/hagency-matrix/tests/enrollment/fixture.rs
- native/hagency-matrix/tests/enrollment/recovery.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency/src/bootstrap/driver.rs
- native/hagency/tests/file_service/fixture.rs
- native/hagency/tests/file_service.rs
- native/hagency/tests/file_service/recovery.rs
- native/hagency/tests/fixtures/matrix_crypto_peer.rs
- native/hagency/Cargo.toml
- ./Cargo.lock
- knowledge/decisions/adr-102-native-matrix-trust-session-enrollment.md
- specs/task-rust-matrix-trust-session-enrollment.spec.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- No DomainStore schema authority mutation endpoint or capability creation changes are allowed.
- No media staging HTTP sender trust-policy replacement service route or MCP protocol changes are allowed.
- Shared ADR101 bootstrap and executable fixture edits remain assigned to the coordinator; the Matrix enrollment owner changes only its assigned Matrix paths.

## Acceptance Criteria

Scenario: A fresh ordinary account enrolls its original identity
  Test: native_matrix_enrollment_fresh
  Level: integration
  Test Double: Local TLS homeserver with an independent actual recipient SDK
  Given a fresh actual SDK and independently provisioned recipient master anchor
  When actual supported TLS protocol uploads and requery complete
  Then the original persisted own identity and every intended cross-signed recipient pass the unchanged trust policy

Scenario: Existing and concurrent identities are never replaced
  Test: native_matrix_enrollment_existing
  Given existing remote or local signing identity or concurrent different remote keys
  When fresh enrollment observes that state or receives required UIA refusal
  Then it never resets overwrites retries or reports completed enrollment

Scenario: Operator anchors and complete recipient evidence are mandatory
  Test: native_matrix_enrollment_anchors
  Given fixed public anchors and actual fresh query responses
  When a master signature device field membership or complete acknowledgement differs
  Then no unverified recipient becomes trusted and no encrypted file write follows

Scenario: Claimed sessions are real and complete
  Test: native_matrix_enrollment_sessions
  Given a frozen verified user and device set with actual recipient one-time keys
  When the original signed claim is applied or one signature response member or persisted session is missing
  Then only complete valid original sessions can decrypt the actual share and event and all incomplete cases refuse

Scenario: Caller loss preserves the original finite operation
  Test: native_matrix_enrollment_custody
  Level: integration
  Test Double: Held actual SDK database and local TLS boundaries
  Given actual enrollment work held at SDK database and HTTP boundaries
  When its caller drops cancels or times out after admission
  Then original requests results and negative fencing remain owned and no second operation or repeated write starts

Scenario: Ambiguous original phases cannot regenerate
  Test: native_matrix_enrollment_unknown
  Given actual Preparing write possible or Applying work with a lost response or persistence acknowledgement
  When a new owner or process opens the same protected state
  Then uncertainty remains even without loadable private keys and no generation upload or claim repeats

Scenario: Protected enrollment restoration validates original state
  Test: native_matrix_enrollment_restore
  Given actual completed enrollment and its original protected SDK identity and sessions
  When the owner reopens or marker binding key profile or record capacity is invalid
  Then only exact complete state validates and all torn corrupted substituted or oversized states refuse without writes

Scenario: Current authority remains separate from historical enrollment
  Test: native_matrix_enrollment_current_scope
  Given a real original enrollment before its next write or readiness acknowledgement
  When current token room membership or expected domain generation changes
  Then the existing negative fence persists and enrollment never grants current file or execution authority

Scenario: The real executable delivers its first requested file
  Test: native_file_service_executable
  Given the actual native service and MCP child with the explicit fresh enrollment profile
  When the first tool call requests a configured-workspace file
  Then an independent real recipient decrypts exact file bytes and metadata after actual enrollment and only domain event acceptance reports delivered

Scenario: Historical file acceptance survives a fresh process
  Test: native_file_service_restart
  Level: integration
  Test Double: Real native child processes and original protected databases
  Given independently proven actual SDK Complete before failed first domain settlement
  When a fresh process opens the original protected stores
  Then it records first Delivered without recreated capability source capture enrollment replay or HTTP replay

## Decisions

The exact 23-path implementation boundary is accepted and overlaps ADR101 only at
its bootstrap and executable fixture seam. The enrollment agent owns the Matrix
implementation and enrollment tests. The coordinator owns the shared bootstrap,
Cargo and independent recipient executable fixture seam, coordinating ADR101 edits
before compilation. Parsing and linting cannot establish executable coverage; all
ten selectors remain unimplemented or unpassed at acceptance. The unchanged
ADR101 positive Windows gate remains unpassed on unconfirmed directory sync.

Use the existing pinned SDK dependencies. The hagency dev-dependency additions
are only the same matrix-sdk-crypto 0.18.0 and ruma 0.16.0 versions for an independent
real recipient, plus matrix-sdk-base, matrix-sdk-sqlite and
matrix-sdk-store-encryption 0.18.0 to inspect the original protected journal after
the service process is reaped. This inspection creates no SDK machine and writes
no authority or journal values. Cargo.lock may change only for these fixture
dependency declaration. No new executable or configurable crypto provider is added.

## Out of Scope

General Matrix account recovery SAS QR UIA signing seed import application services
background key maintenance live homeservers and production cutover remain separate.
