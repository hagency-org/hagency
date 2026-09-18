# Native migration working plan

This is coordination, not canonical runtime task state. There is no provisioned
`task-writer` in this source checkout. User instruction: execute the Rust migration
in a clean worktree. Branch `feat/rust-migration`; baseline `5dbef22`.

Current operator topology (2026-09-16): use LOCAL Codex, Claude Code and OctosCode
on this Mac, mini3 only for Palpo, and the actual Robrix client. This supersedes
the fresh mini3 Codex-login prerequisite, not any migration gate. The local tools
are installed; Codex/Claude provider status reports signed in. Native Claude and
Octos production joins and full three-runner qualification remain open. Focused
native macOS descendant-stop and two local Codex/Palpo/Robrix paths now pass;
Codex file send/receive, owner approvals, explicit live recovery and the five-minute
execution deadline now pass; broader isolation and sustained qualification remain open.
Follow `REQ-LOCAL-THREE-RUNNER-QUALIFICATION`; do not substitute the old Node bridge,
an Octos mock, remote Linux execution or imported credentials for this target.
The obsolete remote-login helper is disabled. Its unused private state and the
existing running remote service remain preserved. Continue with the native
local authentication/runner capability design and its bound implementation tests
before attempting a three-runner live qualification or asserting full parity.

Operator working order (2026-09-16): make it work, make it right, make it fast.
Latest explicit sequencing: finish and qualify Codex end to end FIRST, including
its files, private owner approvals, recovery/restart and sustained soak. Only
after that passes join/qualify Claude Code; leave OctosCode until last. Do not
switch to another adapter to avoid an unresolved Codex vertical-path failure.
The active execution map is `docs/design/native-execution-parity.md`: trace each
TS path into Rust, port missing behavior against existing tests, run the same
Palpo/Robrix path, and fix observed differences. Safety/recovery checks preserve
that path's contract and identify its gaps; adapter-only successes do not replace
end-to-end acceptance. ADR160 ports Codex MCP approvals; ADR161 carries explicit
long-task budgets through execution, host capability and approval custody.
ADR164 now supplies offline-qualified explicit continuation for stopped Codex
failures that retained ADR162 proof. It preserves unknown outcomes and transfers
inputs to a new instruction atomically; it cannot recover the historical failure
without its missing original receipt. ADR165 now supplies expiring inspection
credentials and atomic continue/accept_completed/keep_blocked decisions through
lifecycle console routes. ADR170 exposes these actions in the browser; graph terminal
resolution remains open. The authorized isolated fleet now runs the new slices.
Continue Codex file/approval recovery and restart routing in the same setup.
Finish its end-to-end and soak gates before Claude, then OctosCode. Optimize
measured bottlenecks after the corresponding functional path is qualified.

Isolated qualification checkpoint (2026-09-17): the operator approved new test
accounts/rooms on existing Palpo. First instance passed text but exposed the
approval-cursor/Robrix-ID bugs; file002 then retained an unknown upload. Preserve
that original owner (PID20841) and its SDK/state; it intentionally refuses close.
A fresh independent instance with ADR166/167 passes real owner approval, encrypted
file delivery, canonical Done, whole-tree cleanup, and final reply in Robrix.
Authenticated downloaded media decrypts to the exact34 original bytes; the native
Save dialog remains unqualified under the headless harness. No earlier failure
is reclassified. The successful instance closed cleanly; explicit fresh-generation/session restart
then passed a new workspace task and encrypted Robrix reply without resending the
original file. Receive001 subsequently passes encrypted SDK attachment intake,
actual Robrix approval, exact received bytes, canonical Done and rendered reply.
The first repeated-run pulse then failed with an unsupported notification. Its
original stop/inventory proof is retained. ADR165 accepted a reviewed continuation,
but the old continuous worker had already exited its loop, leaving it queued.
ADR168 adds the missing fixed refusal diagnostic; ADR169 joins explicit resolution
back to the retained continuous worker. Its actual-process offline test passes.
After a clean restart, the isolated service completed12 new tasks over17 minutes:
exact files, canonical Done, encrypted replies, independent exact visible Robrix
reply audit and zero leases. The old queued continuation became superseded during
explicit session003 provisioning; it never ran. ADR170 adds the lifecycle recovery
UI with English/Chinese labels, private inspection and explicit identical retry
on response loss. Browser, focused UI, serial console60 and strict Clippy pass.
On generation4/session004, actual browser inspection and explicit continuation
now pass through the same live worker, local Codex completion and exact Robrix
reply. The original failed dispatch remains unknown. A separate pinned-binary
probe identifies terminal-interaction progress as a missing protocol arm; ADR171
ports its scoped handling. The installed-binary probe now completes, and a live
20-second polled command passes exact files/Done/encrypted Robrix reply on
generation5/session005 (PID15469, port19431). Full runtime96, strict Clippy and
locked build pass. A new long task then reaches the unchanged300-second deadline,
retains whole-tree-stop/inventory proof and leaves its after-file absent. Actual
English/Chinese console inspection and explicit continuation pass on the same
PID15469, with exact bytes, canonical Done and rendered encrypted Robrix reply.
The original dispatch stays unknown. Preserve the unsupported-event failure and
separate PID20841 media owner. Finish groups/DMs, uncertain-media recovery and sustained
qualification before Claude, then Octos. Full migration is open.

