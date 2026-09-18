---
kind: decision
id: ADR-171
title: Correlated Codex terminal interaction progress
status: Accepted
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

An isolated long-running command failed unsupported_event after writing its
before-file. A separate original-owner probe with the pinned Codex0.154.0 binary
identified `item/commandExecution/terminalInteraction`; whole-tree cleanup was
observed. Its generated schema requires threadId, turnId, itemId, processId and
stdin strings. Local Codex source inspection also shows empty-stdin polling emits
this event while the process remains live. TS's runner ignores this notification
without interpreting it as completion; Rust's closed protocol lacked its arm.

Admit it only as Progress after exact current thread/turn and active command-item
correlation. Require bounded process identity and at most64KiB stdin. Never retain
stdin, create a command item, grant approval or infer tool success from this event.
Command completion and canonical task completion keep their existing boundaries.
Fixed diagnostic labels identify terminal interaction and patch-update refusals;
recognizing a diagnostic label does not admit file patch updates or unknown events.

The offline regression fails UnsupportedEvent before this change and checks empty
polls, input, unchanged observations, ordinary completion, foreign scope/items,
absent/completed/wrong-kind items, malformed fields, byte bounds and unknown methods.
The prior live failures remain unknown; this probe does not retroactively identify
the first historical failure's lost notification or turn it into success.


Verification: runtime96 passes serially; strict runtime/native all-target Clippy
and locked native build pass. The original-owner pinned-binary probe changes from
UnsupportedEvent to completed with whole-tree cleanup. Separately, the updated
isolated Palpo fleet completes a20-second polled command sent through actual
Robrix, with exact before/after files, canonical Done, no leases and visible
final encrypted reply. The runtime accepts progress without retaining its stdin.
