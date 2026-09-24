# ADR failure-model consistency: what the records decide, where they contradict each other, and one model to replace the pieces

Scope: read-only review of every record in `knowledge/decisions/` (181 ADRs plus
the template) for the rule each states about a failure, an unknown outcome or a
discrepancy; every rule confirmed or refuted against the code that applies it,
and compared with the retained product where it has a rule. Follows
`docs/reviews/2026-09-22-native-codex-architecture-review.md` (gaps G1–G6 and
the approved closing order) and ADR-181 (Proposed, the evidence slice). Nothing
here is built; every claim is a `path:line`. Prefixes: `K` =
`knowledge/decisions`, `R` = `native/`, `T` = the retained repository root
(the TypeScript product). Code lines are the working tree as read on
2026-09-22/23: HEAD `a3c3801b` plus ~70 uncommitted files of the ADR-181 slice,
so the review's own citations of a day earlier have moved (e.g.
`Failure::LostAuthority` is now a struct variant, `R/hagency-execution/src/operation.rs:153-156`)
and these will move again when the slice lands.

## The thesis

The records contain two failure models, and no record says which one governs.

The **retained model** (ADR-011, ADR-019 for the TS product; adopted piecewise
by the operator's 2026-09-19..22 amendments in ADR-029, 046, 047, 064, 096, 147,
162, 178, 180) is: a fact the host cannot prove ends the *dispatch* as
`outcome_unknown`, quarantines the *session*, fences the *agent* only for an
unconfirmed process cleanup, is said in the thread, and the process always
exits. The **port model** (ADR-036:293-299, ADR-053, ADR-060, ADR-096, ADR-064,
ADR-174, ADR-175, ADR-180:131-134, and about a dozen rules applied in code that
no ADR decides) is: the same fact ends the agent's worker, marks the fleet
failed, makes `/ready` 503, and retains the process.

Each of the operator's six "follow TS" amendments reversed one join and left the
rules that depend on the reversed sentence standing in other records: ADR-047's
"a clean close retires nothing" reversed ADR-096's fence but not ADR-096's
retained-owner shutdown; ADR-029's "a process nobody can place is not owned"
reversed the tracker but not the host's 100 ms re-qualification that turns the
tracker's late reply into `LostAuthority`; ADR-046's "no approval is refused by
its shape" reversed the adapter but not the coordinator's `LostAuthority` on a
store refusal of the same request, and ADR-046 itself still keeps one
(`K/adr-046:489`); ADR-162's "keeps the agent up" (`K/adr-162:63-64`) applies
only when the tree is proven stopped and inspected (`K/adr-169:16-17`), which
excludes exactly the two live signatures. ADR-181, the slice being built now,
says of itself "This changes no verdict: every site still fails"
(`K/adr-181:83-84`): as Proposed it codifies the present inversion as the
evidence baseline, which is right for evidence and wrong as the last word.

Counts from Part 1: of the 181 records, all but four (ADR-001, 015, 017, 022)
state some rule about a failure or a discrepancy; about two dozen decide a
consequence at agent, fleet or process scope (table 1a: ADR-019, 036, 043,
047, 053, 064, 096, 117, 147, 153, 162, 169, 174, 175, 178, 180; table 1b:
ADR-061, 089, 098, 101, 104, 127, 133, 135); two of those name the retained
product's narrower rule and defer it (ADR-036:293-299, ADR-162:79-82). Nine
rules that decide an agent-, fleet- or process-scope consequence are applied
in code and decided in no record: the warm idle qualification cadence and its
`response_ms` bound (F3); lease renewal treating `Busy`/`Unavailable`/a 2 s
timeout as revocation (F11); a store refusal of an approval request as
`LostAuthority` (F4); one lost agent making the fleet `outcome_unknown` and
`/ready` 503 (F7); a dead pump failing every later claim (F8); a fresh
admission failure ending `serve` (F12); a final send that is not `Delivered`
ending the worker (F13). The 18 findings of Part 2 are the operator's four,
verified, plus fourteen more.

## Part 1. What each record decides about failure

Columns: the rule (quoted), the scope of the consequence it decides
(dispatch / session / agent / fleet / process / transport), who resolves it
(automatic / operator / restart / nobody / later evidence). "none" means the
record states no failure rule. Records whose subject is far from execution
(media, usage, console, provisioning wire, early TS-era product decisions) are
in table 1b; I read table 1a's records in full myself and table 1b's through
three read-only extraction passes whose quotes I spot-checked by `grep -F`
against the files (the sample is named under 1b).

### 1a. Execution, custody, transport, approval, recovery, restart (read in full)

