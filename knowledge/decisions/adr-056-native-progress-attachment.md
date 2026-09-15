---
kind: decision
id: ADR-056
title: Bind redacted progress observations to one exact native upstream turn
status: Accepted
---

## Context

A progress adapter needs actual source-bound runtime observations rather than textual thread and turn IDs alone.

## Decision

`hagency-progress-runtime` attaches ADR052's bounded in-memory policy to actual
ADR036/040 Codex `SessionDriver` and `OwnedSession` events. It produces host-local
redacted progress emissions only. It does not install hooks, add a runner service,
change launch policy, write domain state, send Matrix messages, or complete
ADR026's durable editable status behavior. Existing `Update` payloads, default
approval refusal and process-stop behavior remain unchanged.

The dependency direction is deliberate: runtime owns protocol evidence; progress
owns filtering and summary policy; the new adapter depends on both. Runtime does
not import a policy engine, SQLite store or routing authority. The added probe
binary is a fixed offline native protocol fixture and never invokes a model. Its
JSON and platform dependencies already exist at pinned workspace versions; Cargo
adds one local package and no dependency version changes.

The host constructs an attachment with its immutable `RunId` only after the real
session is Running with exact thread and turn IDs, and before reading an update.
Runtime mints an opaque source containing that exact driver-instance token and
thread/turn. The instance comparison uses retained allocation identity, so another
connection with identical textual IDs cannot impersonate it. Sources and evidence
have no public constructor, Deserialize, Serialize or Debug; runtime arguments,
raw JSON and echoes cannot construct them. Source scope is not domain authority:
the future host must establish and retire the run on dispatch, capability, privacy,
allocation and Agent-incarnation changes. Recreating an old RunId after state loss
is forbidden by the host contract, not prevented by an in-memory global registry.

Opt-in `next_observed_update` returns the existing Update plus a redacted receipt
only after the ordinary session state and received terminal suffix accept it.
Every successful default `next_update` also advances the source sequence; consuming
around an attachment therefore produces a detectable gap. Exact cloned receipts
are idempotent even with an old replay clock. Changed, foreign, gapped, new
backdated or excessive observations irreversibly retire the attachment. No reset,
retarget, recovery import, background read worker or shared mutable anchor exists.