The next source slices are ADR172 authenticated optional project-inbox adoption
and ADR173 provider-owned local Codex in the actual configured fleet. Adoption
passes42 library/20 bootstrap/8 CLI tests. Local factory qualification passes a
real native service with two offline agents, two rounds, independent encrypted
files/replies and no manufactured managed login. Original-directory replacement,
resource mismatch and distinct warm/active budgets pass. Strict all-target
native/execution Clippy passes. The new authorized isolated fleet has three accounts/four rooms and passed
startup. At21:33:15 UTC both it and the earlier successful service hit Remote(429),
fenced their transports/DM scopes and stopped intake. Both later closed cleanly;
original SDK stores and receipts remain. ADR174 adds bounded GET-only429 waits
within one request deadline; focused TLS3 and full Matrix224 now pass, along with strict all-target Clippy and
locked build. Explicit fresh-generation recovery restored root2 PID17433/19431
(transport6/session006/DM3) and root3 PID18059/19432 (transport2/DM3). Root3 then
physically provisioned two local Codex agents and passed two private-DM rounds:
four exact files/canonical Done/encrypted delivered replies, actual Robrix captures
and zero leases. Both tasks were observed Started together in round2. Complete
project history through m.room.create contains none of their private markers.
This closes that bounded two-agent DM case, not shared-room intake, fleet media
approval or sustained qualification. Original unknown-upload PID20841 stays retained.

Current integration priorities (2026-09-10): ADR096/097/098/099/100 are integrated.
One actual development attempt now authenticates the original Collector, claims
compatible work and registers the original Started workspace before launch.
Schema20 separates immutable file metadata, upload acceptance and encrypted room
event delivery. The publisher retains original ciphertext/claim custody and
checks current identity, privacy and task authority at the write boundary.
Actual recipient SDK fixtures decrypt exact metadata/bytes and frozen relations.
A protected Complete can settle first Delivered across process restart without
reissuing a POST or PUT. Final SDK Settle acknowledgement loss requires exact
already-Delivered receipt replay before the retained media owner can be released.
Unmatched retained jobs remain unknown and block close.

The pre-publisher combined096/097/099/100 workspace passes556 independent tests
plus one proxy child (557 printed),88 suite summaries, zero failed or ignored.
Full workspace all-target warnings-denied Clippy and formatting pass. Publisher
source passes123 Matrix tests, with three later test-only recovery extensions
passing their exact selectors. Final publisher strict lifecycle passes8 actual
scenarios plus the full19-path boundary (9/9); native and Windows GNU Clippy pass.
The combined five-slice workspace passes564 independent tests (565 printed),
88 suite summaries and zero failures/ignored tests. The consuming SDK-close
follow-up passes four custody tests and strict lifecycle8/8 across its four actual
paths. Final full-workspace Clippy, Windows GNU hagency Clippy and formatting pass.
459 Rust and543 Node bindings resolve with none missing. These follow-up checks
remain separate from the earlier full564-independent-test run.

Latest original hosted CI is1da8f1b. Native34563968212 passes Linux/macOS and fails
six Windows approval tests: four SDK-open acknowledgement timeouts, one SDK-close
timeout and one repository-close timeout after drop began. The cause is unproven.
ADR099 now observes exact connection/ownership-file destruction separately without
changing deadlines, retries or original verdicts. Node34563968228 passes4291 tests
with one platform skip. Original logs are preserved before the next branch push.
Windows GNU compilation is not native Windows workflow evidence; unconfirmed
directory sync remains a no-upload refusal, not positive file qualification.

