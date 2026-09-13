---
kind: decision
id: ADR-129
title: Private recovery-artifact and log retention on both runtimes
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

The migration plan's cutover step 9 requires the operator to "keep recovery
artifacts private with documented retention"
(`docs/design/hagency-rust-migration-plan.md:489`). Two gaps that no store slice
closes remain, because both are filesystem surfaces rather than SQLite rows:

- **G7, the unbounded retained log corpus** — the retained product appends four jsonl files with no
  rotation: `system-info.jsonl`, `audit.jsonl`, `messages-archive.jsonl` and
  `message-delivery-events.jsonl` (`backend-v2.js:2781-2784`), written with
  `appendFileSync` (`:3552`, `:3568`). The retained message *array* caps at 5000,
  but the archive it exits into grows forever. **Two of the four are read back** —
  `messages-archive.jsonl` by `archivedMessageExists` (`:4581`), which gates
  re-persistence in `completeMatrixDispatch` (`:4656-4665`), and
  `message-delivery-events.jsonl` by the startup dedupe set
  (`deliveryEventAttemptIds`, `:2787`) and by `readDeliveryEvents` (`:1602`),
  which back `deliveryEventAttemptExists` (`:4572`) and
  `appendDeliveryEventOnce` (`:4576`) — so they are durable state, not pure logs.
- **G8, the unbounded retained media cache** — the retained MCP media cache directory `MEDIA_FETCH_CACHE_DIR`
  has no TTL, no prune and no cap (`lib/mcp-server-core.js:145-146`, `:568`). It is
  created at import (`:146`), written per fetched media file (`:676`) and read back
  by `statReadableFile` on the next request for the same source (`:635-638`).

Both gaps are on the retained side. Native already bounds the equivalent surfaces
structurally (`hagency-media-store/src/types.rs:10-13`; `lib.rs:253-258`) and logs
to stderr rather than to a file (`native/hagency/src/main.rs:138-142`) — so this
ADR's job is to (a) name native's existing posture explicitly rather than leave it
implicit and (b) close the two retained gaps without inventing a second, unbounded
native surface to match them.

**Citation discipline.** An earlier draft of this slice was written against a
different checkout and cited the stderr writer as `native/hagency/src/main.rs:99-104`,
attributing `private::directory` to the platform crate. On **this** tree the writer
is `main.rs:138-142` (`.with_writer(std::io::stderr)` at `:142`) and
`private::directory` is `hagency-store/src/private.rs:16`. This ADR cites this tree
and no working document: every claim below resolves to a path and line in the
repository.

## Decision

### 1. Native logs to stderr; rotation belongs to the service manager

`native/hagency/src/main.rs:138-142` installs `tracing_subscriber::fmt()` with
`.with_writer(std::io::stderr)` (`:142`); the MCP helper additionally writes its
refusal to stderr and exits with a class code (`main.rs:134`,
`std::process::exit(error.exit_code())`, and the dedicated helper's deadline exit
`std::process::exit(74)` at `native/hagency/src/mcp/stdio.rs:43`). Native writes
**no** log file anywhere. The posture is therefore **stderr**, and the retention
question is the service manager's:

- **systemd** — the units in this repo (`hagency-backend.service`,
  `hagency-push-relay.service`, `bridge-matrix.service`) set no
  `StandardOutput=`/`StandardError=`, so stdout/stderr go to **journald**, whose
  `SystemMaxUse`/`MaxRetentionSec` govern lifetime.
- **macOS launchd** — `deploy/com.hagency.supervisor.plist:86-89` sets
  `StandardOutPath`/`StandardErrorPath` under `__RUNTIME__/logs/`, and
  `install/install-macos.sh:342-343` does the same for the installed service.
  **No `newsyslog` entry exists in this repository**, so the lifetime is the
  operator's until one is configured; the installer must **state the period** —
  today `install/install-macos.sh:466` prints log **paths** with no retention
  sentence.
