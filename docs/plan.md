# Native migration working plan

This is coordination, not canonical runtime task state. There is no provisioned
`task-writer` in this source checkout. User instruction: execute the Rust migration
in a clean worktree. Branch `feat/rust-migration`; baseline `5dbef22`.

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
