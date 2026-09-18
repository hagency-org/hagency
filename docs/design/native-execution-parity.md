# TS → Rust execution parity working map

Operator direction, 2026-09-16: **make it work, make it right, make it fast**.
Trace working TS/JS behavior, port its missing Rust equivalent against existing
tests, run the same complete Palpo/Robrix path, and fix observed differences.
Adapter-only successes support a path; they do not substitute for it. This file
is coordination, not canonical task state or a completed migration claim.

## Target path

Local Codex, Claude Code and OctosCode with the real local Octos backend; mini3
supplies Palpo at `https://crew.ominix.io:19443/`. Use the existing real Robrix2
profile and preserve services/SDK stores. No remote model substitution, credential
copying or mock backend counts.

`Robrix message → Palpo → authenticated intake → canonical task/claim → local
runner → scoped tools/owner approval when needed → canonical completion →
original reply route → encrypted Palpo send → Robrix observation`.

Repeat the shared-room/separate-DM, files, approval, usage and recovery scenarios
in [the qualification runbook](two-agent-qualification-runbook.md), then soak.
Compare task state, actual runner activity and what Robrix receives separately.
Keep failed attempts; a narrower diagnostic cannot replace their acceptance.

## Source-inspected path map

Paths are relative to the repository root. TS includes its JS production callers.
Source presence is not a claim that the complete path passes.