Knowledge governance still has the exact157 pre-migration Error records, with all
260 introduced migration errors removed and no baseline record added or removed.
The original ADR bodies, valid requirements and executable test links remain.
Native state ownership is ADR095; legacy execution ADR028 is unchanged.

The next user-visible slice is the accepted ADR101 FileService application in a
new isolated worktree, based on the integrated prerequisites. Three disjoint
implementation owners handle the shared Bootstrap/file worker, HTTP/MCP plus
actual runtime tool enablement, and executable/TLS/recipient acceptance. It reuses
one fixed private profile, original Collector/writer/workspace and two retained
jobs. POST needs current source authority; historical GET is exact original-row
inspection only. No unimplemented ADR101 selector is integrated into main. Actual fresh SDK device
trust and Olm-session enrollment are a newly identified prerequisite; existing
publisher positives use verified fixture devices. ADR102 will define the real
first-use enrollment path without trusting arbitrary server-supplied keys or
importing fixture cryptographic authority. Positive executable file acceptance
remains required and unpassed until that path exists.

Keep canonical Done, cleanup, owner decision, runtime application, usage and
message acceptance separate. Complete room/history behavior, physical provisioning,
effective sandbox/config-home qualification, file tools, service/console parity,
quotas/retention, measured budgets and release/cutover remain open. No M0–M9 phase
or full migration is complete. The numbered notes below record earlier checkpoints;
this current section supersedes their interim status.

1. M0/M1 first checkpoint: native Salvo process, protected fresh state, custody,
   recovery, bounded work, shared protocol vectors and offline encrypted SDK proof.
2. Verify native CI on Windows, macOS and Linux, plus existing build-tool coverage.
3. Continue M0's complete dynamic endpoint/helper classification, supported runtime
   versions and measured device budgets. Run early process-tree/sandbox proofs.
4. M2 selected-resource domain checkpoint now implements verified-observation
   admission, one-transaction reservations/outbox and uncertain-effect recovery.
   Model qualification, derived catalogs and scoped cross-family checks now use
   the shared policy. Complete legacy/project-side/rotation integration.
   The M3 task/dispatch kernel now shares the domain database: current capabilities,
   atomic mutation receipts, frozen payloads, resource leases and conservative
   restart recovery. Canonical session resolution, per-session message projections
   and atomic dispatch input claims are now implemented in schema 4. Continue the
   internal group/MCP surfaces, task dependencies and durable final replies.
   Schema 5 now provides canonical task metadata, input activation through a
   fenced notice outbox, scoped delegation and start-time human follow-up. The scoped runner HTTP API now exposes task reads,
   comments, mutations and frozen inbox through the bounded writer, using its
   execution-time clock. The pure graph planner now matches the existing dependency policy using
   JavaScript-derived vectors. Schema 6 now provides explicit internal routes and atomic scoped conversation
   admission. Schema 7 now adds durable peer messages, exact-session recipients,
   dispatch-owned input and inspected recovery. Continue group lifecycle and atomic
   graph/task linkage before graph execution. Schema 8 now binds inspected-result
   reporting to the completed task epoch, separately from work creation authority.
   Actual final reply delivery remains to implement.
   Schema 9 now adds creator-scoped group member changes/closure, fresh rejoin
   sessions and durable host stop intents. Retired started work retains resource
   custody until inspected settlement; canonical tasks and input history remain.
   Schema 10 now binds finite task graphs to canonical node tasks, immutable
   assignment inputs and completed-epoch results. Current capabilities and
   inspected report grants remain distinct. Cancellation and scope retirement
   retain unknown leases and concurrency until host inspection. Continue final
   reply privacy/delivery, graph tool adapters and actual runner stop observation.
   Schema 11 now freezes explicit host-observed Matrix routes for fresh sessions
   and separates bounded final intent, send custody and observed delivery.
   Negative membership/privacy observations retire old sessions; cancelled or
   uncertain sends cannot silently resume. Continue real authenticated Matrix
   task-intent/session integration, arbitrary room/DM policy, taskless output and
   transport inspection before treating this as an operational reply bridge.
   Schema 12 now admits host-observed Matrix input into independent current
   session copies and creates/activates canonical task intents from that input.
   Direct main stays null-root; group/explicit threads retain authenticated roots.
   Offline repository and HTTP fixtures reach dispatch, follow-up and final intent
   without manually creating a task. Continue live notice send custody, actual
   authenticated Matrix intake/transport, automatic discussion-window selection,
   taskless output and generalized room policy before operational use.
   Schema 13 now persists private owner decisions and exact scoped grants,
   parks each native request and consumes it once before observed application.
   Shared approval-room negative evidence fences every Agent binding, and any
   unresolved request blocks resume. Continue native decision/inspection adapters,
   Matrix cards/verdict crypto and persisted operator YOLO policy before enabling
   runtime approval or treating this as full M6 parity.
   The initial hagency-platform proof now launches explicit native probes: Windows
   atomic Job Object assignment and POSIX unreaped-leader group cancellation.
   Continue native Windows CI validation, POSIX guardian/detached-child ownership,
   bounded runner IO and effective sandbox proofs before real runner adapters.
   Opaque child signal identities now use pidfds/process handles/macOS audit-token
   versions, with read-only birth metadata kept separate from signal authority.
   Extend those primitives to verified descendant adoption and guardian handoff.
   Native guardian handoff now uses an anonymous bounded prepare/start protocol;
   owner EOF, malformed input and leader exit cancel the owned process group.
   Continue detached-descendant adoption and guardian-loss recovery; a POSIX
   group report still does not establish full cleanup or sandbox enforcement.
   Linux now adopts orphaned descendants through a dedicated subreaper and
   validates pidfd waitability before signalling; only kernel ECHILD after root
   reaping can establish observed full cleanup. Actual Linux/Windows detached
   fixtures remain a CI gate for this step; macOS still refuses that guarantee.
   Schema 14 now fences verified task-notice sends before activation and keeps
   late/cancelled delivery distinct from current task authority. Native MCP and
   CLI task helpers use the same scoped API. The MCP helper now has 19 task,
   delegation, conversation, peer and graph tools; host-generated configuration,
   discovery and file tools remain open. The opt-in Codex approval
   coordinator now consumes durable decisions before typed responses, but the
   pinned upstream has no application acknowledgement: resolved is not Applied.
   Windows owned IO passed actual Windows CI; macOS identity observation fixtures
   were strengthened after CI's short heartbeat sample failed. Linux protected
   cgroup recovery is integrated but still needs successful hosted qualification.
   Authenticated Matrix observation collection and formatting are integrated;
   event intake and actual sends remain open. No operational cutover is enabled.
