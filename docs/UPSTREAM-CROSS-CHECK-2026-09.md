# Upstream cross-check — 2026-09

> From [shisuiki/agent-chat](https://github.com/shisuiki/agent-chat) (upstream author).
> Method: line-level verification of upstream's own audit findings against this
> repository's `master` (as of 2026-09-10, `5dbef22`). Result: **7 solved, 6 open**.
> Each open finding is a question, not a demand — I'd like to hear your thinking.

## Solved in Hagency (verified)

| # | Upstream finding | Evidence at master |
|---|---|---|
| 1 | Dual delivery implementations (`server.js` vs push-relay) | `server.js` removed; queue inlined as `lib/delivery-queue.js` (#48cebe6) |
| 2 | Anonymous group-cursor advancement [Critical] | `authorizeAgentCredential` (backend-v2.js:1398) |
| 3 | Public message-detail reads [Critical] | `authorizeMessageDetailAccess` (:4468, consumed :16903) |
| 4 | tmux operational coupling | `lib/runtime/` seam — tmux/conpty/headless/acp (b34238e) |
| 5 | Matrix identity vs agent instance | `lib/matrix-representative.js` (b108160, ADR-016) |
| 6 | Unenforced architecture boundaries | `scripts/architecture-boundaries.json` + CI gate |
| 7 | Dependency-advisory debt | advisory ratchet (`security/audit-baseline.json`), ongoing repairs |

## Open findings

### F1 · Agent-token auth still fails open by default [upstream Critical]

`lib/backend/auth-adapter.js:49` — `HAGENCY_AGENT_TOKEN_MODE` defaults to `'audit'`
(log-only); mode values `audit | soft | hard`, nothing selects `hard` at install time.

**Question:** any plan to flip the default to fail-closed (or make `hard` the default
for fresh deployments)? Upstream's audit rated this Critical; I'd like both projects
to converge on fail-closed.

### F2 · Agent identity / state machine remains tmux-native

`lib/agent-state.js` — transition events are still `tmux_detected` / `tmux_missing` /
`api_register_with_tmux` (:19–58), and `!agent.tmux && !agent.online → offline` (:148).
The runtime seam decoupled tmux the *executor*; tmux the *identity source* remains.

Since `conpty` / `headless` / `acp` runtimes already exist in the seam, presence could
be a fact **reported by the runtime**, with the state machine defined over runtime-agnostic
events (attached/detached/heartbeat), per-runtime adapters mapping their own signals.

**Question:** is runtime-agnostic presence on your roadmap? Upstream is prototyping
exactly this in a separate kernel (identity registry / addressing / message truth /
delivery queue / transport sidecar SPI) — your field data on the seam approach would
be very valuable either way.

### F3 · Kernel-facing JSON stores lack schema versioning

`backend-v2.js` (17,720 lines at HEAD) contains no `schemaVersion`/migration markers;
the SQLite migration chain 001–008 lives only in `router/src/store.ts`. `agents.json`,
`messages.json`, `cursors.json` still normalize ad hoc.

**Question:** appetite for extending the router's migration discipline to the
kernel-facing stores?

### F4 · `remote/` hand-mirrors `lib/push-relay-core.js`

Both `remote/push-relay.js` and `remote/lib/push-relay-core.js` persist as manual
copies (upstream salt/08 flagged the drift risk).

**Question:** would you accept CI-generating `remote/` from `lib/` (upstream already
ships `scripts/build-remote-package.sh`) instead of hand-mirroring?

### F5 · `backend-v2.js` monolith growth vs upstream's kernel split

15,939 → 17,720 lines at HEAD. I read this as a deliberate trade — boundary
enforcement + seams instead of file splits. Upstream is taking the opposite bet:
a physically separate, dependency-zero communication kernel.

**Question:** if that kernel spec stabilizes, would Hagency consider consuming it,
or does inline feel strictly better at fleet scale? Genuinely asking — your
operational experience is data I don't have.

### F6 · Delivery topology divergence undocumented

`salt/19` designs a relay claim-API; #48cebe6 actually shipped in-process inlining
("SLICED, NOT REWRITTEN"), and `docs/salt/` was not updated to the real topology
(README still lists `server.js`).

**Question:** would you take a docs PR updating salt/19 (or adding a short
"actual delivery architecture" note)? And which route would you recommend for
multi-host delivery at scale — claim-API vs inline+sink?

## Context

Upstream is splitting the communication kernel into a standalone project
(agent-chat-lite). Hagency is the largest body of production experience on this
codebase; I'd rather align early than fork concepts. Thanks for treating this
codebase with so much care — the honest debt docs and rejected-proposal records
are genuinely rare.