- **Windows** — the service wrapper's redirection is not yet written (M8 defers the
  wrapper; `docs/design/hagency-rust-migration-plan.md:372-391` carries no logging
  claim), so this ADR records the **contract** (private, bounded, documented) and
  defers the mechanism to M8.

### 2. The four retained jsonl files gain rotation, and the two that are read back keep answering

`messages-archive.jsonl`, `system-info.jsonl`, `audit.jsonl` and
`message-delivery-events.jsonl` rotate by size in the maintainer, oldest-first,
keeping the newest N rotations (`bin/hagency-maintain:55-59` already treats
`logs/*.jsonl` as rotatable — `rotate_file_if_needed` `:82`,
`prune_rotated_archives` `:119`, invoked at `:355-356`). **A rotation must not be a
truncate of a read-back file**: for `messages-archive.jsonl`, a dropped line can
re-persist an already-committed message (`:4581` → `:4656-4665`); for
`message-delivery-events.jsonl`, a dropped `attemptId` weakens
`appendDeliveryEventOnce`'s dedupe (`:4576`). Rotation therefore (a) rotates whole
complete lines, never a partial row, (b) keeps the newest rotations **readable and
included in the readers' search**, and (c) is size-gated and best-effort, retried
next tick, never blocking a writer. The readers' search set is widened to "live
file + retained rotations", so `archivedMessageExists` and
`deliveryEventAttemptExists` answer across the rotation boundary. The torn-tail
repair (`repairJsonlTornTail`, `:3416`) is preserved.

### 3. The retained media cache gains a TTL and a cap, swept out of band

`MEDIA_FETCH_CACHE_DIR` entries older than a TTL, or beyond a total byte cap, are
removed oldest-first by the maintainer, **skipping any source named by a pending
delivery**. A failed delete is logged and retried; **a retention failure is never a
work refusal** — the cache prune never refuses a fetch.

### 4. macOS and Windows are honest about what is not implemented

The macOS log lifetime is the operator's until `newsyslog` is configured, and the
installer prints that with the period; the Windows wrapper's log redirection is
deferred to M8. This ADR records both as **documented, not enforced**, so no reader
mistakes silence for a bound.

### 5. Where the work runs — the tick contract and the platform split

**Nothing in this ADR runs on the domain writer.** The retention tick contract
established for the store slices rules that file/log rotation is **not a tick
phase**: its objects are the retained jsonl rotations and the retained media
cache, swept out of band by `bin/hagency-maintain`,
and native media and logs have **no** eviction path by design. It therefore neither occupies nor bounds the
single serial `hagency-domain` writer, and no retention step is added to any tick
phase. The rule "a retention failure is never a work refusal" applies
here unchanged.

**What runs on the filesystem is owned by the platform primitives, not by this
ADR.** The native side needs no new code: a private state directory is established
through `private::directory` (`hagency-store/src/private.rs:16`), the identity of a
retained directory is proven by `directory_identity`/`same_directory`
(`native/hagency-platform/src/directory_identity.rs:14`, `:66`) rather than by a
path string, a created file is sealed per-OS through
`private::seal_created_file_handle` (`private.rs:117`, DACL on Windows and mode on
Unix), and the guardian runs non-dumpable so no core file is produced
(`native/hagency-platform/src/cgroup.rs:311-316`). The media journal's bound is the
store's own (`types.rs:10-13`, `lib.rs:253-258`, service wiring
`native/hagency/src/file_service/recovery.rs:51`). This ADR names those owners and
adds none.

**The one open acceptance item.** The macOS log retention is unimplemented: no
`newsyslog` entry exists anywhere in the repository, and `install/install-macos.sh`
prints log **paths** only (`:466`), with no retention sentence and no period. The
installer must say so as an acceptance item, and this ADR must not claim it.

### 6. Tests, and the selector-presence rule