5. Continue M3–M9 in the migration plan; keep production deployments independent
   until every cutover gate is met. A foundation build is not full migration parity.

6. M6 content-format proof now lives in `hagency-matrix-format` (ADR050):
   JavaScript oracle, original body/relations, allowlisted Markdown and bounded
   edits. It is not wired to Matrix sending. Continue host event-size/chunking,
   private-route/crypto integration and actual client round-trip qualification
   before calling this complete M6 formatting or media parity.

7. M6 progress policy/coalescing proof is now isolated in `hagency-progress`
   (ADR052): fixed redacted summaries, exact JS policy/CLI vectors and bounded
   host-run receipts. ADR056 now attaches redacted, exact-instance Codex tool
   evidence through `hagency-progress-runtime`, including native pipe cancellation
   tests. Current domain/owned-worker binding, persistent uncertain-attempt
   recovery, editable Matrix status and route/crypto/delivery qualification
   remain open.

8. M7 bounded transcript normalization is in `hagency-metering` (ADR055).
   Continue transcript discovery, authenticated provenance, persistent ledger,
   exact Agent/project attribution, quota enforcement and console integration.

9. Host-only owned dispatch (ADR053) now binds frozen canonical scope and leases
   to actual native Codex pipes. Started commits before spawn, lost start receipts
   never launch work, and cancellation retains cleanup ownership. Runtime output
   never asserts canonical Done; incomplete cleanup retains dirty leases.
   Continue physical workspace and sandbox qualification, native helper setup,
   approval application, Matrix delivery and integrated platform acceptance.


ADR175 follow-up (2026-09-17): the four passing root3 DM tasks do not qualify
multi-agent files. The next file test retains a protocol failure on agent2 and
an uncertain room publication on agent1 after accepted Robrix approval/upload.
Both original tasks remain unknown, leases2, PID18059 retained; do not resend the
file or reconstruct an SDK owner. Per-agent bounded diagnostics now reach the
protected operator surface; fixed failure logs preserve future causes. Offline
library44/configured-local-fleet1 and strict Clippy pass. Root2 session006 passes
an actual new Robrix task. A separate authorized diagnostic fleet is in progress
on port19433; see the latest progress entry and ADR175 for evidence and limits.