| TS/JS reference and existing tests | Rust equivalent | Specific gap / next acceptance |
| --- | --- | --- |
| `bridge-matrix.js`, `backend-v2.js::routeMatrixMessageToThreadSession`; `tests/bridge-representative-intake.test.js`, `tests/bridge-group-isolation.test.js` | `hagency-matrix::{sdk,intake,event_batch}`, store `verified_ingress` | Intake exists. Re-run actual Robrix DM and mention-gated group requests with local runners; old remote soaks are historical only. |
| `backend-v2.js::enqueueThreadSessionDispatch/pumpRouterDispatches`, `router/src/store.ts`; `tests/router-runner.test.js` | `hagency/src/bootstrap/driver.rs::run`, store `owned_dispatch` | Claim/Started exist. Continuous/factory dispatch must select the actual local family/account, not merely exercise a fixture claim. |
| `backend-v2.js::launchClaimedThreadSessionRunner`, `router/src/runner.ts::runnerEnv` | execution `host.rs`, `local_codex.rs`, bootstrap profiles/accounts | ADR159 joins existing local Codex directories to one unmanaged preset/seat. Two real Robrix → Palpo → local Codex → canonical Done → encrypted Robrix reply paths pass, including actual workspace command/readback. Host still refuses Claude; Octos is not a native family. |
| `backend-v2.js::claudeThreadSessionArgs`, `lib/claude-thread-runtime.js`; `tests/claude-thread-runtime.test.js` | runtime `claude::{arguments,task_arguments}` | Ported model grammar, auto with write lease / plan without it, and gh/git-push ask rules. Native isolated settings do not prove all TS configuration/customization parity. |
| `router/src/runner.ts::runCodexDispatch`; `tests/router-runner.test.js`, `tests/router-codex-mcp-approval.test.js` | runtime `codex`, execution `Operation` | Real local 0.154.0 command/readback and message completion pass through Palpo/Robrix. File delivery exposed missing MCP elicitation handling; ADR160 ports exact active-item/arguments correlation and Once/Deny to the existing owner coordinator. ADR166 fixes reused approval sync cursors; Robrix accepts exact native 40-hex approval IDs. ADR167 admits Palpo nullable blurhash upload metadata. A fresh isolated live file task now passes approval, encrypted delivery, canonical Done and final Robrix reply. Previous unknown attempts remain preserved. Outside-workspace qualification remains unproven. |
| `router/src/runner.ts::runClaudeDispatch`; `tests/router-runner.test.js` | runtime `OwnedClaudeSession`, `claude::SessionDriver` | Actual CLI initialize/task-helper binding work. Execution Prepared/spawn/drive/report and approval coordination remain Codex-specific. Join the existing path with original domain ownership. |
| `lib/runtime/acp.js`, `lib/frameworks/octos.json`; `tests/acp-runtime.test.js`, `tests/acp-permission-requests.test.js` | No native Octos adapter yet | Port initialize/session/new/session/prompt and streamed updates. Verify installed OctosCode protocol mode, actual Octos backend and config-MCP behavior; do not infer them from mocks or a newer source checkout. |
| `router/src/runner.ts::buildPrompt`, `mcp-server.js`, `lib/mcp-server-core.js`; `tests/ephemeral-session-tools.test.js` | runtime `task_mcp`, native `mcp/task_client/runner` | Actual helper reads/heartbeats the assigned task. The four-tool owned profile is NOT full TS coordination parity: task comments and paged room discussion still need mapping/porting. |
| `backend-v2.js::requestThreadSessionOwnerApproval`, `lib/runtime-approval-client.js`; `tests/bridge-matrix-approval.test.js` | execution `approval`, Matrix `approval_delivery/approval_intake` | Codex MCP file approvals now join the same coordinator (ADR160), without widening the four task-tool exceptions. Startup owner wait is explicitly configurable within the original operation budget. Actual private Robrix Approve once now reaches an authenticated verdict and write-accepted native response; the subsequent send_file succeeds. The pinned provider still supplies no separate application acknowledgement. Claude has one-shot wire replies but no joined durable owner-grant path. |
| `lib/session-file.js`, `router/src/files.ts`, `lib/matrix-file.js`; `tests/matrix-file-bridge.test.js` | native runner `files/received`, Matrix `upload/publication.rs`, file/media-store crates | Local Codex send_file now delivers a 34-byte encrypted attachment and final reply rendered in actual Robrix. Independent authenticated media download verifies exact plaintext bytes. Robrix native Save dialog blocks the headless harness, so its save-to-disk workflow remains unqualified. Encrypted native receive-file now passes exact bytes, owner approval, Done and rendered reply; native attachment-picker UI and other runner families remain open. |
| TS runner result/settleAndRelease, task transitions and final replies | execution `operation.rs`, store `owned_dispatch/replies`, bootstrap `finish_attempt` | Two local successes meet distinct Done/stop/delivery checks. File failure retains an unsettled `owned_runner_failure` stop row despite observed whole-tree stop; generic orphan recovery correctly refuses it. ADR164 now joins an existing ADR162 receipt to explicit continuation through the shared stop-settlement kernel. This cannot retrofit the missing proof in the historical failure. Do not clear custody manually or erase the failure. Historical gen11 remains failed. |
| `router/src/store.ts::{beginOutcomeInspection,resolveOutcomeUnknown}`, Dashboard/AgentOps routes; `tests/router-core.test.js`, `tests/router-backend.test.js` | store `recover_dispatch`, conversation stop settlement, ADR162 receipt | TS explicitly supports operator inspection followed by continue-with-new-instruction, accept-completed or keep-blocked. ADR148's earlier claim that TS only recovers orphan homes is superseded. ADR164 adds lifecycle-scoped receipt inspection and explicit continuation for original receipt-bearing stopped failures, with a distinct instruction/dispatch and atomic input/custody transfer. ADR165 adds expiring inspection credentials and atomic continue/accept_completed/keep_blocked decisions. ADR170 now exposes original stopped-task discovery, inspection and all three decisions in the lifecycle console. Terminal graph actions remain open. The historical failure has no receipt and remains refused. |
| `router/src/store.ts::claimDispatchObserved`, backend live-runner cleanup | store `execution.rs::claim_clock` | ADR163 excludes only exact proven-stopped attempts from physical occupancy, retaining all task/workspace custody and claim gates. Missing proof still counts. Offline real-process and store regressions pass; the old live failure has no receipt and remains blocking. |
| `router/src/runner-guardian.ts`, `router/src/owned-process-tree.ts`; `tests/runner-guardian-process-tree.test.js` | platform `supervisor/unix.rs`, `supervisor/unix/macos{.rs,/native.rs,/tracking.rs}` | Ported native suspended launch, retained ancestry and identity-checked stop. Actual detached/reparented/early-leader cases stop while a foreign process survives; observation loss stays unknown. Sampled ancestry is not adversarial/crash containment; existing unsupported fast-double-fork cases remain unsupported. |
| `router/src/runner-activity.ts`, `lib/metering`; `tests/router-activity.test.js`, `tests/runner-metering.test.js` | progress-runtime, metering, execution `usage` | Claude usage capture exists but production Host cannot reach it. Match actual dispatch activity and durable attribution, not only normalization fixtures. |
| TS final reply routing / Matrix delivery journal; `tests/bridge-appservice-send.test.js`, `tests/matrix-delivery-journal.test.js` | bootstrap `finish_attempt`, Matrix `outgoing::send_final` | Send path exists. Confirm correct room/thread/privacy in Robrix and no duplicate delivery after interruption. |
| `router/src/runner.ts` executionTimeoutMs / approvalTimeoutMs | execution `Limits`, bootstrap claim, store owned approval context | ADR161 allows explicit execution up to the retained twenty-minute allowance and owner waits within the durable ten-minute ceiling. Capability lifetime covers configuration; leases still renew for only five seconds and cannot outlive capability expiry. Existing configured defaults and RPC/write bounds stay unchanged. Live long-task acceptance remains open. |

## Work order

1. Finish the existing **Codex** local message-to-reply path. Native macOS
   descendant stop and real local command execution now pass their focused
   checks. Explicit local provider selection is now wired through the native
   host and startup. Basic text and workspace execution pass live. Finish native
   stopped-failure recovery and restart route-generation
   handoff, then validate the new MCP approval adapter and encrypted file delivery
   in the same setup. Complete Codex groups/DMs, recovery and sustained soaking
   before beginning Claude, as explicitly directed by the operator.