Every scenario in `specs/task-rust-artifact-retention.spec.md` is bound to a test
that must exist **on every hosted leg**: no file-level OS gate (`#[cfg(unix)]` or
`#[cfg(windows)]` around a whole test function in the spec's binding set), and
where a behaviour genuinely differs by OS, the test names the **refusal** on the
other OS rather than disappearing. A test that only ran on one leg would let a
scenario read as verified while half the release matrix never executed it.

Concretely: `native_logs_to_stderr_with_no_file_sink` asserts the no-file-sink
posture on Linux and macOS and asserts the named not-yet-implemented refusal on
Windows; `native_media_store_capacity_refuses_rather_than_evicts` and
`native_media_store_survives_reopen_with_unsettled_operation` assert the bound and
the survival on every leg; `native_owned_dispatch_fixture_artifacts_are_temp_owned`
and `native_receive_partial_destination_survives_unknown_outcome` assert their
retention on every leg, with any Windows path-shape difference expressed as an
assertion on the canonicalized `PathBuf`, never by skipping. All fixtures are
`tempfile`-owned and no test contacts a live service.

The retained-side negative cases are bound in the sibling contract
`specs/task-rust-artifact-retention-node.spec.md`: a retained Vitest selector can
never appear in the Cargo inventory, so placing those names in a rust-tagged spec
would leave them permanently unbindable.

### 7. No migration

All of this slice is filesystem behaviour plus one maintainer command: no schema
change, no new receipt row, no native migration. The retained jsonl files and the
media cache are bounded in place. `docs/progress.md` records the change.

## Consequences

Good, because the two retained gaps (G7 for the read-back files, G8 for the cache)
gain documented bounds; because the two files that are durable state keep their
readers working across rotation; and because native's existing posture (stderr,
structural media caps) is written down rather than assumed.

Bad, because retention on the retained side is split between the maintainer sweep
and the service manager (two owners, one of them external); because a rotation that
fails is only retried, never guaranteed; and because the macOS and Windows bounds
are documented but not enforced — the honest limit this ADR carries.

## Alternatives Considered

**Truncate the jsonl files instead of rotating them.** Rejected: two of the four are
read back, so a truncate silently drops durable state and can re-persist a
committed message or weaken delivery dedupe.

**Rotate on the request path (`appendDeliveryEvent`/`archivePrunedMessages`).**
Rejected: it puts filesystem work on the hot path and can refuse work; the
never-a-work-refusal rule puts rotation in the maintainer, out of band.

**Give native a file log so both runtimes match.** Rejected: native logs to stderr
by design; inventing a file would add an unbounded native surface to match a
retained gap, and would put a second writer beside the single domain writer.

**Enforce the macOS log lifetime in the installer (write a `newsyslog` entry).**
Deferred, not rejected: today the installer prints paths only; the retention
sentence is an acceptance item, and a `newsyslog` entry is a later variant.

---

## What is retained, for how long, and what is never retained (the docs lane's
adoption note)

**Retained, and for how long.** On the native side: nothing by this ADR —
stderr goes to the service manager (journald's `SystemMaxUse`/`MaxRetentionSec`
on Linux; the operator's own rotation on macOS until a `newsyslog` entry
exists), and the media store is bounded structurally, not by time. On the
retained Node side: the four jsonl files rotate **by size, keeping the newest
N rotations** (N and the size gate are the maintainer's existing
configuration surface, `bin/hagency-maintain`), and `MEDIA_FETCH_CACHE_DIR`
prunes by **TTL or total byte cap** — the numbers are configuration, not
schema, and the installer prints them. **Never retained:** no credential, no
token, no auth material, no owner-identifying log line — the `/credential/`
naming guard and the store's opacity rules apply to every rotation and every
cache entry exactly as to the live files.

**Migration need: none.** Both surfaces are filesystem behaviour plus one
maintainer command; no schema change, no new table, no receipt row. Should
implementation discover one anyway (it is not expected), it takes the next
ledger number, **032**, per the serial chain (028 MA-S1's, 029 MA-S4's, 030
MA-S2's, 031 PC-C1's).