| ADR | Status | Rule stated about failure / discrepancy (quoted) | Scope decided | Resolver |
|---|---|---|---|---|
| ADR-011 (TS) | Accepted | "After `started`, the dispatch is NEVER re-executed automatically, and any terminal state other than `completed` … settles as `outcome_unknown`" (`:195-199`); "Graceful shutdown stops new scheduling, terminates and awaits all guardians, then exits" (`:207-208`); restart: "every dispatch left in `started` … becomes `outcome_unknown` with the same operator notice" (`:212-214`); "adapter startup failure aborts the dispatch" (`:258`) | dispatch; process exits | operator (inspection); restart reconciles |
| ADR-019 (TS) | Accepted | "Unconfirmed cleanup retains a durable agent fence. The same owned close channel may later provide confirmation and clear that termination receipt" (`:114-116`); "started or parked work settles as `outcome_unknown`" (`:108-109`); "shutdown may remain unavailable after a timeout even when workspace inspection has completed" (`:158-159`) — this is the console Stop verb, not process exit | dispatch + agent (persisted fence) | later close-channel evidence; Start refused meanwhile |
| ADR-005 (TS) | Accepted | "A missing, timed-out, malformed, or failed adapter produces an explicit deny with a diagnostic; it never becomes an implicit allow" (`:57-59`); "cancelled requests get no later response" (`:104`) | request | automatic (deny) |
| ADR-012 (TS) | Accepted | "`outcome_unknown` has the three accepted resolutions `continue`, `accept_completed`, and `keep_blocked` … Continuing creates a new queued recovery dispatch and never executes the original started dispatch again." (`:149-152`) | dispatch | operator |
| ADR-021 / 026 (TS) | Accepted | "Stale or incomplete authority fails closed. Missing tools or update failures must be reported" (`021:45-46`); "terminal status derive from the router, not model claims" (`026:21-22`) | tool call | automatic |
| ADR-029 | Accepted | "A newcomer that ancestry, session, group and coalition evidence all fail to classify is recorded as not owned … it never ends an observation" (`:396-400`); "What stays fatal and sticky is loss of inspection itself" (`:406-408`); "The refusal was the port's own invention" (`:394`); positive receipt needs "two complete empty live-member censuses after leader exit" (`:180-181`) | dispatch (cleanup verdict) | nobody in-process (sticky) |
| ADR-030 | Accepted | closing a group: "Started, parked and previously unknown work receives a durable stop intent, loses its execution capability and remains outcome_unknown" (`:30-32`); "Uncertain retired work retains logical leases, workspace dirtiness and session quarantine until the host proves its process scope stopped and inspects effects. Pending stop intents survive restart and count against execution concurrency." (`:35-37`); "Settlement is a host-only transaction" (`:38`) | dispatch + session (store) | host proof, then operator |
| ADR-032 | Accepted | "Timeout, malformed input, EOF with partial data or pending requests, and transport failure make the connection permanently inert" (`:89-90`); "There is no reconnect, resume, resend or retry API" (`:93`) | connection | host reconciliation |
| ADR-034 | Accepted | "Every termination is unresolved with respect to execution … The future host adapter must fence the dispatch, observe/stop the guardian … and follow the existing outcome-unknown policy" (`:77-81`) | dispatch | host |
| ADR-036 | Accepted | "A failed turn still ends the attempt as a protocol failure with its owner retained, which ends the agent's worker. The retained product settles such a dispatch as outcome-unknown … quarantines that one session until an operator resolves it, and keeps the agent running for its other sessions. That parity belongs to the recovery work and is not decided here." (`:293-299`); "the existing `error` and failed `turn/completed` arms still end the turn as Failed" (`:264-265`) | agent (deferred) | nobody (deferred) |
| ADR-040 | Accepted | "IO errors remain `Cleanup::Unknown`, which a retained caller can inspect and retry" (`:109-110`); stop "bounded to three seconds per attempt … an earlier failed operation stop can also be retried on final drop" (`:115-119`); "Unknown observation is sticky and cannot rearm" (`:150`) | dispatch (cleanup) | retry promised (see F9) |
| ADR-043 | Accepted | "Current negative evidence retires availability and saved grants immediately … A third member … reported through Agent B also fences Agent A" (`:30-33`); "restoration needs a new room generation" (`:35-36`) | approval room (all agents sharing it) | operator (new generation) |
| ADR-046 | Accepted | policy refusal → decline (`:430-434`); expiry → decline (`:466-468`); "An unknown expiry denial sends nothing and ends the operation with LostAuthority" (`:488-489`); "A planned request that is durably decided now leaves the intake's plan … A request that still reads pending and is refused keeps ending the intake" (`:520-523`); "What still ends a session is a request the adapter cannot show faithfully: malformed, oversized, an unknown field, or an unknown method" (`:557-558`) | dispatch (turn continues) except the two named cases → agent | automatic |
| ADR-047 | Accepted | "conservatively fences the entire device incarnation after any incomplete authenticated collection" (`:92-93`); "A future live execution host must stop using an incarnation whenever negative persistence itself returns Busy, unavailable or OutcomeUnknown" (`:102-104`); amendment: "A clean close … write nothing to the domain" (`:245-246`), "The retained product fences nothing at shutdown" (`:243`), "A cancelled read is not negative evidence" (`:266`), "A fenced generation stays unavailable, and startup still must not rotate a generation by itself" (`:285-286`) | transport (that agent), rooms (all agents) | operator (generation rotation) |
| ADR-048 | Accepted | "Unknown writes, read errors and timeout keep cleanup unresolved. Explicit stop retains the capability for inspection/retry" (`:146-147`) | dispatch (cleanup) | retry |
| ADR-053 | Accepted | "A revoked, expired, replaced, dirty or quarantined scope stops the actual retained owner" (`:82`); "A pending writer receipt may take up to its two-second bound before authority becomes unknown" (`:106-107`); "failed operation, explicit stop, failure reconciliation and final drop can each retry" (`:113-114`); "An explicitly returned Report may retain an unresolved owner" (`:128-129`); "macOS's whole_tree_stopped=false is preserved: its successful upstream text still yields cleanup-unknown quarantine and a held lease" (`:162-164`); "Unknown started/parked work retains its leases and marks exclusive workspaces dirty; no negative method can clear quarantine" (`:182-184`); "Idle maintenance also observes that owner; a failure is sticky" (`:304-305`); spawn abandoned at deadline → "`Negative(Fenced)`, dispatch and attempt `outcome_unknown`, session quarantined, workspace dirty" (`:545-548`) | dispatch + session (store); owner retained in-process | operator |
| ADR-057 | Accepted | Done-epoch fence: "Fresh scope renewal rejects the epoch change, stops the owner and records `LostAuthority / Protocol::Unknown / canonical Done / Negative(Fenced)`" (`:178-179`); "absent receipts remain unknown" (`:177`) | dispatch (by design: Done ends the runner) | automatic |
| ADR-060 | Accepted | "requires all three observed facts: `whole_tree_stopped`, `leader_exited`, and `signals_accepted`" (`:100-101`); "Cancellation, deadline, unsupported approval or a negative/unknown cleanup path never publishes" (`:123-124`); "MacOS whole-tree uncertainty remains a hard release/send gate even when leader exit or helper exit was observed" (`:140-141`); "a held completion reference … cleanup is unproven, the held row is retained for reconciliation and not published, and the operation reports `CleanupUnknown`" (`:242-244`) | dispatch (completion withheld) | nobody in-process |
| ADR-064 | Accepted | "Any intake refusal or unknown outcome stops further polling, retaining original custody without automatic retry or synthetic denial" (`:220-221`); "UTD, malformed events … remain quarantined/unknown … A host inspection/recovery lifecycle is required" (`:155-158`); amendment: close "no longer fences the approval rooms it served" (`:256-257`), "Every failure path in this ADR still fences exactly as written" (`:259`) | approval pump (service); approval room | nobody in-process |
| ADR-065 | Accepted | "A conclusively ineligible event must not retire otherwise healthy Matrix transport" (`:11`); "Interrupted SDK calls, failed Derived persistence, coverage ambiguity and limited timelines retain their original unknown/quarantine state and conservative transport fencing" (`:100-102`) | event (terminal tombstone) vs transport (fence) | human resends / operator |
| ADR-075 | Accepted | "Do not retry shutdown or count later closure as the original success" (`:35-36`) | store shutdown verdict | nobody |
| ADR-082 | Accepted | "An already timed-out caller never becomes successful after late cleanup" (`:44`) | store shutdown verdict | nobody |
| ADR-086 | Accepted | fixture rule only: "An observed exit fails immediately" (`:25`) | test | — |
| ADR-087 | Accepted (TS) | claim/wake gap "leaving no future wake" fixed by one combined claim (`:23`, `:31-33`) | scheduler | automatic |
| ADR-091 | Accepted | two negative invalidations execute after receiver loss; "A host must retain an observed negative result before admission and stop relying on that scope if persistence fails" (`:39-41`) | transport/room | automatic |
| ADR-095 | Accepted | "Inspect ambiguous account/process creation; do not launch again on timeout" (`:25`); "uncertain effects require inspection" (`:26`); "An admitted command that loses its response returns `outcome_unknown`; the caller reconciles with the same ID" (`:74-75`) | dispatch / effect | operator |
| ADR-096 | Accepted | "A lost/unknown claim result stops this one-attempt driver" (`:107`); "Shutdown … keeps unresolved Report/physical roots until owned cleanup is settled or explicitly retained as unknown … One OS owner retains any unresolved report; shutdown failure cannot consume the last owner and pretend release" (`:160-164`); "Normal SIGTERM then deliberately retains the original service, result and both writer locks and exposes outcome_unknown" (`:201-202`); "No macOS unknown cleanup is called a complete shutdown" (`:206`); "There is no automatic replacement claim or shutdown retry" (`:212`); readiness: "503 with the full component list when any is not — never a silent 200" (`:241-242`, `/ready` at `:262-265`); amendment 2026-09-21 reverses only "Collector::close explicitly fences its transport" (`:280-287`) | process (retained), readiness (fleet) | operator (kill) |
| ADR-106 | Accepted | "Later successful cleanup must never replace the original timeout result or fabricate a sent ACK" (`:63-64`) | store shutdown verdict | nobody |
| ADR-113 | Accepted | "event reads and control pumping cannot extend" the absolute deadline; "authority renewal and process custody remain unchanged" (`:27-31`) | dispatch | — |
| ADR-117 | Accepted | unsettled file delivery at completion → "`SettlementUnknown` failure and negative observation; the driver reports `outcome_unknown`" (`:29-31`) | dispatch (→ agent via ADR-036) | operator |
| ADR-120 | Accepted | "A close that does not finish is still reported exactly as it is today" (`:37-38`) | store shutdown verdict | nobody |
| ADR-130 | Proposed | stop route "fence, never settle" (`:33`); "Automatic positive stop settlement remains G8 and requires exact original stopped-owner and workspace-inspection evidence" (`:111-112`) | dispatch | operator |
| ADR-137 | Proposed | "The private send fails closed, with a named reason … the approval is **denied**" (`:26-30`); "it **never retries silently**" (`:55`); "if the denial write itself fails, the request stays `pending` and the failure is loud" (`:59-61`) | request | automatic (deny) |
| ADR-138 | Proposed | none (observation only) | — | — |
| ADR-147 | Decided | provisioning: "Account errors retain Unknown" (`:53`); shutdown "drains all retained agents despite an individual failure … reporting aggregate OutcomeUnknown for a partially failed drain" (`:302-303`); re-attach: "an agent that cannot come back is skipped and shown, never fatal" (`:607-608`), "**One agent's failure is that agent's only.** It is registered in the fleet as `not_attached` … the fleet is not failed, readiness is unchanged" (`:636-638`); owner join: "the refusal ended the coordinator for good" (`:685-686`) → `AwaitingOwner` "is not a failure" (`:698-699`), "the fleet is not failed and readiness is unchanged" (`:712`) | agent (re-attach), fleet (not failed) | later evidence |
| ADR-148 | Decided | crashed host's dispatch is "settles it to `outcome_unknown`, quarantines the session, and the candidate query … never re-claims it" (`:57-59`); recovery is "An operator console route … never an automatic path" (`:117-118`); retained "a crashed agent is left offline for a human to inspect, never auto-restarted into new work" (`:113-114`) | dispatch/session | operator |
| ADR-149 | Decided | "An expired budget is `Error::Timeout`. `Error::Cancelled` means a real cancellation" (`:107-108`); "The fail-closed denial leg … denies the pending request at most once, whatever the class" (`:133-135`) | request | automatic |
| ADR-150 | Decided | "No automatic retry, in any form" for a failed retirement (`:143`) | engagement effect | operator |
| ADR-151 | Accepted | stale-session refusal is operator-only; "commit loss remains explicit uncertainty" (`:30`) | intake batch | operator |
| ADR-153 | Decided | "An absent/lost reply is not positive evidence and invokes existing conservative failure fencing" (`:40-41`); "A changed generation runs existing route/approval/reply retirement" (`:37`) | project room (all agents) | operator (explicit recovery of negative scopes) |
| ADR-156 | Accepted | "Started operation drop/error permanently closes the stream … asks its original guardian to stop" (`:48-49`) | dispatch | host |
| ADR-160 | Accepted | "A refused recognized elicitation returns cancel before closing the session; other unsupported RPC families retain protocol rejection." (`:23-24`) | session (that dispatch) | automatic |
| ADR-161 | Accepted | "ordinary and parked authority checks renew only five seconds at a time, capped by the original capability expiry" (`:32-34`); "Each callback remains capped by the original remaining operation lifetime; a late request never restarts its clock" (`:39-40`) | dispatch | — |
| ADR-162 | Accepted | inspection "only after actual full stop and an acknowledged negative fence" (`:16-17`); "Recording it never settles the stop or frees any lease" (`:32-33`); "A worker that waits carries `awaiting_operator` … the fleet counts an agent as lost only when it failed and is not waiting" (`:74-76`); "here the waiting worker does not take in new requests until it is resolved" (`:81-82`); "Waiting: …" notice (`:98-101`) | dispatch + session; agent alive only if stopped+inspected | operator |
| ADR-163 | Accepted | TS "retains an explicit unavailable marker when that cleanup was not confirmed" (`:11-12`); cap excludes only attempts "with a matching ADR162 receipt" (`:19-20`) | occupancy | — |
| ADR-166 (no frontmatter) | — | the approval pump "refused a sync response because an earlier receipt had the same next_batch cursor but a different body" (`:6-7`); "the retained batch refuses instead of fabricating proof. No synthetic sync token, journal clearing, new owner, or automatic send retry is introduced." (`:14-16`) — a live instance of F8 | approval pump | nobody |
| ADR-164 / 165 / 170 | Accepted | continue / accept_completed / keep_blocked port; "Missing proof remains a refusal" (`164:20`); "Older failures without that receipt remain unresolved" (`165:14-15`) | dispatch | operator |
| ADR-168 | Accepted | "Preserve the original failed dispatch, stop receipt, workspace inventory and cleanup verdict" (`:16`) | evidence | — |
| ADR-169 | Accepted | "Only an original report with stopped physical custody and a recorded inspection can wait this way" (`:16-17`); "This is not automatic retry … or restart recovery" (`:19-20`) | agent (retained across a resolvable failure only) | operator |
| ADR-173 | Accepted | "The real active handoff still checks ApprovalHost::fits and refuses an insufficient operation" (`:26-27`) | dispatch | config |
| ADR-174 | Accepted | 429: "A final failure still follows the existing conservative fencing path" (`:20`); amendment: connect-phase redial, "When attempts run out the failure is the same Transport word: the collector fences exactly as before" (`:68-69`); "A fenced worker still does not resume, so restart and recovery are unchanged" (`:95-96`) | transport → worker | operator (generation) |
| ADR-175 | Accepted | "Existing owner retention and explicit recovery rules remain unchanged" (`:24`); handoff refusal before an Operation: "Worker retention and all admission behavior remain unchanged" (`:37`) | agent (worker ends) | restart |
| ADR-176 / 179 | Accepted | pacing/SDK budget are config; "It cannot settle the earlier unknown file room write or recreate a failed factory owner" (`176:27`); "The service exited1 before any runner task" (`179:10`) | — | — |
| ADR-178 | Accepted | selection RunnerAuthority "the driver mapped, like every selection error and without a log line, to OutcomeUnknown; the continuous worker ended with no diagnostic" (`:68-70`) → now re-resolve once, "A session the fresh resolution still names remains a failure" (`:77-78`) | agent (worker) for ordinary hosts | automatic (factory) / nobody |
| ADR-180 | Accepted | "a notice send that is refused or unknown ends the assignee's worker, not only that attempt; that is the existing fail-closed rule and is not softened here" (`:131-134`); "an approval the owner does not answer ends the agent (outcome-unknown, owner retained)" (`:61-62`, later reversed by ADR-046) | agent | restart |
| ADR-181 | Proposed | "a write that fails never changes the attempt's outcome and is counted, not retried" (`:51-52`); "This changes no verdict: every site still fails; it only stops discarding the reason" (`:83-84`) | none (evidence) | — |