2. Join **Claude** to the same Host/driver/completion/approval path, reusing its
   implemented session, helper and usage capture. Run the identical real flow.
3. Last, port **Octos/OctosCode** from the working ACP/backend implementation and actual
   installed protocol; run the identical flow using the real local backend.
4. Run cross-runner groups/DMs, files, approvals, restart and sustained soaking.
   Then measure latency/memory/throughput and optimize observed bottlenecks.

Safety and recovery preserve each path's existing contract and identify concrete
differences. They are not a separate substitute program. Broader parity/release
requirements in [the migration plan](hagency-rust-migration-plan.md) remain open.

## Evidence checkpoint, 2026-09-16

- TS router build succeeds; existing Claude-configuration, runner-dispatch and
  guardian suites pass **47 tests** locally. These are fixtures, not providers.
- Rust runtime after launch-policy parity fixes passes **92 tests**. Native
  MCP/task-client integration passes **21**, including actual helper/loopback
  read/heartbeat/readback and delayed retained-context handoff.
- Installed local Claude Code **2.1.270** accepts native task MCP binding and its
  exact catalog. No prompt, tool call or API connection was sent. The synthetic
  discovery context grants no account/task authority. Leader exit was observed;
  whole-tree stop remained false. This is not a completed vertical path.
- Current platform/runtime suite passes **122 tests**; downstream execution,
  CLI-pin and launch suites pass **78** after ADR159. These are offline tests. Real local
  Codex **0.154.0** separately completed one matching in-workspace command and
  file readback with whole-tree cleanup. Outside and symlink-outside cases made
  no tool attempt: no file creation is not sandbox-denial evidence, and the
  qualification exits **1**. The older pinned qualification gates remain red.
- A separate local qualification fleet uses three Matrix accounts and four
  rooms on the same Palpo, with fresh SDK stores and the existing exact owner
  master-key anchor. The original remote service/stores were not reused.
- Actual Robrix UI sent two local tasks: exact reply, then a workspace shell
  write/readback. Both reached canonical Done and one delivered encrypted reply,
  rendered in actual Robrix captures. Both released their leases before the next
  test. Measured claim-to-delivery was **14,114 ms** and **22,553 ms**; these two
  samples are a baseline, not throughput or soak qualification.
- File test3 failed before any file admission or approval. Original report:
  protocol/malformed, whole_tree_stopped, negative settlement, canonical
  InProgress. No retry/file was sent. ADR160 ports the missing TS MCP path;
  TS reference12, focused Rust MCP4, and owned approval1 pass. Full runtime95
  passes serially. Execution80 non-qualification checks pass; its existing
  pinned sandbox qualification remains1 pass/2 failures. One earlier parallel
  runtime run had an uncertain process-start failure; isolated and serial reruns
  passed without diagnosing its cause. Preserve that evidence, not a flaky label.
- Native restart first refused fenced generation1. Explicit generation2
  re-observation restored approval startup, but ordinary intake refused Domain
  and the session route remained generation1. Generic operator recovery returned
  409 because the original has an unsettled stop record. No raw DB edit,
  fabricated cleanup grant, replacement SDK store or fresh-task bypass was used.
  The diagnostic service is available but its agent driver is **unavailable**.
  Repair these actual joins before claiming restart/file/approval acceptance.

- ADR162 now preserves original stopped-owner content inspection for future
  failures (store352/execution83/strict Clippy pass). It grants no automatic
  resume and cannot backfill the old failure's missing proof. Authenticated
  generation3 re-adoption restores intake on the same account/device/room and
  SDK stores; new task004 remains queued because the old unresolved attempt
  consumes the ordinary single execution slot. No file/approval acceptance or
  soak passed. Keep that capacity refusal separate from the stale-route gap.
- ADR163's occupancy distinction passes execution84 and store45; strict Clippy
  passes. The rebuilt current TS router-core/backend reference passes88. The old
  failure is preserved and the live service is not restarted for this change,
  because it cannot supply the missing original receipt. Explicit TS-compatible
  operator resolution and the live file/approval path remain open. The subsequent
  ADR164 continuation slice below applies only where original proof exists.
- The existing native orphan recovery route now binds its addressed agent to
  the original dispatch inside the recovery transaction. A wrong-target HTTP
  regression first returned200; it now returns404 without changing custody.
  This does not widen the route to stopped attempts or resolve legacy proof loss.

### Local Codex startup selection

Private `agent-driver.json` / `development-driver.json` may explicitly select:

```json
"local_codex": {
  "profile": "provider_owned_codex_v1",
  "preset": "<existing-unmanaged-preset>",
  "seat": "<that-preset-seat>",
  "home": "<existing-absolute-provider-home>",
  "codex_home": "<existing-absolute-codex-directory>"
}
```

