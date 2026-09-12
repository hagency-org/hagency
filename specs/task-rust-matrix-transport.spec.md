spec: task
name: "Native authenticated Matrix device and observation transport"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-PALPO-OUTBOUND]
tags: [active, rust, matrix, authentication, custody]
---

## Intent

Bootstrap a host-owned Matrix account and persistent SDK device and collect
bounded authenticated observations without enabling event admission or sends.

## Constraints

### Must
- Pin the exact homeserver registration account device and independent transport generation in host-only configuration.
- Verify whoami against the complete configured MXID and device before admitting positive observations.
- Protect encrypted SDK state and crypto storage with exclusive ownership retained through queued mutations and close.
- Preserve device keys on restart and reject foreign identities wrong keys legacy stores unsafe files and concurrent owners.
- Bound HTTPS requests headers bodies JSON events rooms SDK work queues and cancellation without hidden sync loops.
- Require authenticated full room state and host-owned room generation privacy targets before domain observations.
- Persist exact negative transport evidence and atomically retire all old routes grants final and notice authority.
- Refuse positive replay at an unavailable generation and require fresh authenticated evidence at a new generation.
- Reject stale failed bootstrap evidence that targets a newer transport incarnation.

### Must Not
- Do not treat plaintext event JSON browser claims or display names as authenticated event or approval authority.
- Do not send notices messages keys approvals or registration requests to any real service.
- Do not erase or silently recreate an existing cryptographic identity.

## Boundaries

### Allowed Changes
- ./Cargo.toml
- ./Cargo.lock
- native/hagency-matrix/**
- native/hagency-core/src/replies.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/domain/matrix_routes.rs
- native/hagency-store/src/migrations/015-matrix-transport.sql
- native/hagency-store/tests/**
- native/hagency/tests/**
- specs/task-rust-matrix-transport.spec.md
- knowledge/decisions/adr-047-native-matrix-transport.md
- docs/**

### Forbidden
- Production JavaScript deployment data real credentials another working tree and live Matrix or model accounts.

## Acceptance Criteria

Scenario: Matrix account and endpoint are exact host authority
  Test: native_matrix_transport_identity
  Level: integration
  Test Double: local scripted HTTPS and HTTP Matrix responses
  Given pinned host homeserver registration account and device configuration
  When whoami returns matching missing foreign or stale account and device information
  Then only matching authenticated responses admit positive transport observations
  And plaintext event JSON browser claims and display names create no authenticated event or approval authority

Scenario: SDK identity and exclusive private stores survive restart
  Test: native_matrix_transport_storage
  Given encrypted SDK state crypto identity and private store ownership
  When restart wrong key wrong identity unsafe files legacy data or a competing owner is observed
  Then original device keys survive and unsafe or concurrent ownership is refused
  And queued mutations retain exclusive ownership through completion and close

Scenario: HTTP and SDK collection are bounded and cancellable
  Test: native_matrix_transport_bounds
  Level: integration
  Test Double: local scripted HTTP and a paused SQLite owner
  Given scripted oversized duplicate malformed slow and cancelled Matrix responses
  When the collector reads HTTP JSON events rooms and queued SDK work
  Then finite request header body event room and queue budgets reject unsupported input without hidden sync loops

Scenario: Room observation derives from authenticated full state
  Test: native_matrix_transport_rooms
  Given a current verified account and host-owned room generation privacy targets
  When bounded full state proves or contradicts membership encryption and join rules
  Then domain observations preserve exact members and invalidate unsafe or unavailable room authority
  And no notices messages keys approvals or registration requests are sent

Scenario: Failed authentication retires every prior route
  Test: native_matrix_transport_negative
  Given existing sessions grants finals and notices for an authenticated transport
  When exact negative transport evidence is committed
  Then all old routes grants final and notice authority retire atomically
  And positive replay cannot restore the unavailable generation
  And unsafe shared room evidence retires other Agents old room routes

Scenario: Stale and failed transitions preserve current custody
  Test: native_matrix_transport_generation
  Given negative evidence replacement generations restart and concurrent bootstrap responses
  When old responses or transaction rollback are observed
  Then stale failed bootstrap cannot retire a newer incarnation and only fresh authenticated generation can restore availability

Scenario: Native schema upgrade retains older state
  Test: native_matrix_transport_migration
  Given schema fourteen transport state and incomplete or failing upgrade conditions
  When schema fifteen is opened or rolled back
  Then original identities and custody remain intact and missing structure fails visibly

## Out of Scope

This limited SDK observation collector does not complete Matrix event provenance,
encrypted live sends, cross-signing or key-upload/bootstrap workflows, historical
key import, remote provisioning, account registration, live deployment or M5.