### 1b. Media, usage, console, provisioning-wire and early product records (extraction passes)

Verified by `grep -F` against the files: every quote in 1b-i and 1b-ii marked
with an asterisk; the rest are the extraction pass's verbatim copy, unchecked
by me.

#### 1b-i. Media, file delivery, upload custody

| ADR | Status | Rule stated (quoted) | Scope | Resolver |
|---|---|---|---|---|
| ADR-027 (TS) | Accepted | "Only an acknowledged Matrix event is delivered. Queued or failed uploads are never reported as sent." (`:23-24`) | upload | automatic (outbox retry) |
| ADR-058 | Accepted | "A future service adapter must make an explicit worker and unknown-outcome/cancellation design before enabling this in a live request." (`:115-116`) | file | — |
| ADR-061 | Accepted | "The SDK encryption constructor documents a panic if its randomness source fails; no fallback key or plaintext result is provided." (`:49-50`) | process (panic) | nobody |
| ADR-066 | Accepted | "Errors quarantine new writes without trimming tails or inventing a replacement ID." (`:40`); "A future asynchronous adapter must retain this actual owner until its synchronous operation and recovery have finished." (`:70-71`) | store; owner retained | nobody |
| ADR-068 | Accepted | "There is no old unauthenticated fallback, proxy, retry, query token, URL preview or thumbnail." (`:46-47`) | transport | automatic |
| ADR-072 | Accepted | "The library performs no automatic retry"* (`:41`); "even a failure before the socket writes is conservatively unknown once execution may have begun" (`:34-35`) | upload | nobody |
| ADR-073 / 074 / 076 / 077 | Accepted | terminal refusals; "Lost domain/ACK results recover exact receipts, never manufacture new current authority" (`074:34-35`); "A failed writer response never implies permission to expose data or retry it automatically" (`076:58-59`) | dispatch / transport | automatic (refusal) |
| ADR-078 | Accepted | "restart NEVER reset WritePossible"* (`:71`); "No NotSent, retry, reset, replacement-ID or expiry-based resend API exists" (`:80-81`) | upload | nobody |
| ADR-079 | Accepted | "once IO may have begun the original Store retains custody and uncertainty" (`:33-34`) | store; owner retained | nobody |
| ADR-083 | Accepted | "Resend stays Terminal in either case." (`:46`) | upload | nobody |
| ADR-084 | Accepted | "Failed persistence poisons this owner's upload execution"* (`:56`) "until explicit owner close" (`:57`); "absent acceptance stays unknown forever without a resend grant" (`:58-59`) | upload owner | explicit close |
| ADR-085 | Accepted | "Acceptance may be recorded after revocation, lease expiry, task completion, privacy promotion or process exit" (`:42-43`) | upload | automatic (historical) |
| ADR-089 | Accepted | "Collector::close now borrows its caller so refusal cannot consume the last retained owner. It atomically refuses retained jobs before closing admission."* (`:88-89`); "Persistence failure poisons the SDK owner" (`:80`) | transport; close refused | explicit `reopen_upload_owner` |
| ADR-090 / 097 / 100 | Accepted | "A possible upload/event or unknown staging remains outcome_unknown even if cancelled" (`097:78-79`); "there is no pruning, reset or automatic resend" (`097:90`) | file / dispatch | nobody |
| ADR-098 | Accepted | "Collector close refuses unresolved publication custody."* (`:25`); "An unmatched retained job remains unknown and prevents close"* (`:34`) | transport; close refused | nobody |
| ADR-101 | Accepted | "Driver keeps its original Report and physical root until real owned cleanup. Only once both owners acknowledge safe closure does Bootstrap close the SAME Collector and then its original writers." (`:240-242`); "It records a sticky Unknown if that close itself returns Unknown"* (`:244`); "Negative: unknown jobs consume finite capacity and can prevent clean shutdown."* (`:306`); "two uncertain jobs can intentionally exhaust the service until host investigation" (`:313-314`) | process (shutdown), service capacity | operator ("host investigation") |
| ADR-104 | Accepted | "It never exits the application. The parent accepted this exceptional retention policy before implementation."* (`:58-59`); "It is never reclaimed by a timeout; process termination ends that physical custody."* (`:93-94`) | process (a worker parked forever, Windows) | process termination |
| ADR-105 | Accepted | "Lost claim or intake evidence halts the attempt." (`:105`); "Pending or failed close remains sticky unknown." (`:257`); "This first profile does not recover an interrupted write into Ready after process death." (`:217-218`) | file / attempt / close | nobody |
| ADR-117 | Accepted | see 1a | dispatch | — |
| adr-167 (no frontmatter) | "Accepted" in body | "no uncertain upload is retried" (`:18`) | upload | nobody |
| ADR-177 | Accepted | "If still unknown at expiry, report unresolved."* (`:19`); "It cannot settle historical unknowns." (`:24`) | agent (model guidance) | agent polls |

#### 1b-ii. Usage, console, retention sweeps, service units, release

| ADR | Status | Rule stated (quoted) | Scope | Resolver |
|---|---|---|---|---|
| ADR-055 / 067 / 069 / 071 / 118 / 119 / 157 | Accepted / Proposed | refusals and unknown-not-zero; "Parse failure is an idempotent unknown observation, not a successful zero-token measurement" (`063:37-38`) | observation | automatic |
| ADR-063 | Accepted | "Timeout is outcome unknown"* (`:81-82`); "the caller must retain the original source/call/content identity" (`:82-83`) | store | caller |
| ADR-070 | Accepted | "A pending or refused write stops further capture with an explicit failure"* (`:34-35`), "while the existing owned runner cancellation, cleanup, canonical completion and delivery rules still run" (`:35-36`) | dispatch (usage) | explicit retry of the same tuple |
| ADR-107 / 108 / 111 | Accepted | "Caller/reply loss, response timeout or authority loss after a possible commit remains outcome_unknown … No write is automatically retried."* (`108:58-60`) | console / store | operator (visible retry) |
| ADR-109 | Accepted | frozen publication retried unchanged (`:35-38`); "Startup never calls register or changes a generation to gain attachment." (`:68-69`) | publication / startup | automatic |
| ADR-121 / 122 / 123 | Proposed | unknown-not-zero; "A refusal grants no retry, reply, lease or completion authority." (`122:52-53`) | admission | automatic |
| ADR-124 | Proposed | "**Refusal-on-tick rule.** On `Busy` or `OutcomeUnknown` the tick logs the refusal code with the `[ceiling]` prefix and waits for the next tick — never an in-line retry"* (`:154-156`); "An alert is diagnostic, never enforcement" (`:83`) | sweep | automatic (next tick) |
| ADR-125 | Proposed | "**Unknown fate is retained — a named product decision.**"* (`:79`); "A retention failure is never a work refusal."* (`:95`); "On `Busy` or `OutcomeUnknown` a phase logs its refusal … and waits for the next tick — never an in-line retry." (`:274-277`) | sweep / store | automatic (next tick) |
| ADR-126 / 132 / 138 / 145 | Proposed | observation only; "a failing component **never renders ready**"* (`145:21`); "the strip cannot restart anything" (`145:32`) | console | — |
| ADR-127 | Proposed | "when the close outcome is unknown it **parks** (keeps the status endpoint and the original owner, refuses a false successful exit …) rather than exiting 0"* (`:17-20`); "a parked unknown-close exceeds it by design and is SIGKILLed holding an unrelinquished owner — the honest terminal state"* (`:41-42`); "**Start gate is `/ready`, never `/health`**"* (`:45`); `Restart=on-failure`, `TimeoutStopSec=20` (`:32-33`) | process (park; SIGKILL at 20 s) | systemd |
| ADR-129 | Proposed | "a retention failure is never a work refusal" (`:98`); MCP helper "exits with a class code" (`:53-55`) | retained files / helper process | automatic |
| ADR-133 | Proposed | "KeepAlive is `true`" (`:37`): "restarts on **any exit: a crash, a clean exit-0, and a pid-kill** alike"* (`:46`); start gate polls `/ready` (`:66-68`); references the parked unknown-close (`:18-19`) | process | launchd |
| ADR-134 | Proposed | release workflow refusals; readiness contract unchanged (`:64-66`) | release | automatic |
| ADR-135 | Proposed | stop contract: "the process exits 0 inside the budget **or parks on an unknown close without exiting 0**"* (`:111-112`) "— a park is not a failure; SIGKILL at the budget leaves the WAL"* (`:113`); restart must preserve pending/unknown rows (`:119-122`); "rollback to the JS service as the authority is refused" past step 8 (`:132-134`) | process / store / cutover | operator |
| ADR-136 | Accepted | Windows lane "does not block" (`:25-27`) | CI | — |
| ADR-146 | Proposed | "**G8 is reopened**"* (`:13`): the global stop sweep "is removed" (`:15-16`); `expire` → `lose` → `outcome_unknown` (`:258-260`); recovery "never automatic"* (`:282-283`) | dispatch / session | operator |