The adapter recognizes only command execution and file-change tool lifecycles.
The source qualification is installed Codex0.153.4's generated v2 schemas and
pinned upstream source commit `3d2ee51ca2d5db578f328aa75e20aa22c0197c9a`, especially
`app-server-protocol/src/protocol/v2/item.rs`, and the command denial/abort
notifications in `app-server/src/bespoke_event_handling.rs`.
[The official app-server protocol](https://learn.chatgpt.com/docs/app-server)
provides the general lifecycle contract; exact pinned status/field evidence takes
precedence over guesses from notification names.

| Accepted protocol evidence | Progress interpretation |
| --- | --- |
| item/started, explicit inProgress | Pending attempt |
| item/started with terminal/missing/future status | Unconfirmed attempt until terminal lifecycle evidence |
| command item/completed, completed, integer int32 exitCode0 | One upstream-observed completed command |
| command item/completed, failed or declined, valid/missing exit | One failed attempt |
| command completed with nonzero/missing/null/noninteger/out-of-range exit | Unresolved attempt |
| fileChange item/completed with completed | One upstream-observed edit |
| fileChange item/completed with failed/declined | One failed attempt |
| missing/future/malformed status | Unresolved attempt |

The projector never parses a command, shell prefix, tool argument or file path to
infer the action or result. Command and file-change categories map to the fixed
Bash/Edit filter names and seven-verb policy vocabulary. File-change completion
is upstream tool evidence, not independent filesystem inspection. Commands with
inconsistent success/status evidence never appear successful. If an explicitly
supplied final turn item status/exit contradicts a previous redacted terminal
observation, only this optional projection is invalidated; the runtime's existing
protocol outcome remains available to its owner. An omitted final item or a final
item with neither status nor exit cannot upgrade earlier evidence.

MCP, dynamic, collaboration, web search, image and sleep items are explicitly gated:
the adapter exposes a bounded fixed diagnostic count of unsupported call starts,
without inventing ACP result evidence or broadening filters. Reasoning, text,
output deltas, retry text, private approval contents, usage and generic notices
produce no tool counts. Their accepted sequence still participates in scope/order
receipts. There is no raw command/input/output/error/path/credential/title in a
progress string, receipt projection, Debug implementation or serialization. The
existing default Update API can still carry runtime text to its intended consumer;
this adapter returns it unchanged and does not relabel it safe for logging.

A typed `ToolEvent` entry in `hagency-progress` uses the same internal call-state
logic as ACP while preserving distinct content-hash domains. Runtime evidence is
mapped directly to this typed entry, never to a fabricated ACP JSON frame.
Existing exact JS behavior vectors and correction vectors still pass. Pending,
failed and completed calls remain separate; a pending notice accepted before
completion does not consume the later completion observation. Filters apply to
starts and terminal outcomes alike. Whole-object perGroup replacement, exclusion,
lifetime totals, coalescing, throttling and the existing one-attempt uncertainty
rules remain in ADR052.

A completed upstream turn can finish the local projection, even when some attempts
remain unresolved. Its summary then says unresolved. Failed, interrupted, invalid
or cancelled sources retire it; they cannot manufacture a finished-success line.
No answer-delivery evidence is passed to `finish`: activity, returned model text,
pipe write, child exit and accepted progress cannot prove an answer reached Matrix.
The adapter's `settle` accepts only existing ADR052 host-local submission inspection;
it is neither a Matrix receipt interface nor a durable outbox. It remains usable
for historical outstanding-attempt settlement after retirement and cannot rearm.

The `next_driver`/`next_owned` helpers borrow the existing owner. Cancelling their
read future retires the projection while the original runtime guard performs its
existing cancellation/stop behavior. The host retains its driver/OwnedSession,
cleanup report and process identity; there is no detached process task or lost
physical owner. External close/drop of an active source invalidates cloned
observations and further claims. A successfully completed source remains valid
only for its old completed projection's final snapshot/settlement; it cannot open
a new attachment or accept another turn. Pending local acceptance becomes Unknown
on retirement, never automatically NotAccepted. Owned failed/interrupted stop may
retire the shared source before the wrapper consumes terminal evidence; this
conservative projection refusal does not erase the owner's exact protocol outcome.
Optional projection failure must be reported by the future host without rewriting
canonical or runtime state. Process stop/drop retain their existing bounded,
blocking platform operations and must remain off HTTP/UI threads.

Bounds are shared and explicit: runtime frames/JSON/item IDs retain existing
ADR032/036 bounds, per-turn evidence tracking is at most MAX_ITEMS, and the adapter
retains at most1024 redacted receipts. Runtime sequences use checked arithmetic;
policy retains at most256 tool IDs and1024 significant events (including host
start/finish), and at most256 submission attempts. Receipts contain bounded opaque
IDs and fixed enums, never raw tool payloads. New timestamps must be monotonic;
quiet/gated observations advance the same clock used by claims and settlement
without adding tool events or policy receipts. Exact replay does not move the clock. Capacity does not evict old receipts, drop
counts silently or continue on a replacement RunId. Claims and settlement retain
the pure policy's atomic clock/state errors; malformed new runtime observations
retire the projection rather than permit continuation with missing evidence.

Tests use both real SessionDriver duplex streams and a real native subprocess via
OwnedSession pipes. They cover identical textual IDs on distinct sources, late
attachment, skipped reads, cloned replay, changed lifecycle, EOF, stale scope,
capacity, backdated events, held receipts after close/drop, borrowed-future
cancellation, mixed explicit/unknown outcomes, contradictory final snapshots,
perGroup filters, native Unicode workspaces, redaction and retained process cleanup.
The child deliberately remains alive after protocol completion so only the host's
existing owner can stop it. No live service, model, token, domain database, Matrix
client, canonical task fixture or synthetic delivery receipt is used.

Remaining gates: current domain-authority attachment in the owned execution worker,
non-Codex typed adapters, broader upstream tool evidence qualification, private
approval status projection, durable editable status intent/custody/recovery, exact
Matrix privacy/route fencing and actual encrypted transport. This slice closes the
local typed runtime-to-policy seam only; it is not operational M6 parity.

## Consequences

The adapter consumes exact ordered evidence and emits redacted host-local progress. It does not mutate task truth, send Matrix events or prove durable editable-status parity.

## Alternatives Considered

Attaching by matching textual IDs, accepting skipped observations or treating cloned evidence as fresh activity would lose source continuity. Importing routing or store authority into runtime would reverse the documented dependency boundary.
