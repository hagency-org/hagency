spec: task
name: "Qualify retained Windows NTFS directory sync for media storage"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, media, windows]
---

## Intent

Supply actual qualified Windows directory sync through production media Store
opening while preserving unconfirmed inspection and exact retained custody.

## Constraints

### Must
- Keep the original retained directory and existing private owner and ACL checks.
- Open only fixed relative dot and compare full live volume and 128-bit object identity before journal creation.
- Admit only completed local mounted NTFS profile evidence with the exact documented optional app-container traversal bit.
- Require real original journal and directory sync acknowledgements for positive preparation or restoration.
- Preserve inspectable directory-unconfirmed outcomes for unavailable or unsupported candidates without an ambient or read-only sync fallback.
- Keep native query buffers and unique original handle owned through any pending completion and retain unknown worker custody after caller timeout.
- Keep the media-store library unsafe-forbid and audit exact pinned FFI layout status and handle lifetimes in the storage private layer.
- Prove the default production Store opening path under ordinary Windows token with actual encrypted stage and separate-process exact restoration.
- Preserve all ADR103 failed original verdicts and distinguish native Windows execution from local cross-compilation.

### Must Not
- Do not terminate the application on native pending status or release buffers still possibly used by IO.
- Do not enable privileges use raw volumes accept remote filesystems alter process deadlines or fabricate directory evidence.
- Do not change domain schema Matrix routing unknown-send recovery runtime profiles or production activation flags.
- Do not reopen caller paths introduce arbitrary handle authority or reconstruct original source custody.

## Boundaries

### Allowed Changes
- native/hagency-store/src/private.rs
- native/hagency-store/src/private/windows_directory.rs
- native/hagency-store/src/private/windows_directory/tests.rs
- native/hagency-store/Cargo.toml
- native/hagency-media-store/src/lib.rs
- native/hagency-media-store/src/tests.rs
- native/hagency-media-store/Cargo.toml
- native/hagency-media-store/examples/windows_directory_flush.rs
- native/hagency-media-store/examples/windows_directory_flush/handles.rs
- native/hagency-media-store/examples/windows_directory_flush/token.rs
- native/hagency-media-store/examples/windows_directory_flush/fixture.rs
- ./Cargo.lock
- .github/workflows/rust.yml
- knowledge/decisions/adr-104-native-windows-directory-sync.md
- specs/task-rust-windows-media-directory-sync.spec.md
- docs/progress.md
- docs/agent-knowledge.md

### Forbidden
- Existing media-store production changes must remain safe Rust.
- Probe FFI is test-only and cannot provide production evidence constructors or process-exit behavior.
- Dependency changes are limited to existing pinned capability and Windows API feature edges with no unrelated version churn.

## Acceptance Criteria

Scenario: Production storage retains the actual syncable directory
  Test: native_media_stage_directory_sync_handle
  Given an original private cap-std directory and ordinary storage construction
  When the Store derives its platform sync owner
  Then supported real file and directory acknowledgements describe that same object
  And replacement or private identity mismatch cannot qualify another root

Scenario: Unknown evidence cannot create qualified media custody
  Test: native_media_restore_capacity_and_durability
  Given original encrypted media and unsupported or refused directory evidence
  When preparation restoration or replay is requested
  Then unconfirmed storage remains inspectable while qualified custody refuses
  And actual qualified original custody keeps exact operation and receipt identity

Scenario: Prepared media retains its original identity across append ordering
  Test: native_media_prepare_exact_identity
  Given actual original encrypted media and a retained preparation
  When another bounded record is appended before the original preparation commits
  Then its original operation digest ciphertext and descriptor remain exact
  And no reopened owner or alternate identity recreates original custody

Scenario: Private storage never repairs an unqualified object
  Test: native_media_stage_platform
  Given actual private storage and an object whose permissions or type no longer qualify
  When admission or recovery checks the retained object
  Then it refuses rather than weakening permissions or accepting another directory
  And no failed platform check supplies durable evidence

Scenario: Only exact supported filesystem characteristics qualify
  Test: native_windows_directory_profile_flags
  Given exact mounted disk flags and the one optional named flag
  When fields are missing unsupported or unknown
  Then the pure gate refuses without inventing an actual native acknowledgement

## Out of Scope

Hardware power-loss proof generic filesystem support upload or service activation
and removing runtime or sandbox qualification gates. The actual Windows example
is an additional required native execution gate; it must exercise default Store
opening and two separate ordinary-token processes. A Windows-only actual-handle
completion fixture must show that caller result loss retains the original worker
and object until a controlled gate releases; this tests ownership, not an observed
native pending result. An exceptional native pending result waits for its exact
original completion. Failed or inconsistent completion waits leave the calling
worker parked with its fixed original storage; no return or admission follows. Local lifecycle tests and
cross-compilation cannot stand in for that platform result. The parent reviewed
and accepted the exact pending-completion design and path manifest before
implementation.