ADR176 current checkpoint (2026-09-17): shared optional Matrix request pacing is
offline-qualified (TLS3, native45, configured-local-fleet1, strict Clippy/build).
The intermediate diagnostic fleet's coordinator hit429 during target2 provision;
its uncertain effect is preserved and original PID62001 closed0 with no tasks.
Completed root2 PID17433 also closed0 to reduce test traffic. Preserve unknown
owners PID20841/PID18059. The fresh paced fleet is PID83315/port19434 under
`local-native-palpo-paced-20260917T223343Z`, with one250ms shared request cadence.
Live qualification is in progress; see progress for exact evidence and limits.


Current live checkpoint after ADR177: root5 PID83315/port19434 has four passing
concurrent DM tasks on the selected pair1&3, plus working actual approve/deny
isolation. Agent3's approved file002 has independently verified34-byte plaintext,
but its final reply incorrectly claimed failure from a nonterminal unknown view.
Agent1's denied task created no upload. The project history reaches room creation
with no private qualification markers. Preserve these separate verdicts; this is
not a fully passing file workflow or shared-room task qualification. Agent2's
original pre-operation Worker failure and Queued input remain untouched, so
fleet readiness remains degraded. Root6 hit429 before any dispatch and closed0;
its uncertain provisioning effect remains preserved.

ADR177 adds bounded read-only polling guidance without changing receipt status,
authority, retry or completion semantics. Native library45/file-service7/MCP6,
strict Clippy, build and caller audit pass. Root2's prior session006 closed0;
explicit session007/transport7/DM-generation5 passed a fresh actual Robrix file
task with exact31-byte delivered content and correct final reply. Original
PID56393 then closed with observed exit0. Intermediate provider tool outputs
were not retained; the deterministic MCP test covers the unknown polling boundary.
Unknown owners PID20841/PID18059/PID83315 remain retained. Continue
Codex qualification before Claude, then Octos; see progress for exact evidence.


ADR178 factory project inbox (2026-09-18): resolve main-room sessions per original
transport/room generation from the factory collector's own authenticated Group
observation plus the matching available writer row. Refresh only room metadata;
retain provider/account/workspace bindings. Plaintext project execution requires
an explicit host-only Group selection and exact registered-project writer check.
Private DMs remain encrypted and each agent retains its original workspace.
Exact mentions pass the real two-agent configured fixture, followed by private
DM tasks with no project leakage. Matrix197, owned-claim12, native library45,
configured fleet5, and both corrected macOS cleanup fixtures pass; strict Clippy,
locked build and caller audit pass. Earlier newly enabled fleet runs intermittently
failed (peer EOF; one initial Domain refusal), still unresolved and retained in
progress. Synthetic method/receipt diagnostics now aid further reproduction.
Live root7 with pinned ADR178 binary and1s Matrix pacing is in progress; no live
project success or automatic thread discovery claim yet. Preserve original unknown
owners20841/18059/83315. Continue Codex, then Claude, then Octos.


ADR179 adds optional matrix_sdk_timeout_ms (existing10..60000ms bounds, default20s)
selected before startup. The same frozen Limits.sdk reaches approval/coordinator/
factory; running deadlines, HTTP limits and task budgets do not change. Root7
at1s pacing reproduced fresh approval enrollment cancellation at20s and exited1
with zero tasks; preserve its partial SDK. The synthetic configured regression
retains that negative case and proves a separate original60s enrollment/close.
Native46/configured fleet5/CLI8 and strict Clippy/build/caller audit pass; bootstrap
plaintext roundtrip was intermittent under concurrent load, then passed alone.
Fresh authorized root8 PID49126/port19437 is starting with60s SDK budget and1s
pacing. This is not a live qualification pass; see progress for original evidence.


Latest root8 live evidence: ADR179 approval/coordinator startup passed and both
factory targets became Active. Agent1 stopped pre-task with generic unknown;
agent2 admitted unaddressed project text with wake0, then selected its exact
project mention but lost warm-runtime authority before Started. No project reply,
no file/model replay, zero leases; PID49126 and original SDK/runtime state remain
retained. This is not a full project or two-agent qualification pass. Progress
records exact task/dispatch and private evidence. Investigate retained runtime
and guardian observations before another live attempt; no historical cause is
asserted. Preserve older unknown owners20841/18059/83315 too.
