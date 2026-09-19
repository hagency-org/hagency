---
kind: decision
id: ADR-003
title: "Use public status plus private UI-only approval"
status: Accepted
liveness: auto
tags: [matrix, approval, privacy, ui]
---

## Context

Execution approvals may contain commands, paths, issue content, or other
project-sensitive details. A public project room is useful for coordination but
must not become the approval authority or disclose the detailed request.

## Decision

Every remote execution approval uses two Matrix surfaces. The public project
room receives only a redacted, non-actionable status notice. The agent-owner
encrypted DM receives the full structured request and UI buttons for
single-use approve or deny actions. Button clicks emit structured Matrix events;
hagency alone validates and consumes them. Plain text and generic `!ctl`
commands never authorize an execution request.

### Amendment 2026-08-11 — the encrypted DM has one authorised exception, named here

"The agent-owner **encrypted** DM" above reads as admitting no exception, and the
implementation has one: with `HAGENCY_APPROVAL_DM_MODE=plaintext-test` **and**
`HAGENCY_ALLOW_PLAINTEXT_APPROVAL_TEST=1` **and** `NODE_ENV !== 'production'`
(`resolveApprovalDmMode`, `bridge-matrix.js`), the full structured request — input preview
included — is sent to a deliberately unencrypted diagnostic room.

The exception is authorised. It is not recorded here, which is the defect: the authorisation
lives in **ADR-006's Alternatives** section and in `specs/task-owner-ui-approval.spec.md:56-58`,
and this ADR neither states it nor cross-references them. Read alone — which is how a decision
record is read — ADR-003 overstates its own guarantee.

Recorded rather than removed, because the carve-out is genuinely useful: E2EE failures are
otherwise undiagnosable from outside the crypto layer. What makes it safe is that it requires
three independent settings and refuses in production, which the
`plaintext approval diagnostics require explicit non-production opt-in` test in
`tests/bridge-matrix-approval.test.js` asserts. What makes it honest is saying so in the
document that promises encryption.

## Consequences

Good, because project participants can see progress without receiving private
details or approval power.

Bad, because the workflow requires coordinated protocol support in hagency
and Robrix2, plus a healthy encrypted DM channel.

## Alternatives Considered

- Approve by typing text in DM: rejected because free-form text is ambiguous and replayable.
- Put approval buttons in the public room: rejected because visibility would imply an unsafe control surface.
- Let Robrix2 decide authorization locally: rejected because clients are presentation surfaces, not server authority.

### Amendment 2026-09-13 — the public status notice is named, and what it is not

The native approval path adds a **public status notice**: a one-way status
word posted to the project room when an approval is pending — `agent`,
`project`, `waiting_for_owner`, a short body — and nothing else (ADR-137;
the private-card plan v6's PC-C1). Naming it here closes the same gap the
2026-08-11 amendment closed for the test room: read alone, "the encrypted DM
is the approval channel" would misdescribe a second artefact leaving the
approval path. What this notice is **not**: it is not the request, it is not
actionable, and it carries none of the request's material — no input
preview, no tool name, no scope, no request id. Its validator
(`PublicFrozen`) enforces content-freedom by construction, and reading it
confers no grant and no authority. The encrypted DM remains the only channel
that carries request content; the notice is a lamp, not a channel.