This is not a credential import or readiness record. Leave `managed_account`
and `factory_service` absent. The configured pinned executable still runs through
the original guardian and task helper. Selection and admission require the exact
resource; replacement of either retained directory stops the original operation.
Unix provider-directory policy is qualified offline; Windows is not enabled.
Create a separate local qualification Matrix device/state instead of sharing a
running remote service's SDK store/device or letting both consume the same inbox.
- `approval_owner_wait_ms` optionally replaces the prior fixed1000 ms wait.
  It and the response reserve must fit the original operation budget. The local
  qualification originally chose10000 ms within30000 ms. ADR161 now supports
  explicit longer operations; generation3 uses120000 ms within300000 ms.
  The new queued task has not yet exercised that configuration with a model.
- Original evidence is retained in the private qualification cache. No fresh
  local three-runner Palpo/Robrix soak has passed. Keep gen10 success and gen11
  failure with their original remote-run scope.

## Receipt-bound Codex continuation, 2026-09-17

The native console now exposes lifecycle-scoped inspection at
`GET /console/api/agents/{agent}/stopped-dispatches/{dispatch}/inspection`
and explicit continuation at
`POST /console/api/agents/{agent}/continue-stopped-dispatch`. The body contains
`original`, `fence`, `inspectionDigest`, `replacement` (the existing DispatchInput
shape with a distinct nonempty `payload.instruction`), and `evidence` (the
operator's review note). The replacement preserves task/session/resources.

Only a matching original ADR162 host receipt and owned_runner_failure stop can
enter this transaction. Inspection is private; both routes enforce lifecycle
scope and the addressed agent. Exact replay, including after reopen, is a read
of the original commit; changed content conflicts. Inputs move without being
marked processed and the failed dispatch remains outcome_unknown. Active or
unknown conflicting owners, stale routes and unsettled file receives, uploads or
delivery refuse.

This is an offline-qualified continuation slice, not live recovery, full TS
resolution parity or a completed Codex E2E gate. The old live file failure has
no original receipt and remains blocked; services and accounts were untouched.
ADR165 below extends this with the TS inspection-token workflow.
See ADR164 and task-rust-stopped-dispatch-continuation for the compatibility API boundaries.


## Operator outcome resolution, 2026-09-17

Schema35 adds lifecycle-scoped inspection credentials and durable operator
resolution receipts. The API is available for user testing without deploying or
restarting the current live fleet:

1. `POST /console/api/agents/{agent}/stopped-dispatches/{dispatch}/inspect`
   with `{}` or `{"ttlMs":60000}`. Lifetime is 1..60 minutes, default15 minutes.
   The private response includes `inspectionId`, one-use `inspectionToken`,
   `expiresAt`, and the original receipt/task/route snapshot. Only the token hash
   persists. The historical GET inspection route still does not mint authority.
2. Review the historical inventory against current workspace and external
   effects. The API does not infer semantic safety or scan a new physical owner.
3. `POST /console/api/agents/{agent}/resolve-stopped-dispatch` with:

```json
{
  "original": "original-dispatch-id",
  "requestId": "unique-operator-decision-id",
  "inspectionId": "inspection-id-from-step-1",
  "inspectionToken": "private-token-from-step-1",
  "action": "accept_completed",
  "operatorNote": "Reviewed the retained workspace and external effects"
}
```

Actions are `continue`, `accept_completed`, and `keep_blocked`. Continue also
requires `replacement` in the existing DispatchInput shape, preserving task,
session and resources with a distinct nonempty `payload.instruction`. It enqueues
one new attempt and transfers frozen inputs atomically. Acceptance marks the
canonical task Done once; blocking marks it Blocked and starts no process. Neither
creates a graph result, accepted runner output, processed-input receipt, or final
message delivery. Original unknown attempt evidence survives all three decisions.
Terminal graph actions refuse until their separate graph-resolution contract is
ported; graph continuation retains existing recovery checks.

Retry an uncertain HTTP response with exactly the same body/requestId. Its durable
receipt survives token expiry and restart; changed content conflicts. New requests
with expired or consumed credentials, changed scope/task, missing original proof,
conflicting owners or unsettled media effects refuse without mutation. Time is
read after acquiring the writer transaction. Issuance is bounded to16 retained
inspections per dispatch and40000 total, reclaiming expired unused credentials.

The old live file failure still lacks its original owner receipt and cannot use
this workflow. Independent file/approval qualification needs the separately
requested fleet/state. Current services and the failed attempt remain intact.


## Isolated Codex file and restart qualification (2026-09-17)

The operator authorized an isolated test fleet. The first new instance uses
`local-native-palpo-approved-20260917T180452Z` under the private qualification
cache; three accounts/four rooms were created without modifying earlier fleets.
Text001 completed with the exact encrypted Robrix reply. File001 exposed two
protocol mismatches: approval receipt identity treated a Matrix next_batch cursor
as a unique body, and Robrix accepted only legacy 32-hex approval IDs while native
IDs have 40 hex digits. ADR166 now keys receipts by exact (cursor, digest), including
reopen validation. Robrix's pure parser accepts exactly both lowercase profiles
and preserves all verdict bindings; no new UI labels/i18n keys were needed.

File002 then reached actual Robrix Approve once, authenticated encrypted verdict,
and native write-accepted response. It failed during upload and retains unknown
custody. Read-only decryption of protected SQLite values finds WritePossible and
no accepted response. The original failure reason was not retained, so its exact
HTTP response cannot be reconstructed. Graceful shutdown intentionally retained
its original owner, PID 20841, instead of discarding uncertainty. A proposed third
session was refused at custody_open; no third task ran there. Its unapplied config
is retained separately and the actual session002 config restored. Do not kill or
reuse that original SDK owner to bypass the failure.

One separately recorded 34-byte opaque diagnostic upload returned HTTP 200 with
content_uri and blurhash:null. Local Palpo source serializes that Option<String>.
The native parser rejected this reproducible response, including its protected
reopen validator. ADR167 shares strict upload parsing across both boundaries and
admits only optional null/string blurhash beside content_uri. Duplicate keys,
wrong types, unknown fields, body/header/EOF bounds and no-retry rules remain.
File-service diagnostics now retain the Matrix error variant before the existing
one historical inspection. No previous response or receipt was synthesized.

A second independent test instance, `local-native-palpo-upload-fixed-20260917T184629Z`,
uses three fresh accounts/four fresh rooms and the original retained Robrix owner
profile/trust anchor. File003 passed the complete actual path: local Codex shell
write/readback, send_file with matching owner card, actual Approve once click,
authenticated verdict, accepted media upload, delivered encrypted file event,
canonical Done, whole-tree completion/lease release, and final encrypted reply
`ISO_CODEX_20260917_FILE_003_OK` rendered in Robrix. The attachment is 34 bytes.
An authenticated owner media GET, checked against the protected publication
ciphertext digest and decrypted with its exact descriptor, matches original SHA256
`82440d1dd5c9d5fefb79afde494688ad47cb99bf7fa104d42bd0afe8b12c025f`.
This independent byte check is not a Robrix Save-dialog pass: that native dialog
blocks the headless stdin loop without an accessible panel. The test-owned UI was
restarted with its original profile; no SDK/profile reset was performed.

The successful service closed cleanly with exit 0. Fresh authenticated provisioning
advanced transport/approval generations to 2 and created local_codex_dm_002 with
its own workspace. The same SDK stores reopened successfully. Restart001 wrote
and read the new workspace marker, reached canonical Done, released all leases,
and rendered `ISO_CODEX_20260917_RESTART_OK` in Robrix. The earlier file remains
one delivered operation; no upload/publication was repeated. This is explicit
fresh-session restart qualification, not seamless resume of the retired route.
The successful test service remains on 127.0.0.1:19431; Robrix automation on 19401.
The earlier retained unknown owner remains preserved separately.

Evidence stays private: per-effect intent/result files, both failure reports,
protected read-only snapshots, approval/delivery/restart captures, binary hashes,
`file-003-completion-evidence.json`, `file-003-media-byte-evidence.json` and
`restart-001-evidence.json`. Read-only before/after comparison confirms unchanged
original legacy dispatch/stop/lease/inspection rows: completed 2, unknown 1, queued 1.
No provider credentials were copied, no existing remote service changed, and no
formatter, commit or PR was run. Automated setup retried only explicit 429 refusal
after its cooldown; uncertain upload effects were never retried.

Validation: the new custody test first failed with Transport(Wire) on the Palpo
fixture; after the fix full Matrix lib 190 and media-upload 7 pass. Earlier ADR166
approval intake 19/full lib 189 runs passed before the additional upload test.
Robrix octos_actions 38 pass, including native wire fixture and property grammar;
the property failed before the parser fix. Strict all-target Clippy for
hagency-matrix/hagency and locked native build pass; actual headless Robrix build
passes. agent-spec parse/lint were attempted but the executable is absent, so no
agent-spec lifecycle pass is claimed. Existing broader qualification failures
are not overwritten by this narrower result.

Remaining: outside-workspace sandbox qualification, group/DM
isolation with multiple agents, original-owner recovery after uncertain media,
seamless restart policy, sustained soak, and the native Save-dialog workflow.
Codex remains first; Claude and Octos joins are not yet qualified. The Rust
migration is still incomplete and production cutover remains unauthorized.

## Receive-file, repeated-run failure and recovery join (2026-09-17)

Receive001 passes on the second isolated fleet. A private Rust probe used the
same pinned Matrix SDK and exclusively reopened the original owner profile while
the test Robrix process was closed. It sent one encrypted 37-byte attachment;
the original profile was then reopened in Robrix. The actual receive_file owner
card rendered there and Approve once produced an authenticated verdict. Native
receive `receive_30a737107e3eb97758b0bbadabed05f4` reached Ready in workspace002,
canonical task Done, completed dispatch and zero leases. Independent byte checking
matches SHA256 `2a1ee0f2d3e6b342ffa33e79e6e324e10b1881547dd4a68b7ca2087a8f43b2fc`;
the final `ISO_CODEX_20260917_RECEIVE_OK` rendered in Robrix. Evidence is
`receive-001-evidence.json` plus actual approval/result captures. This qualifies
the native encrypted receive path and Robrix approval/result, not the headless
native attachment-picker dialog. No owner credentials or SDK state were cloned.

The first pulse of a separately recorded 12-pulse owner-DM soak failed with
`unsupported_event`, before creating its workspace marker. The original host
recorded whole-tree stop and the original inventory receipt; the failed dispatch
`matrix_dispatch_f9789ea4e56d351531f44e7437250799` remains outcome_unknown. The
soak stopped immediately and sent no second pulse. Its exact rejected category
was absent from the original report. ADR168 now carries the runtime's existing
fixed refusal category through the authenticated native diagnostics; it cannot
reconstruct the historical lost category or turn this failure into success.

An authenticated lifecycle inspection and exact current file-hash comparison
then authorized a distinct continuation using the ADR165 API. The transaction
settled only this original stop and queued
`iso_codex_20260917_recovery_dispatch_001`. The old live worker had already left
its dispatch loop, so no replacement process started. This is an observed live
recovery integration failure, not a successful recovery. ADR169 retains the
continuous worker/report until exact explicit resolution is observed, then
releases that workspace entry and resumes ordinary claiming. It does not patch
the already-running old binary or grant restart recovery over retired routes.
The original unknown upload owner PID20841 remains separate and untouched.

A separate sandbox qualification rerun still passes actual inside-workspace
command/write/readback and whole-tree cleanup. Outside and symlink-outside
turns emitted no command attempt, so both remain unqualified. Original and new
evidence/logs are preserved separately (`sandbox-before-20260917.json` and
`sandbox-after-20260917.*`). No retry or broader permission was granted.


## Continuous recovery and console workflow (2026-09-17)

ADR169 passes the real-process offline continuous-driver scenario: the original
stopped receipt is reviewed through authenticated HTTP, explicit continuation
commits, and the same worker claims and completes the new instruction. The store
observation binds the original capability, fence, stop receipt and consumed
inspection/resolution; it grants no authority by itself. Full owned-completion
16, bootstrap20, bootstrap library18, strict all-target Clippy and locked build
pass. The production-caller check also passes. The historical live queued
continuation was subsequently superseded by explicit route-generation3/session003
provisioning; it never executed and is not successful recovery evidence.

The isolated service then ran ADR168/169 with the original SDK stores. A new
12-task owner-DM run completed over 1,018.689 seconds at 90-second cadence. All
12 canonical tasks are Done, their dispatches completed, exact workspace bytes
match, one final encrypted reply each is delivered, and leases return to zero.
The original per-pulse UI substring check also matched owner prompt text; its
UI claim was insufficient. A separate bounded timeline audit subsequently found
all12 exact whole-message agent replies inside visible Robrix bounds, excluding
prompt substrings, and retained seven snapshots/captures. Private evidence is
`soak-002/completion-evidence.json`, `observations.jsonl` and `rendered-audit.json`.
This is a bounded single-agent run, not multi-agent or sustained qualification.
The earlier unsupported-event failure remains unexplained and preserved.

ADR170 adds a lifecycle-scoped sixteen-row stopped-dispatch list without changing
the roster projection. The native console displays the original task, inventory
hashes and inspection expiry, requires an operator note, and offers continue with
a distinct instruction, accept-completed and keep-blocked. One-use credentials
stay in memory. A lost response retains the exact pending request for explicit
retry; no automatic mutation retry occurs. English and Chinese labels are included.

Validation: new projection test passes; five focused UI tests pass; the actual
static Next build and real Chromium lifecycle walk pass, including commit then
lost-response then exact explicit replay. Full console60 passes serially and
strict all-target Clippy with the browser feature passes. An earlier parallel
console run had59 pass/1 real-agent startup failure (outcome_unknown before native
entry); the isolated selector passed. Cause remains unproven; preserve both logs.
No production origin check was weakened: the browser fixture explicitly supplies
same-origin Fetch Metadata to Playwright's intercepted-request HTTP client.
The rebuilt service/UI is being qualified live on the isolated fleet; source and
offline success alone do not close the live recovery gate.


### Live browser continuation passed

On generation4/session004, a controlled long-command task wrote its before-file
then failed unsupported_event before the operation deadline. Original host cleanup
proved whole-tree stop and preserved a one-file inventory. The actual native
Chromium UI listed and inspected this dispatch, displayed the original hash,
accepted an explicit note/new instruction, and rendered the committed receipt.
The same service PID93924 claimed `continuation_09bdfede-eb25-4650-ac03-3270c0a4fb5f`
without restart. Local Codex wrote/read exact recovery bytes, completed the same
canonical task, released all leases and delivered `ISO_CODEX_20260917_RECOVERY2_OK`
as an encrypted final reply, independently observed as an exact visible Robrix
message. The original dispatch remains outcome_unknown. Evidence is
`recovery-002-{inspection-evidence,resolution.result,completion-evidence}.json`
and actual console/Robrix captures. This passes explicit stopped-task continuation;
it does not qualify the intended timeout, uncertain media recovery or old routes.

A separate private probe of the pinned binary identified terminal interaction as
one missing progress notification; the original report's unknown category remains
unchanged. ADR171 correlates it to the current active command and treats it only
as bounded progress. Live regression qualification follows the offline red/green
case; the first historical soak failure still has no recoverable exact category.


### ADR171 live command polling regression passed

The opt-in original-owner probe refused terminal_interaction before the change
(13.272s, whole-tree stopped). With ADR171 it completed the same20-second command
(25.938s, no refused notification, whole-tree stopped). These private diagnostic
runs are separate from live acceptance and never enter ordinary test execution.

After clean close and explicit generation5/session005 provisioning, the isolated
fleet ran the new binary and final console assets against the original SDK stores.
Actual Robrix sent a20-second shell command with repeated empty-stdin polling.
Dispatch `matrix_dispatch_becee36542d5d22164f888fd8f73a879` completed the canonical
task, preserved exact35-byte before/34-byte after files, released every lease and
delivered `ISO_CODEX_20260917_TERMINAL_OK`, visibly rendered in Robrix. End-to-end
observation took58.2s; this is a single regression sample, not a throughput claim.
Evidence is `terminal-001-evidence.json`, its intent and actual Robrix capture.
A first harness preflight refused the driver's ordinary refreshing state before
sending any message; it was corrected to accept refreshing/no_work with no error.

Final ADR171 validation: runtime96 passed serially (session43 included), strict
all-target runtime/native Clippy including browser features and locked native
build pass. Final static console build includes the existing padded table style
for readable inventory columns. Production-caller check passes45 wired plus the
unchanged tracked G8; no missing/ambiguous/unresolved/unknown gaps. Whitespace
check passes. agent-spec/task-writer are absent; no lifecycle pass is claimed.

Current successful isolated service: PID15469, port19431, generation5,
local_codex_dm_005, binary `bin/hagency-adr171`, console assets
`native-console-adr170-3`. Original unknown media owner PID20841 stays retained.
Historical unknown dispatches, explicit resolution receipts, superseded earlier
continuation, accepted file delivery and ready receive are all preserved. No
formatter, commit, PR or production cutover was performed.

Next Codex gates are multi-agent shared-room/separate-DM isolation, sustained
qualification across those routes, positive outside-workspace sandbox-denial
witnesses, uncertain-media recovery/seamless restart and native file-dialog UI.
The successful explicit stopped-task continuation and long-command progress now
close their specific gaps. Claude's Host/approval join and Octos's native adapter
remain after Codex under the operator's requested order. The overall Rust port
is incomplete; no percentage or cross-runner success is inferred from these runs.


### Actual five-minute deadline and UI continuation passed

Timeout003 on the unchanged generation5/session005 service wrote its before-file
then ran a360-second command against the300000ms host operation budget. The
original dispatch `matrix_dispatch_539908d9247376eea355f0ba64d4aa75` stopped at
300043ms from claim to original stopped-inventory receipt, with owned_failure
`deadline`, cancellation recorded and whole-tree cleanup proved. The exact
before-file remains; the after-file does not exist. Its three-entry original
inventory also includes the earlier terminal regression files.

Actual Chromium displayed the stopped dispatch and hashes in English and Chinese,
with no horizontal document overflow. After an explicit note and new instruction,
the same PID15469 claimed `continuation_c8dd3eea-d4e8-499d-86ba-6732b49f1bb8`.
Local Codex wrote/read `timeout-003-proof.txt` with SHA256
`4c64602c7191bbe6ff0f2fbc9015792e3ab2bdf8cb74343a58409c57abf05ce6`, completed the
canonical task, released all leases and delivered the encrypted final reply
`ISO_CODEX_20260917_TIMEOUT3_OK`, independently matched as an exact visible Robrix
message. The original dispatch remains outcome_unknown; no restart was needed.

Private evidence under the successful isolated root is
`timeout-003-{stop-evidence,inspection-evidence,resolution.result,completion-evidence}.json`,
its original intent, actual English/Chinese console captures and the Robrix
`scene_timeout003_complete.png`. This closes the actual operation-deadline and
explicit UI continuation case. It grants no unknown-media settlement or seamless
restart; original PID20841 and all historical failures remain preserved.


### Current checkpoint: local factory source and rate-limit interruption

ADR172 optional project-inbox adoption passes authenticated TLS fixtures and live
root3 adoption. ADR173 provider-owned local Codex factory joins pass actual offline
two-agent/two-round task, private approval, encrypted file and final reply tests.
Each agent keeps its workspace/context; the original provider directories stay
shared only through the explicit retained LocalCodex selection. No managed login
is fabricated. All warm8 selectors pass across suite/focused runs. The two
historical configured-fleet regressions also pass when explicitly run on macOS.
Strict Clippy and library42/bootstrap20/CLI8 pass. The factory still exposes only
its DM to task intake; shared-project routing remains a separate implementation
gap and is not covered by those private-DM successes.

The new root3 `local-native-palpo-fleet-20260917T212730Z` created three accounts
and four rooms; its original registration429 was retried only after cooldown.
PID95426/port19432 then reached ready. Both it and root2 PID15469 hit Remote(429)
at21:33:15 UTC and stopped intake with transport/DM scopes unavailable. Their
exact failing endpoints are unknown. Root2 preserved18 completed/3 unknown/
1 superseded dispatches; root3 had zero dispatches, before target1 could admit.
Both services closed exit0, original SDK stores intact. Preserve these failures.

ADR174 adds bounded retries only for complete429 JSON GET responses, keeping one
original request deadline and all mutation/unknown-custody rules. Focused TLS3
pass; broader validation is underway. Recovery is staged with root2 transport6,
root3 transport2, new sessions/workspaces and fresh DM scope generation3. No
unavailable scope is revived at its old incarnation. PID20841 stays retained.
These stages are not live two-agent qualification or completed migration.


### ADR174 validation and live two-agent DM rounds passed

Final Matrix validation passes224 tests: library193 (including rate-limit3),
transport9, token-provision9, upload7 and download6. The serial library run took
733.80s; all original test handles completed. Strict all-target native/Matrix/
execution Clippy with browser features and locked native build pass. Initial
commands used nonexistent feature/test-target names; those command failures are
retained and not test failures or passes. No ordinary tests contacted live services.

Both isolated services were re-adopted under fresh transport/session authority,
with their original SDK stores and a restored DM generation3 after unavailable2.
They run the same retained `bin/hagency-adr174`, SHA256
`58e215add042a2b150221dd14df42413bbb604ba42471a0472c102af05a8282b`.
Root2 is PID17433, port19431, transport6/session006; root3 is PID18059, port19432,
transport2/session002. Both report ready. Previous failed instances and their
clean-close receipts remain. The original unknown-upload PID20841 stays retained.

Root3's real native factory created two agents in the same project, each with its
own encrypted DM, SDK owner, original local Codex process, home and workspace.
The retained target1 request was processed after restart without resending it.
The actual owner joined both new DMs, and actual Robrix submitted two tasks per
agent. Four dispatches completed with canonical Done, exact different bytes at
the same relative isolation-marker.txt path, encrypted delivered replies bound to
the correct sender/DM and zero leases. Round2 observed both original tasks Started
concurrently. The authenticated unencrypted project history reaches m.room.create
and contains none of the private markers; no group task was submitted.

The initial UI audit clicked sidebar coordinates but captured the other selected
room and failed. Preserve this harness failure; its two completed tasks were not
resent. Stable named tabs, whole-message equality and actual selected-room captures
then establish all four final replies. Snapshots also contain hidden-tab widgets,
so their coordinates alone are insufficient. The round1 resumed observation took
6.18s and round2 52.08s; the former is not original task latency and neither is a
throughput claim. Evidence is `two-dm-completion-evidence.json`, per-round receipts,
original/resumed scripts and logs, actual Robrix frames and
`project-private-marker-audit-complete.json`. The first short-page audit alone did
not prove history exhaustion; the final audit explicitly reaches room creation.

These results qualify the bounded two-agent private-DM task path. Factory
shared-room task intake, multi-agent file/approval interaction, sustained soak,
positive sandbox-denial witnesses, unknown-media recovery, seamless restart and
native picker UI remain. Codex remains first, then Claude, then Octos. No formatter,
commit, PR, production cutover or unavailable agent-spec lifecycle pass.


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


Native private configuration for a paced fresh fleet may select
`"matrix_request_interval_ms": 1000` and `"matrix_sdk_timeout_ms": 60000` before
starting the service. The first value is a shared per-process request cadence;
the second is each original SDK/enrollment job's fixed maximum duration. The
20s SDK default remains when omitted. Both must fit the actual startup and
operation flow; neither setting guarantees server quota or permits a running
job to extend its deadline. Do not apply the settings to revive incomplete
SDK custody. The read-only `provision existing` adoption command uses its own
bounded HTTP client; these driver fields do not configure that command.


Latest root8 live evidence: ADR179 approval/coordinator startup passed and both
factory targets became Active. Agent1 stopped pre-task with generic unknown;
agent2 admitted unaddressed project text with wake0, then selected its exact
project mention but lost warm-runtime authority before Started. No project reply,
no file/model replay, zero leases; PID49126 and original SDK/runtime state remain
retained. This is not a full project or two-agent qualification pass. Progress
records exact task/dispatch and private evidence. Investigate retained runtime
and guardian observations before another live attempt; no historical cause is
asserted. Preserve older unknown owners20841/18059/83315 too.