#### 1b-iii. Early product (TS-era), protocol foundations, Claude runtime, outbound custody, qualification gates

Records with no failure rule at all: ADR-001, ADR-015 (no frontmatter id),
ADR-017, ADR-022. Two ids name two files each (ADR-035, ADR-037); ADR-166 and
adr-167 have no frontmatter id.

| ADR | Status | Rule stated (quoted) | Scope | Resolver |
|---|---|---|---|---|
| ADR-002 / 003 / 004 / 006 / 007 / 008 / 009 (TS) | Accepted | refusals and fail-closed startup; "A non-empty store without a readable identity fails closed." (`008:28`); "a failure to journal is surfaced as a warning rather than swallowed" (`007:65-66`) | store / transport | automatic |
| ADR-010 (TS) | Superseded | "A wake or switch requires a verified exit of the previous process." (`:35-36`) | session/process | automatic (superseded by ADR-011) |
| ADR-013 (TS) | Accepted | "It must not terminate running engagements"* (`:227`) | engagement | operator |
| ADR-014 (TS) | Accepted | "**6. A dead credential is a human-visible state, not a retry loop.**"* (`:389`) | agent credential | operator |
| ADR-016 (TS) | Accepted | "refuse new auto-joins against an exhausted ceiling, and NEVER stop a running agent"* (`:458`); a failed card delivery "calls `denyPending`, and **the request is denied.**"* (`:739`) | agent / request | operator (budget); automatic (deny) |
| ADR-018 / 020 / 023 / 024 / 025 (TS) | Accepted | "A blocked or uncertain task requires its existing explicit recovery."* (`020:35`); "Failed dispatches keep their prior position."* (`023:111`); "an unknown or failed departure must remain visible as such." (`025:32`) | task / dispatch / agent allocation | operator |
| ADR-028 / 039 / 115 / 116 / 143 (approval scope, wire) | Accepted / Proposed | "unknown scopes and file-change callbacks without enough detail retain once/deny only." (`028:25-26`); "A verdict from any sender other than the fleet's representative is refused before `approve`." (`143:128-129`) | grant / wire | automatic |
| ADR-031 | Accepted | "Failure fences the failed node's process just like cancellation. Uncertainty never becomes automatic retry."* (`:40-41`); "started, parked and unknown work retains leases and durable stop intents until host inspection." (`:69-70`) | dispatch / task / process | host inspection |
| ADR-033 / 034 (control evidence, periodic loop) | Accepted | "uncertainty never causes an automatic replay." (`033:22-23`); "conflicting post-intent observations remain unknown without rollback." (`034:32-33`) | process | nobody |
| ADR-035 (both) / 036 (metadata) | Accepted | "requires a distinct explicit recovery attempt without removing old guards or rewriting failures." (`035-distinct-fault-attempt:12-13`); "fail closed on unsupported or incomplete observations." (`036-native-process-metadata:15`) | process | operator |
| ADR-037 (outbound custody) | Accepted | "Started claims become `unknown`"* (`:89-90`); "Unknown work blocks later Matrix work until the current host inspects it."* (`:90-91`) | transport / store | host inspection |
| ADR-037 (stage release) / 038 | Accepted | "Unknown postrelease outcomes remain consumed and never trigger rollback, loop recreation, retries" (`037-native-stage-release:17-18`); retirement "fences claimed ACKs and input scheduling" (`038:79-80`) | process / session | nobody / automatic |
| ADR-041 / 042 / 049 / 051 | Accepted | "The client has no automatic retry" (`041:23`); "the watchdog terminates this helper process with exit code 74"* (`049:46`); "Failed graph reporting intentionally fences the capability" (`051:69-70`) | task client / helper process | caller / parent |
| ADR-045 | Accepted | "Restart or expiry requeues only unstarted claims; Sending becomes Uncertain."* (`:22-23`) | task notice | host inspector |
| ADR-052 / 054 / 056 | Accepted | "Unknown retains the outstanding token and blocks further claims until explicit host inspection settles it."* (`052:97-98`); "An interrupted Applying stage therefore stays explicit OutcomeUnknown" (`054:39-40`); "Pending local acceptance becomes Unknown on retirement, never automatically NotAccepted." (`056:113-114`) | dispatch / transport | host inspection |
| ADR-059 / 062 | Accepted | "Uncertain writes are never automatically resent, even with a stable Matrix transaction ID."* (`059:133-134`); the workflow test "requires leader exit, explicit CleanupUnknown, canonical Done, retained workspace lease and no final claim/HTTP PUT"* (`062:81-83`) | transport / dispatch | inspection; retained owner |
| ADR-080 / 088 / 094 / 099 | Accepted | fixture/observation only; "No second shutdown or later success repairs that result." (`088:24-25`) | store shutdown verdict | nobody |
| ADR-093 | Accepted | "Lost start results yield no binding and no child."* (`:57`); "No cleanup uncertainty releases a new execution grant or current file permission." (`:87-88`) | dispatch / workspace | automatic |
| ADR-102 | Accepted | "A failure or lost acknowledgement after Preparing is Unknown" (`:92-93`); "any ambiguous enrollment must stop for a separate operator recovery workflow."* (`:285`) | transport (SDK) | operator |
| ADR-110 / 112 | Accepted | "an undeliverable private card is **never silently re-sent under the same request id**" (`110:74-75`); "Panic/abandonment leaves explicit Unknown and blocks new mutations." (`112:69`); "a timeout never authorizes a replacement collector or claims successful shutdown." (`112:128-129`) | approval request / delivery owner | operator; deny via ADR-137 |
| ADR-114 | Accepted | "where readiness is unknown the dispatch **parks** with the named reason `account_readiness_unknown`"* (`:119-120`); "A logout failure leaves readiness unknown, never ready" (`:150-151`) | dispatch (parked) | operator login |
| ADR-139 / 140 / 144 | Accepted / Proposed | qualification gates "FAIL — never skip" (`140:43-44`; `144:79-80`) | CI gate | operator |
| ADR-152 / 154 / 155 / 158 / 159 / 171 / 172 | Accepted | "Failure or future drop closes the stream driver permanently and the owned wrapper requests bounded stop through its original guardian."* (`155:14-15`); "The original operation guard closes the session and asks its retained owner to stop on errors, uncertain IO or cancelled waits."* (`158:41-42`); "Failed publication … poisons the original owner and leaves uncertainty inspect-only." (`152:39-40`) | session / process | host |
| ADR-057 / 160 / 166 | Accepted | see 1a | — | — |

## Part 2. Contradictions, reversed-but-dependent rules, and rules with no record — ranked

Each finding: the records, the code that applies the rule, the retained
product's rule, and which of G1–G6 it belongs to. The operator's four are F1–F4.

### F1. One fact, two blast radii: a failed turn ends the agent (ADR-036) while the store scopes the same fact to the session (ADR-053/162/169)

- Records: ADR-036:293-299 defers agent survival to "the recovery work";
  ADR-053:182-184 and ADR-162:74-82 scope the consequence to dispatch and
  session; ADR-169:16-17 lets the worker survive only "with stopped physical
  custody and a recorded inspection".
- Code: `run_continuous` returns on any `Err` of one attempt
  (`R/hagency/src/bootstrap/driver.rs:330-334`) and on a report that failed,
  is unsettled or is not physically stopped (`:367-383`); the thread then only
  answers `Close` (`:191-207`). "Recoverable" requires `physically_stopped`
  AND `StopInspectionStatus::Recorded` (`:373-375`); the inspection is
  captured only when `stopped(report.cleanup)` and the store answered
  `Negative(Fenced)` (`R/hagency-execution/src/operation.rs:844-857`). The
  store side is session-scoped: `observe_owned_failure` writes the notice and
  `fence_dispatch` (`R/hagency-store/src/domain/owned_dispatch.rs:597-624`),
  which quarantines the session and dirties the workspace
  (`R/hagency-store/src/domain/conversation_lifecycle.rs:78-87`); the
  "Waiting" answer exists (`R/hagency-store/src/domain/task_intents.rs:32`).
  A lost agent then makes the fleet `outcome_unknown`
  (`R/hagency/src/bootstrap/fleet.rs:48-50,111-114`) and `/ready` 503
  (`R/hagency/src/lib.rs:409-415,244-252`).
- Retained: `settleUnknownInternal` settles the dispatch, revokes the
  capability, deletes leases, dirties the workspace, blocks the task and posts
  the notice (`T/router/src/store.ts:2623-2660`); the runner returns
  `outcome_unknown` with the provider's words as `terminal_reason`
  (`T/router/src/runner.ts:481-490`); the pump's `.catch` only requeues or
  cancels a still-`leased` dispatch and otherwise logs and reschedules
  (`T/backend-v2.js:2735-2756`). Nothing marks the agent.
- Consequence today: every failed or unknown turn whose cleanup is not proven
  ends the agent for good, and the only path back is a service restart.
