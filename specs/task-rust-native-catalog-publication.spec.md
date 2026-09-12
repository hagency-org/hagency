spec: task
name: "Publish current native resource catalogs through original outbound custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, catalog, outbound]
---

## Intent

Implement ADR109's coherent domain observation and automatic adapter publication
partition. Native executable service wiring and browser remote status remain
separate; this partition must not claim them complete.

## Constraints

### Must
- Observe one exact current registration and its qualified resource catalog through the original domain writer without another writable database.
- Compare configured registration generation and canonical fingerprint with actual stored identity when observing a catalog and after custody waits immediately before HTTP admission; retain old frozen custody on refusal.
- Preserve fleet-scoped qualification explicit role withdrawal and cross-family requirements and emit empty offers to withdraw an empty catalog.
- Publish only retained peer DTO fields and public resource identifiers with bounded derived display labels; keep model and identity values exact.
- Refuse more than200 resources per role and peer field-size or complete payload excess without truncating resource lists.
- Keep the original publication lane and exact frozen sequence digest and bytes after unknown HTTP outcomes; changed resources cannot replace pending work.
- Exercise actual local HTTPS and current domain mutations with no live service or account changes.

### Must Not
- Never expose internal preset account path credential or private approval-room data in capabilities.
- Never equate heartbeat catalog acknowledgment or configuration qualification with live Matrix readiness resource allocation or execution authorization.
- Never start another domain writer add a reverse callback or claim current executable wiring or full M5 completion.

## Boundaries

### Allowed Changes
- native/hagency-store/src/lib.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain/catalog_publication.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/catalog_publication.rs
- native/hagency-palpo/src/adapter.rs
- native/hagency-palpo/src/catalog.rs
- native/hagency-palpo/src/lib.rs
- native/hagency-palpo/tests/catalog.rs
- native/hagency-palpo/tests/common/mod.rs
- native/hagency-palpo/Cargo.toml
- ./Cargo.lock
- knowledge/decisions/adr-109-native-catalog-publication.md
- specs/task-rust-native-catalog-publication.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Current qualified catalog is exact and private
  Test: native_catalog_snapshot_scope_and_roles
  Given current original registrations resources and role choices
  When one catalog observation is built through the domain writer
  Then only the exact registration's eligible public resources and identities appear
  And withdrawn roles internal identifiers and stale registrations are refused or absent as appropriate

Scenario: Complete catalog bounds cannot silently omit resources
  Test: native_catalog_snapshot_capacity
  Given native configurations at and beyond the retained peer's finite per-role bounds
  When a complete publication is requested
  Then valid complete data is returned and excess or unsupported wire fields fail without a partial publication

Scenario: Current resource edits publish automatically
  Test: native_catalog_outbound_dynamic
  Level: integration
  Test Double: local HTTPS Palpo peer; real native adapter and domain writer
  Given the actual native outbound loop and local HTTPS peer
  When the original domain writer adds or withdraws a resource
  Then successive acknowledged updates contain the real changed catalog and exact current identity

Scenario: Unknown acknowledgment retains original publication
  Test: native_catalog_outbound_pending
  Given a peer accepts an update but loses its response
  When canonical resources change and the original loop continues
  Then it retries the exact original body sequence and digest before publishing the later catalog

Scenario: Registration rotation stops the old publisher
  Test: native_catalog_outbound_retirement
  Level: integration
  Test Double: local HTTPS Palpo peer; actual domain registration rotation
  Given an original configured registration and publication scope
  When the domain registration rotates or the original loop is cancelled
  Then a fresh pre-send observation refuses the old registration and preserves historical custody without claiming to recall an already admitted HTTP request

Scenario: Custody queue waits cannot retain stale send admission
  Test: native_catalog_outbound_queued_rotation
  Level: integration
  Test Double: local HTTPS Palpo peer; actual original custody SQLite lock
  Given an actual SQLite write lock blocks the original custody writer after the first domain observation
  When the original domain registration rotates before the custody wait finishes
  Then the old frozen publication remains historical and no HTTP request is admitted
