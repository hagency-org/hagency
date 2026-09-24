---
kind: context
id: CTX-NATIVE-PALPO-SERVICE
title: "Original executable ownership of Palpo catalog publication"
status: Accepted
---

The parent approved the exact 15-path task in
specs/task-rust-native-palpo-service.spec.md. It was parsed and linted at 100%
before source edits in an isolated eb80500 tree. The catalog library checkpoint
bac727f was integrated as 508220c, preserving both append-only coordination sections.
The service reuses that qualified library rather than editing its transport/store
source or introducing a separate publication implementation.

Use `hagency serve --state-dir <private-state> --palpo-transport` to opt in.
The separate development-driver flag remains optional. Fixed palpo-transport.json
contains profile=palpo_v2_resources_v1, endpoint, full expected registration and
machine_generation. It accepts no fingerprint, secret inline, arbitrary file path,
environment, runtime setting or timing knob. The separate token and optional CA
are fixed private children, read once with existing no-follow/owner validation and
bounded reads. Production library HTTP limits, 15s publication interval and retry
policy are unchanged; local HTTPS tests allow that original cadence explicitly.

Canonical registration digest means canonical::digest(serde_json::to_value(reg)),
including every validated registration field. It is not the development profile's
supplied string, a partial identity or a token digest. The service derives the
side identity from registration.server_name and uses fixed native-palpo-v2 binding.
The original DomainStore must already contain that same registration. A missing
or mismatched value refuses before Adapter::attach or any outbound HTTP. Empty
init state alone cannot publish: registration provisioning is an explicit separate
prerequisite. Automatic register/rotation would change domain authority before a
possibly refused outbound attachment and is intentionally not part of startup.

Bootstrap owns the original custody Store and DomainStore and passes clones to
one retained async Palpo task only after the real server is polled. The task owns
configuration, HTTP adapter and original run_with_resources future. Its cancellation
token cooperatively stops the library's joined polling/publication loops while
already-received custody and known ACK observations settle. No select discards
the activation or run future; no detached replacement is started. Close keeps the
original JoinHandle through caller cancellation and its two-second observation
timeout, stores a consumed result synchronously, and closes writers only after
the original worker is known complete. A panic/lost join stays Unknown. Ordinary
terminal adapter errors are recorded independently; known loop termination does
not turn external unknown publication into Accepted.

ADR109 checks the original domain registration after custody waits and immediately
before publication HTTP admission. Configuration wiring keeps that API and check.
Already-admitted requests can finish through subsequent registration rotation;
bytes cannot be recalled. The service supplies no transaction across both stores
and the peer, and no publication grants execution permission. The library's own
blocked-writer/registration checks are separate qualification evidence from this
service's executable configuration and retained task/writer tests.

The existing authenticated operator capability endpoint adds a fixed
palpo_publication object with configured/state/error only. Running means the
original task has attached, not that the peer accepted any catalog. The original
development/runtime diagnostic snapshot is untouched. Catalog publication does
not complete Matrix event consumption, work dispatch, allocation readiness,
registration onboarding or production API parity. No browser product flow changes.

Tests seed only legitimate existing domain registration/resources before startup.
The executable itself creates and runs its Adapter, and authenticated local native
HTTP performs subsequent edits. A real HTTPS missing acknowledgment preserves the
same original bytes, sequence and digest through an edit before a newer catalog
can be published. Cancellation retains the pending unknown body and closes the
original writers before they can reopen. A separate gated-task test models slow
join completion and panic; it is not a fabricated network receipt. Unix executable
SIGTERM evidence and cross-platform Bootstrap cancellation are reported separately.
The first executable fixture failure precreated stderr before calling create-new
private::open; its original failure log is retained after removing that redundant
fixture write. A later fixture serialized the Resource roles cache into an API
that correctly forbids caller-supplied roles; its failure remains recorded after
switching the request to the existing provider-configuration input shape. No
deadline or production validation rule changed for either fixture correction.