- Gap: G1. Contradiction between ADR-036 (agent) and ADR-053/162/169 (session)
  is explicit in ADR-036's own text; the code follows ADR-036.

### F2. Shutdown keeps the last owner (ADR-096) after the operator ruled that a clean stop must be no worse than a crash (ADR-047 amendment)

- Records: ADR-096:160-166, 199-212 (retained owner; SIGTERM "deliberately
  retains the original service, result and both writer locks"; "No macOS
  unknown cleanup is called a complete shutdown"); ADR-047:239-243 ("An
  orderly stop was strictly more destructive than a crash … The retained
  product fences nothing at shutdown") reversed only the Matrix fence, and
  ADR-096:280-287 records only that sentence as reversed. Dependent rules left
  standing: ADR-053:128-129, ADR-060:140-141, ADR-130:111-113, ADR-096:206.
  The park is also decided, as a feature, in three Proposed service records:
  ADR-127:17-20 ("when the close outcome is unknown it **parks** … rather than
  exiting 0") and `:41-42` ("SIGKILLed holding an unrelinquished owner — the
  honest terminal state"); ADR-135:111-113 (the cutover stop contract: "or
  parks on an unknown close without exiting 0 … a park is not a failure");
  ADR-133:18-19 by reference. And the file-service records make unknown *file*
  jobs block the close too: ADR-101:240-244, `:306` ("unknown jobs consume
  finite capacity and can prevent clean shutdown"), ADR-098:25, `:34`,
  ADR-089:88-89; ADR-104:58-59 goes furthest ("It never exits the
  application"). Reversing ADR-096 alone leaves six records deciding the same
  park.
- Code: the driver answers `Close` with `Err(OutcomeUnknown)` whenever the
  report is not stopped (`driver.rs:194-206`, predicate `:273-276` =
  `!retains_process_custody()`, `operation.rs:525-532`); `Bootstrap::close`
  returns `Err` on any child failure before closing anything else
  (`R/hagency/src/bootstrap.rs:1394-1399`); `serve` then never calls
  `stop_graceful` and awaits the server forever (`:1567-1580`); `main`
  parks on `std::future::pending` (`R/hagency/src/main.rs:297-303`).
- Retained: `shutdown()` always reaches `process.exit(0)` in `finally`
  (`T/backend-v2.js:17386-17416`); `stopServer` aborts every live runner and
  awaits their settlement (`:17622-17624`); the cleanup receipt is bounded at
  8 s (`T/router/src/runner.ts:368-377`); an unconfirmed cleanup fences the
  **agent** (`manualDown`, `offlineReason: 'runner-cleanup-unconfirmed'`,
  `stopUnconfirmedDispatches`, persisted) and a later confirmed close clears
  it (`T/backend-v2.js:2717-2733`; ADR-019:113-116). The retained product's
  fail-closed rule for unproven cleanup exists — at agent scope, persisted,
  clearable by later evidence — and never at process scope.
- Observation that sharpens the review's item 3: the port's in-memory retained
  owner is strictly *weaker* than the retained product's persisted agent fence
  (a `kill -9` clears it, and ADR-147 re-attaches the agent with no fence;
  `R/hagency/src/bootstrap/fleet.rs:337-383` reads no cleanup evidence) and
  strictly *more destructive* to the process. Reversing ADR-096's rule is not
  giving up fail-closed; it is moving the fence to where the retained product
  keeps it.
- Gap: G2.

### F3. The host re-qualifies an idle warm owner every 100 ms under `response_ms` and latches one late reply as `LostAuthority` — no ADR decides the cadence or the bound; ADR-029's reversal never reached it

- Records: ADR-029:377-416 reversed the tracker's refusal ("The refusal was
  the port's own invention", `:394`). ADR-053:105-106 decides 100 ms for
  authority renewal "while a native operation is pending"; ADR-053:304-305
  decides that an idle-maintenance failure "is sticky"; ADR-040:150 decides
  "Unknown observation is sticky and cannot rearm"; ADR-113 only separates
  `event_wait_ms` from `response_ms`. No record decides that idle
  qualification runs every 100 ms, nor that its bound is `response_ms`
  (hard-capped 2 s, `R/hagency-execution/src/host.rs:38`).
- Code: idle loop ticks at 100 ms and calls `qualify` bounded by
  `idle_until.min(now + response_ms)` (`R/hagency-execution/src/warm.rs:610-616`);
  `qualify` performs a synchronous guardian round trip capped at 5 s and maps
  any error to `LostAuthority { WarmQualify, Io }` (`:532-534`); the
  supervisor latches the first observation error forever
  (`R/hagency-platform/src/supervisor/unix.rs:383-397`); the guardian answers
  an `Observe` only between 25 ms whole-system censuses
  (`unix.rs:718-745`).
- Retained: no continuous host-side qualification of a running guardian; the
  runner lease is 20 min (`T/backend-v2.js:246`), liveness is a 120 s
  heartbeat TTL (`T/lib/supervisor-lifecycle-manager.js:14`) with 30 s grace
  (`T/lib/agent-state.js:14`). I found no in-run store-refusal or
  observation-timeout path that aborts a healthy retained runner (see "could
  not determine").
- Gap: G3. Rule applied in code, decided in no ADR (the sticky latch is
  decided; the cadence and the bound are not; the "follow TS" reversal
  covers the tracker only).

### F4. A store refusal of an approval *request* is `LostAuthority` and stops the runner, after ADR-046 decided that a refused approval is a decline

- Records: ADR-046:421-434 (policy refusal → decline), `:456-468`
  (owner-wait expiry → decline), `:528-562` (no shape refusal; "The TypeScript
  product denied and carried on", `:428`). ADR-046 itself keeps one fatal
  case: "An unknown expiry denial sends nothing and ends the operation with
  LostAuthority" (`:488-489`). ADR-180:61-62 recorded the pre-amendment
  fatal outcome.
- Code: `request_owner_approval` error → `LostAuthority { ApprovalRequest, cause }`
  (`R/hagency-execution/src/approval/observations.rs:211-215`); any terminal
  from `update` stops the runner (`:270-274`); an approval raised late in a
  turn is a `Deadline` because the response deadline must fit the operation
  (`R/hagency-execution/src/approval/state.rs:230-236`; decided by
  ADR-161:39-40).
- Retained: a park refusal writes the family's own `decline` and returns for
  Codex; only an MCP-elicitation park refusal throws, which settles the
  dispatch unknown (`T/router/src/runner.ts:660-664`, `:498`); the parked
  runner's lease is the runner lease plus the approval TTL
  (`T/backend-v2.js:2703`); ADR-011:170-172. The port's ADR-160:23-24 (a
  refused MCP elicitation "returns cancel before closing the session") matches
  the retained MCP case at dispatch scope; it is the command/file/permission
  families where the port diverges.
- Gap: G4. Contradiction inside ADR-046 (`:488-489` vs `:428, 537-541`) and
  between ADR-046 and observations.rs; ADR-161's "a late request never
  restarts its clock" is a decided divergence from the retained lease+TTL
  that ADR-161 does not name as one.

### F5. The Matrix fence is durable and restart rules honour it, so a transient endpoint failure needs an operator rotation — and the only records deciding retry decide four attempts

- Records: ADR-047:92-93 (fence "after any incomplete authenticated
  collection"), `:102-104` (stop using an incarnation when negative
  persistence returns Busy/unavailable/OutcomeUnknown), `:285-286` (a fenced
  generation stays unavailable; startup must not rotate); ADR-096:23-24
  (same); ADR-174:18-20 (429 retry then "the existing conservative fencing
  path"), `:63-69` (connect-phase redial, four attempts, then "the collector
  fences exactly as before"), `:95-96` ("A fenced worker still does not
  resume, so restart and recovery are unchanged"); ADR-147:639 ("A fenced
  generation still refuses, as before").
- Code: `fence_read` fences every non-cancelled error
  (`R/hagency-matrix/src/collector.rs:274-284`), `fence_observation` writes
  `invalidate_matrix_transport` for both known incarnations (`:286-334`), the
  store sets `available=0` (`R/hagency-store/src/domain/matrix_routes.rs:221-224`)
  and a same-generation positive cannot restore it (`:213-219`); the driver's
  refresh error ends the attempt (`R/hagency/src/bootstrap/driver.rs:477-481`)
  and, via F1, the worker; re-attach refuses the fenced generation
  (`fleet.rs:369-380`, status `not_attached`).
- Retained: unbounded exponential backoff with the cursor retained and no
  persisted fence for read-side failures (`T/lib/appservice-sync.js:463-522`);
  a single agent's account failure is skipped with a warning
  (`T/bridge-matrix.js:4284-4296`).
- Gap: G5 (second half). Consistent across the port's records, and every one
  of them is stricter than the retained product; the parity doc names the
  resumable fence as an operator decision
  (`docs/design/native-execution-parity.md:731-733`).

### F6. One agent's room-read failure retires the shared project room and fences every other agent's transport (`Generation` cascade)

- Records: ADR-047:95-97 ("failed, malformed or unavailable snapshots
  invalidate the exact previously captured room generation, retiring other
  Agents' old room routes too"); ADR-153:37, 40-41; ADR-178:74-80 handles the
  *selection* side (re-resolve once on RunnerAuthority) only; ADR-047:279-283
  narrowed the room fence for a *cancelled* read only. ADR-065:11 states the
  opposite principle for events ("must not retire otherwise healthy Matrix
  transport"). ADR-147:636 states "One agent's failure is that agent's only".
- Code: a failed room-state read (not `Cancelled`) invalidates the prior
  room scope (`R/hagency-matrix/src/collector.rs:477-503`); the next agent's
  collection then fails `Generation` (`:167-173, 260-265`) and `fence_read`
  fences its transport (`:274-284`).
- Retained: per-agent skip (`T/bridge-matrix.js:4284-4296`); no shared fence.
- Gap: G5. Decided (ADR-047:95-97) but contradicts ADR-065:11, ADR-147:636 and
  the retained rule.

### F7. One lost agent makes the fleet `outcome_unknown` and `/ready` 503; ADR-096's readiness amendment decides the vocabulary, nothing decides that rollup

- Records: ADR-096:230-278 (component states; "never a silent 200"; "names
  states, never private counts or fleet detail"). ADR-147:636-638 (a
  non-re-attached agent does not fail the fleet or readiness) and ADR-162:74-76
  (a waiting agent is not lost) each carve one exception; the general rule
  "any other failed agent fails the fleet" is decided nowhere.
- Code: `lost` = error and not `awaiting_operator`
  (`R/hagency/src/bootstrap/fleet.rs:45-50`); one lost entry makes the fleet
  word `outcome_unknown` (`:111-114`); `/ready` maps it to
  `ComponentState::OutcomeUnknown` (`R/hagency/src/lib.rs:412-415`) which is
  not ready (`:244-252`).
- Retained: no global failed flag (review G5); `/health` always 200 with body
  detail (ADR-096:253-260).
- Consequence the service records add: the systemd and launchd start gates
  poll `/ready` (ADR-127:45-48, ADR-133:66-68), so a fleet with one lost agent
  never passes its own start gate after a restart.
- Gap: G1/G5. Rule applied in code, decided in no ADR.

### F8. The approval pump is one task that stops on the first error, and every agent's next claim then fails `OutcomeUnknown`

- Records: ADR-064:216-221 decides the pump stops ("Any intake refusal or
  unknown outcome stops further polling, retaining original custody without
  automatic retry or synthetic denial"); ADR-046:509-527 patched one trigger;
  ADR-137:26-33 decides a failed *send* denies that one request. Nothing
  decides what a stopped pump does to the agents.
- Code: send not accepted or send error → `Err(OutcomeUnknown)` and the
  drain ends (`R/hagency/src/bootstrap/approval.rs:238-248`); intake refusal
  or retained pending → the same (`:289-297`); the spawned drain marks the
  service status failed (`R/hagency/src/bootstrap.rs:1536-1539`); a driver
  reserving a notice slot on the dropped receiver fails `OutcomeUnknown`
  before claiming (`R/hagency/src/bootstrap/driver.rs:595-601`).
- Retained: approvals are parked per runner (`T/router/src/runner.ts:650-664`);
  I found no single service-wide approval pump to compare (could not
  determine the retained card-delivery failure scope).
- Seen live and recorded: ADR-166:6-7 (the pump refused a sync response over
  a cursor/body mismatch; the record adds a cursor rule and "no … automatic
  send retry", `:15-16`), ADR-046:509-527 (a host-decided request stopped the
  pump and "every agent of the fleet ended `outcome_unknown`"), ADR-174:86-90
  (one failed `POST keys/query` dial "stopped the approval pump and ended the
  coordinator and an agent").
- Gap: G4/G5. Half decided (ADR-064) with an undecided fleet-scope
  consequence; ADR-137's per-request denial is the model that should govern.

### F9. The records promise a stop retry the guardian cannot serve

- Records: ADR-040:115-119 ("an earlier failed operation stop can also be
  retried on final drop"), ADR-053:113-114 ("failed operation, explicit stop,
  failure reconciliation and final drop can each retry"), ADR-040:109-110
  ("which a retained caller can inspect and retry"), ADR-048:146-147.
- Code: the guardian reports once and exits (`unix.rs:769` then process end);
  the host caches the first report and every later `wait`/`stop` returns it
  (`R/hagency-platform/src/supervisor/unix.rs:517-519, 571-574`);
  `retry_stop` therefore re-reads the same verdict
  (`R/hagency-execution/src/operation.rs:534-562`). ADR-029:180-182 requires
  two empty censuses inside the guardian's own 2 s stop
  (`R/hagency-platform/src/supervisor/unix/macos.rs:144-153, 209-218`), and
  the refusal category is now recorded (`:223-248`, ADR-181 §5).
- Retained: 8 s wait, guardian exit 125 with the reason on stderr
  (`T/router/src/runner.ts:373`, `T/router/src/runner-guardian.ts:139-144`);
  a later confirmed close may clear the agent fence (`runner.ts:393-399`).
- Gap: G2. Inconsistency between three records and the implementation; the
  ADRs' "retry" is a re-read.

### F10. `awaiting_operator` excludes the two live signatures: recovery in-process exists only for the case that does not need it

- Records: ADR-162:16-17 (inspection only after full stop and negative fence),
  ADR-169:16-17, ADR-162:74-76. ADR-096:160-166 keeps the unproven-cleanup
  owner; ADR-036:293-294 ends the worker.
- Code: `driver.rs:373-378`; `operation.rs:844-857`. A `CleanupUnknown` or a
  `LostAuthority` whose requested stop did not prove the tree (the 09-19 and
  09-22 losses) is not recoverable, so the worker ends AND the process cannot
  exit (F2): the operator's only tool is `kill -9`, which the operator's own
  record confirms restarts fine (memory 2026-09-22, ADR-047:236-238).
- Gap: G1+G2. Not a contradiction inside any one record; the composition of
  ADR-036 + ADR-096 + ADR-169 leaves the most common failure with no resolver.

### F11. Lease renewal treats `Busy`, `Unavailable` and a 2 s reply timeout as revocation; ADR-053 names only "revoked, expired, replaced, dirty or quarantined", and ADR-181 would freeze the wider rule

- Records: ADR-053:82 (the five scopes that stop the owner), `:106-107` ("A
  pending writer receipt may take up to its two-second bound before authority
  becomes *unknown*"), ADR-161:32-34 (5 s renewals); ADR-181 §4 (`:83-84`)
  keeps every site fatal. The port already decides the opposite for its own
  background writers: "On `Busy` or `OutcomeUnknown` the tick logs the refusal
  code … and waits for the next tick — never an in-line retry" (ADR-124:154-156),
  "A retention failure is never a work refusal" (ADR-125:95, `:274-277`), and
  the readiness rule that "a refused tick is a live loop" (ADR-096:273-276).
  One writer, two meanings for the same `Busy`: transient for a sweep, fatal
  for a running agent.
- Code: every 100 ms the renewal maps any store error to
  `LostAuthority { LeaseRenew, cause }` (`R/hagency-execution/src/operation.rs:1025-1036`);
  the store returns `Busy` for a full byte permit or queue and
  `OutcomeUnknown` at 2 s (`R/hagency-store/src/domain_worker.rs:2734-2780`);
  `AuthorityCause` now names `busy`, `unavailable`, `timed_out`
  (`operation.rs:100-135`) but the verdict is unchanged (`:182-186`).
- Retained: lease 20 min, heartbeat TTL 120 s + 30 s grace (F3 citations);
  the store, not the host, expires a lease (`T/router/src/store.ts:3826-3858`
  on restart; natively `R/hagency-store/src/domain/execution.rs:284-290`).
- Gap: G3. Rule applied in code beyond what ADR-053 decides.

### F12. A fresh agent's admission failure ends `serve`

- Records: ADR-147:636-638 decides "that agent's only" for re-attach; nothing
  decides fresh admission.
- Code: `self.admit(...).await?` (`R/hagency/src/bootstrap/fleet.rs:439`)
  returns from `run`, which ends `serve`'s select
  (`R/hagency/src/bootstrap.rs:1557-1563`) and closes the service; a
  non-re-attached admission failure also sets `failed` (`fleet.rs:326-329`).
- Retained: skip and warn (`T/bridge-matrix.js:4284-4296`).
- Gap: G5. Applied in code, decided in no ADR.

### F13. A refused or unknown delegation-notice send ends the assignee's worker by an "existing fail-closed rule" that no record states

- Records: ADR-180:131-134 names "the existing fail-closed rule"; the rule it
  refers to is ADR-036:293-294 (a failed attempt ends the worker), which
  ADR-036 itself calls undecided. ADR-011:79-81 (retained) makes a failed
  thread send a visible task-binding failure with inputs kept.
- Code: `driver.rs:826-838` maps every final-send outcome other than
  `Delivered` to `Failure::OutcomeUnknown`, which `run_continuous` returns
  (`:333`).
- Gap: G1.

### F14. Provider refusal is recorded as `protocol`; the provider's own words are dropped

- Records: ADR-036:263-266 (error and failed `turn/completed` "still end the
  turn as Failed"), `:293-299` (retained records the provider's text; "not
  decided here"); ADR-181 §2-3 (Proposed) adds `terminal_reason` and the
  uncollapsed failure.
- Code: `report.protocol != Completed` → `Failure::Protocol`
  (`operation.rs:1652-1653`); the store keeps one word
  (`R/hagency-store/src/domain/owned_dispatch.rs:587-594`).
- Retained: `terminal_reason` = reason + exit identity + last 500 chars of
  stderr (`T/router/src/runner.ts:483-484`).
- Gap: G6 (evidence, being built) and G1 (verdict, undecided).

### F15. Two more agent-enders decided only as "unchanged": handoff refusal and unsettled file delivery

- Records: ADR-175:33-37 (a refusal before an Operation exists keeps "Worker
  retention … unchanged"); ADR-117:29-31 (an unsettled delivery at completion
  is `SettlementUnknown` → `outcome_unknown`), which via ADR-036 ends the
  worker.
- Code: `driver.rs:647-664` returns `Err(Failure::Worker)`;
  `operation.rs:1598-1602` maps the store's `State` refusal to
  `SettlementUnknown`.
- Retained: not determined for file delivery; a launch refusal requeues or
  cancels a still-leased dispatch (`T/backend-v2.js:2735-2749`).
- Gap: G1.

### F16. Restart reconciliation is parity, but only the store side; the persisted agent fence the retained product carries across restart has no native counterpart

- Records: ADR-011:212-215, ADR-148:55-59, ADR-162:85-93 (notice after
  restart), ADR-147:588-672 (re-attach). ADR-019:113-116 (TS agent fence).
- Code: `recover_all` → `lose` (leased → queued; started/parked →
  `outcome_unknown`, quarantine, dirty, notice, `lost` event)
  (`R/hagency-store/src/domain/execution.rs:217-283`) mirrors
  `reconcileOnStart` (`T/router/src/store.ts:3826-3858`). Re-attach consults
  no cleanup evidence (`fleet.rs:337-383`); the retained product persists
  `stopUnconfirmedDispatches` on the agent record (`T/backend-v2.js:2728,
  12645-12646`) and refuses Start while it stands (`:1940, 13803`).
- Gap: G2 (the fence the reversal of ADR-096 must move to).

### F17. Every store shutdown verdict is final and unretried (ADR-075/082/106/120), while the process is required to stay up on it (ADR-096)

- Records: "Do not retry shutdown" (ADR-075:35-36), "An already timed-out
  caller never becomes successful after late cleanup" (ADR-082:44),
  "Later successful cleanup must never replace the original timeout result"
  (ADR-106:63-64), ADR-120:37-38; ADR-096:208-212 ("Bootstrap retains the
  original wrappers and reports that uncertainty … There is no automatic
  replacement claim or shutdown retry").
- Code: `bootstrap.rs:1440-1453` (domain/store shutdown → `OutcomeUnknown`),
  `:1571-1580`, `main.rs:299-302`.
- Retained: `process.exit(0)` in `finally` (`T/backend-v2.js:17412`); WAL
  recovery on the next open (ADR-120:46-50 already makes a clean close and a
  crash leave the same on-disk shape).
- Gap: G2. Consistent records; the composition (final verdict + no exit) is
  what leaves a stuck process, and ADR-120 already removed the reason for
  it (ADR-120:46-48: "A clean close and a crash now leave the same on-disk
  shape"). ADR-127:41-43 says the same in its own words — the parked process
  "is SIGKILLed holding an unrelinquished owner … (ADR-120 leaves the WAL for
  the next open)" — which is an argument for exiting, not for parking.

### F18. ADR-181 is Proposed as verdict-neutral, so it would codify F3, F4, F11 as the evidence baseline

- `K/adr-181:83-84`, `:71-84` (`LostAuthority { site, cause }` with `busy`,
  `timed_out`, `unavailable` as causes of a *fatal* verdict). Right for the
  evidence slice; the amendment list in Part 5 must change the verdict for
  those causes in the same records ADR-181 cites, or ADR-181's own
  "Consequences" should say the verdict is owed.

## Part 3. The retained product's rules, one sentence each

- A runner failure or non-zero exit after `started` settles the **dispatch**
  `outcome_unknown` with `terminal_reason` = reason + exit identity + 500
  chars of stderr (`T/router/src/runner.ts:481-490`).
- Settlement revokes the capability, deletes the leases, marks the workspace
  dirty with a generation, blocks the task with a waiting reason, and queues
  "Result uncertain …" in the thread (`T/router/src/store.ts:2623-2660`).
- A later request into that session is answered "Waiting: a previous runner in
  this session stopped …" and no dispatch is minted (`T/router/src/store.ts:1525-1532`).
- The agent keeps claiming; the pump handles a launch failure by requeue or
  cancel only while the dispatch is still `leased`, and otherwise logs
  (`T/backend-v2.js:2735-2756`).
- An unconfirmed cleanup marks the **agent** `manualDown` with
  `offlineReason: 'runner-cleanup-unconfirmed'` and a persisted
  `stopUnconfirmedDispatches` list; a later confirmed close removes the entry
  (`T/backend-v2.js:2717-2733`; `T/router/src/runner.ts:393-399`).
- The cleanup receipt is bounded at 8 s; the guardian exits 125 with the
  reason on stderr after its own 5 s deadline (`T/router/src/runner.ts:373`;
  `T/router/src/runner-guardian.ts:139-144`).
- A park refusal for a Codex command/file/permission request is answered with
  `decline` and the turn continues; an MCP-elicitation park refusal throws and
  settles the dispatch (`T/router/src/runner.ts:660-664`).
- The parked runner's lease is the runner lease plus the approval TTL
  (`T/backend-v2.js:2703`); the runner lease is 20 min (`:246`).
- Liveness is a 120 s heartbeat TTL with 30 s grace
  (`T/lib/supervisor-lifecycle-manager.js:14`, `T/lib/agent-state.js:14`).
- On restart, `leased` returns to `queued` with leases deleted;
  `started`/`parked` settle unknown with the same notice
  (`T/router/src/store.ts:3826-3858`).
- Shutdown aborts every runner, awaits their settlement, saves, and always
  `process.exit(0)` (`T/backend-v2.js:17384-17416, 17622-17624`).
- Appservice sync retries forever with exponential backoff and a held cursor;
  nothing is fenced (`T/lib/appservice-sync.js:463-522`).
- An agent whose account setup fails is skipped with a warning; the others
  start (`T/bridge-matrix.js:4284-4296`).
- Orphan recovery is an operator act: inspect, then continue / accept /
  keep blocked (`T/router/src/store.ts:2880-2895, 3000-3068`; ADR-148:100-114).
- The product's own earlier records say the same at agent scope: a fleet
  ceiling "refuse[s] new auto-joins … and NEVER stop[s] a running agent"
  (ADR-016:458); "A dead credential is a human-visible state, not a retry
  loop" (ADR-014:389); a failed card delivery denies that request
  (ADR-016:739), which is ADR-137's rule.

## Part 4. One failure model for the port

Rule of the table: the retained product's rule wherever it has one; a stricter
port rule only where named, with the reason. "Scope" is the widest thing that
changes state. "Exit" is whether the process may complete a shutdown while the
fact stands.

| Fact | Scope | Recorded | Resolver | Exit | Retained rule / port exception |
|---|---|---|---|---|---|
| A turn failed (`error`, failed `turn/completed`) | dispatch `outcome_unknown`; session quarantined; task blocked; workspace dirty if it could write | `terminal_reason` = provider's text + exit identity + stderr tail (ADR-181 §2); thread notice | operator: continue / accept / keep blocked (ADR-165) | yes | retained (`store.ts:2623-2660`, `runner.ts:481-490`). No exception. |
| A provider refused (usage limit, auth) | same as above | provider's words as the reason | operator | yes | retained. No exception; the port's `protocol` word is a loss of evidence, not a stricter rule. |
| Outcome unknown (EOF, transport, deadline, cancel after start, lost start receipt) | same as above | uncollapsed failure (ADR-181 §3) | operator | yes | retained (ADR-011:195-201). No exception. |
| Cleanup unproven (guardian refusal, census error, 2 s stop timeout) | dispatch as above **plus** the agent fenced: no new claim until the fence clears; persisted | `stop_reported` with refusal, live rows, guardian exit and stderr (ADR-181 §5); a durable `agent_fences` row (name owed) | later evidence (a confirmed later census / stop) or operator; the retained product clears it from the same close channel | yes; the in-memory owner is dropped after the bounded receipt, the fence stands in the store | retained (ADR-019:113-116, `backend-v2.js:2717-2733`). Port keeps one stricter rule on purpose: the two-census proof (ADR-029:180-182) stays, because the port's tracker is proof-based and "never stricter than proof allows" (ADR-029:400-403). |
| Authority refused: `revoked`, `generation`, `quarantined`, `state`, `not_found` | dispatch fenced (as unknown); tree stopped | `LostAuthority { site, cause }` | operator | yes | retained: a router refusal throws and settles the dispatch (`runner.ts:498`). No exception. |
| Authority check `busy`, `unavailable`, `timed_out`, `io`, `locked` | none: retry on the next tick within the lease; the store's own expiry settles a lease it cannot renew | counted; a WARN per occurrence | automatic | yes | retained: no in-run host check; the store expires (`store.ts:3826-3858`, natively `execution.rs:284-290`). Port keeps a coarser renewal (5 s, ADR-161) rather than none — stricter on purpose: a revocation must reach a running tree inside its own budget. |
| Approval refused by the store (request or response) | that request declined; turn continues | the store's word as `denial_reason` (ADR-137 shape) | automatic | yes | retained (`runner.ts:660-664`). No exception; an unknown deny *write* is `SettlementUnknown` for the dispatch, never `LostAuthority`. |
| Approval refused by the adapter | declined; turn continues | as ADR-046:421-434 | automatic | yes | retained. |
| Approval unanswered at owner expiry | declined; turn continues | ADR-046:456-468 | automatic | yes | retained; the port's independent approval clock (ADR-149) stays. |
| Matrix connect / read failed | that agent's attempt ends without work; redial (ADR-174) then exponential backoff with the cursor held; **no fence**; other agents untouched | WARN with the transport word; status `retrying` with the backoff | automatic | yes | retained (`appservice-sync.js:463-522`, `bridge-matrix.js:4284-4296`). Port exception kept on purpose: genuine negative **evidence** (wrong device, unsafe room, revoked engagement, host invalidation) still fences (ADR-047:261-264), because the port's rooms are encrypted DMs and a wrong identity must not keep a key. |
| Matrix write outcome unknown | that send stays uncertain; the dispatch settles per its custody | journal | operator (ADR-047, 059) | yes | port rule, kept: the retained product has no encrypted-send custody to compare. |
| `Generation` from another agent's room change | re-resolve the plan once (ADR-178); if still absent, that agent waits for the next observation; **no transport fence** | WARN | automatic | yes | retained (no shared fence). Port exception: a changed *membership* still retires the old room scope (ADR-153), because a member who left must not read on. |
| Guardian observation failed (census error, tracker gap) during a run | dispatch: cleanup unknown at stop (row 4); the run is not stopped for an observation error alone | `stop_detail` (ADR-029:307-315) | as row 4 | yes | retained: no in-run census. Port stricter on purpose (see row 4). |
| Guardian observation failed while idle (warm) | that warm owner is retired and re-warmed on the next dispatch; never `LostAuthority` | WARN | automatic | yes | retained: no warm runtime to compare; the rule follows ADR-029's "a process nobody can place is not owned" and ADR-047's "a cancelled read is not negative evidence" applied to the host's own timeout. |
| Restart found in-flight work | `leased` → `queued`; `started`/`parked` → `outcome_unknown` + notice; agents re-attach; agent fences (row 4) survive | `lost` event with writer `restart` (ADR-181 §6) | operator | — | retained (`store.ts:3826-3858`; ADR-147 amendment). No exception. |
| An admission failed (fresh or re-attach) | that agent `not_attached` with the cause; the fleet runs; readiness reports the agent | status row | operator | yes | retained (`bridge-matrix.js:4284-4296`). No exception. |
| The approval pump failed (send, intake) | that request denied with the reason (ADR-137); the pump continues with the next request and backs off; a claim never fails for it | `denial_reason`; WARN | automatic | yes | port rule (no retained single pump); ADR-137's own rule extended from the send to the intake. |
| Readiness | `/ready` 503 only when the service cannot serve: a writer closed, the fleet loop dead, the pump task dead; a failed or fenced agent is body detail | component list | — | — | operator choice; ADR-096 brief-21 words kept. |
| Shutdown (SIGTERM) | stop admission, cancel every attempt, wait the bounded cleanup receipt per agent (8 s retained), record row 4 for any unproven tree, close the stores, **exit** | status `closed` or `closed_with_fences` | — | always | retained (`backend-v2.js:17384-17416`; ADR-011:207-208). Reverses ADR-096:160-166, 199-212. |

What the table deliberately does not change: the store's session quarantine and
dirty workspace (ADR-053:182-184), operator-only recovery (ADR-148), the
completion proof (ADR-060:100-101; publication still waits for a proven stop —
a held row stays held under an agent fence, and the operator's `accept_completed`
is the retained way out), and the no-shape-refusal approval rule (ADR-046).

## Part 5. Amendments needed, per record, and the gap each closes

| ADR | Amendment or supersession | Closes |
|---|---|---|
| ADR-036 | Supersede `:293-299`: a failed turn ends the dispatch as `outcome_unknown` with the provider's text as the reason, quarantines the session, and the agent's worker continues to its next claim; delete "which ends the agent's worker". | G1 |
| ADR-096 | Supersede `:160-166`, `:199-212`: shutdown waits one bounded cleanup receipt per attempt, records an agent fence for any unproven tree, closes the writers and exits; delete "shutdown failure cannot consume the last owner", "Normal SIGTERM then deliberately retains …", "No macOS unknown cleanup is called a complete shutdown". Amend `:241-242`/`:262-265`: a failed or fenced agent is body detail, not 503. | G2, G5 |
| ADR-053 | Amend `:82`: `Busy`, `Unavailable`, `OutcomeUnknown`, `Io`, `Locked` from a renewal are not scopes and stop nothing; amend `:106-107` to say "retried on the next tick, settled by the store's expiry"; amend `:113-114` and `:128-129` to say the stop receipt is read once (F9) and the owner is dropped after it, with the fence persisted. | G3, G2 |
| ADR-060 | Amend `:140-141`: macOS whole-tree uncertainty still gates publication, but the held row waits under an agent fence, not under a retained process owner. | G2 |
| ADR-040 | Amend `:109-110`, `:115-119`: a `Cleanup::Unknown` is inspected, not retried; the guardian's one report is the receipt. | G2 |
| ADR-029 | Add: the host's idle qualification is bounded by the warm idle budget, not `response_ms`, runs on a coarse tick, and a late or failed reply retires the warm owner without a verdict; the sticky latch (`:406-408`) applies to the guardian's own census, not to the host's timeout. | G3 |
| ADR-113 / ADR-161 | ADR-161 `:32-34`: renewal every 5 s on a 5 s lease is unchanged; add that a renewal refusal other than revocation is retried. ADR-161 `:39-40`: name the divergence from the retained lease + approval TTL (`backend-v2.js:2703`) and decide it (the review's "independent approval clock"). | G3, G4 |
| ADR-046 | Amend `:488-489`: an unknown expiry-deny write is `SettlementUnknown` for the dispatch, never `LostAuthority`; add: a store refusal of the *request* (`request_owner_approval`) is answered with the family's decline and recorded with the store's word, as `:430-434` already does for the adapter. | G4 |
| ADR-064 | Supersede `:220-221`: an intake refusal or unknown outcome denies or retains that request and the pump continues; add the per-request scope. | G4, G5 |
| ADR-137 | Extend `:26-33` from the send to the intake and to the pump's own errors. | G4 |
| ADR-047 | Amend `:92-93`, `:102-104`: a failed authenticated *read* (connect, timeout, 429 exhausted, `Busy` from the store) is retried with backoff and the cursor held; only negative evidence (`:261-264`) fences. Amend `:95-97`: a failed room read of one agent retires no other agent's route. Keep `:285-286`. | G5 |
| ADR-174 | Amend `:20`, `:68-69`, `:95-96`: after the four attempts the worker backs off and retries; it does not fence and does not end. | G5 |
| ADR-153 | Amend `:40-41`: a lost reply on the project refresh is retried, not fenced; a *changed* membership still retires (kept). | G5 |
| ADR-178 | Amend `:77-78`: a session the fresh resolution still names is refused work this poll, not a worker failure. | G1 |
| ADR-180 | Supersede `:131-134`: a refused or unknown notice send ends that attempt and leaves the notice journaled; the assignee's worker continues. | G1 |
| ADR-175 | Amend `:33-37`: a handoff refusal before an Operation is that attempt's failure with its category; the worker continues. | G1 |
| ADR-117 | Amend `:29-31`: the driver reports `outcome_unknown` for the dispatch; add "and continues". | G1 |
| ADR-162 / ADR-169 | Amend ADR-169 `:16-17` and ADR-162 `:74-76`: every failed attempt leaves the worker up; `awaiting_operator` names a quarantined session whether or not the tree is proven (an unproven tree additionally carries the agent fence of row 4); drop "does not take in new requests" (`:81-82`) in favour of the "Waiting" answer already ported (`:98-101`). | G1, G2 |
| ADR-147 | Extend `:636-638` from re-attach to fresh admission; add that re-attach reads the persisted agent fence and honours it. | G5, G2 |
| ADR-130 | Amend `:111-113`: automatic stop settlement stays owed; an unproven stop is a persisted agent fence, not a retained process. | G2 |
| ADR-127 / ADR-133 / ADR-135 | Amend ADR-127 `:17-20`, `:41-43` and ADR-135 `:111-113`: an unknown close records its fences and exits inside `TimeoutStopSec`; delete "parks … rather than exiting 0" and "a park is not a failure". Amend ADR-127 `:45-48` / ADR-133 `:66-68` only if the readiness rule (row "Readiness") changes what `/ready` means at a start gate. | G2, G5 |
| ADR-101 / ADR-098 / ADR-089 | Amend ADR-101 `:240-244`, `:306`; ADR-098 `:25`, `:34`; ADR-089 `:88-89`: an unknown file job is recorded as unknown (its rows already are, ADR-097:78-79) and the close proceeds; the in-memory owner is not what keeps the row's custody. | G2 |
| ADR-104 | Leave `:58-59` (the Windows parked worker) as the one named exception, or supersede it with the same rule; the record already calls it "exceptional" and Windows is paused (ADR-136). | G2 |
| ADR-124 / ADR-125 | No change; cite `124:154-156` and `125:95` as the port's own precedent for "a writer refusal is a next-tick retry". | G3 |
| ADR-019 / ADR-011 (TS) | No amendment; cite them as the parity source for the agent fence and the exiting shutdown. | — |
| ADR-181 | Amend "Consequences": the verdicts for `busy`/`unavailable`/`timed_out`/`io` causes and for `approval_request` are owed to the amendments above; the evidence shape is unchanged. Add the `agent_fences` row to §1 or a sibling record. | G6 (kept honest) |
| ADR-065, ADR-091, ADR-095, ADR-148, ADR-163, ADR-164, ADR-165, ADR-170 | No change: they already state the session/dispatch scope and operator resolution the model keeps. | — |

Order that matches the operator's approved closing order: ADR-181 note (1),
then ADR-036/178/180/175/117/147 and ADR-096's readiness sentence (2), then
ADR-096/053/060/040/130/162/169 (3), then ADR-029/053/113/161 (4), then
ADR-046/064/137/161 (5), then ADR-047/174/153 (6).

## Could not determine

- Whether a second SIGTERM after the one-shot handler
  (`R/hagency/src/main.rs:285-296`) is swallowed or terminates the parked
  process; needs a run, and running was out of scope.
- The retained product's card-delivery failure scope (whether a failed
  private card send affects more than that request); no single approval pump
  exists there to compare with `R/hagency/src/bootstrap/approval.rs`.
- Whether the retained product has any in-run path that aborts a healthy
  runner on a store refusal or observation timeout; none found in
  `runner.ts`, `store.ts`, `backend-v2.js` at the cited ranges, but the search
  was by symbol, not exhaustive.
- The retained product's rule for a dispatch completing while a file upload is
  still in flight (F15); not searched beyond `runner.ts`.
- The value of the retained `APPROVAL_TTL_MS`; only its use in the lease
  (`T/backend-v2.js:2703`) was read.
- Whether every unstarred quote in tables 1b-i..iii is verbatim: the three
  extraction passes copied them from the files and I verified 50 of them by
  `grep -F` (all matched; the starred ones); the rest I did not check, and the
  passes kept at most two quotes per record, so a record in 1b may carry a
  rule the table does not show.
- Whether the retained product's `stopUnconfirmedDispatches` fence survives
  its own restart reconciliation in every path (`T/backend-v2.js:2728` is
  persisted through `saveAgents`, and `:1940`, `:13803` read it; the
  orphan-home reconciliation at `:3049-3095` was not read here).
- Line numbers in `R/` after the ADR-181 slice lands (the tree read here has
  ~70 uncommitted files).
