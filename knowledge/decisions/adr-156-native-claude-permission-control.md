---
kind: decision
id: ADR-156
title: One-shot native Claude permission control without new verdict authority
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION, REQ-EXECUTION-AUTHORIZATION, REQ-OWNER-UI-APPROVAL]
---

## Decision

The native noninteractive Claude profile uses the official Agent SDK control
channel already framed by ADR154/155. A can_use_tool request has an upstream
request ID and original tool input. Its success response contains behavior allow
with that same updatedInput, or behavior deny with a fixed message and interrupt
true. No updatedPermissions or caller replacement input is allowed. The retained
tmux/channel adapter remains unchanged. Neither transport is owner authority.

Add an explicit runtime control opt-in once system/init established the owned
stream's session. This opt-in grants no response authority; the future execution
Host must bind its original durable approval context and consume/recheck the
exact grant before calling the send primitive. Keep production Claude refusal
until that integration, scoped task MCP, usage and account gates are complete.

Retain at most16 callback identities and64KiB compact input per callback. Store
the original input privately before returning the untrusted observation to the
host. Preparing allow or deny consumes that retained input exactly once into a
non-Clone, non-Deserialize, source-bound frame. No caller can replace its bytes.
Cancellation tombstones and sent records stay until session close; IDs cannot be
reused. An unsupported or excessive request refuses, never truncates into an allow.

Each request has an original receive timestamp, not the time a delayed host
eventually consumes it. Derive fixed owner and response cutoffs from that time,
within the existing session lifetime. Response reserve must cover the bounded
write timeout. Starting a prepared send fixes its write deadline; message/control
returns never renew any clock. Hold the original pinned host future separately
from the mutable session, and poll only cancellation-safe leaf IO alongside it.
Returning a message keeps that same future alive; its output is consumed once.

Before the first response byte, parse currently available stdout, including any
partial frame, and return its messages to the host. During a partial write retain
the exact buffer and offset; return new messages before continuing that buffer.
The caller must process barriers and recheck its original durable grant. A
cancelled request or ResultObserved cannot resume a response. Already accepted
bytes are not recalled; a closed partial write remains explicit uncertainty,
never application or retry permission. Ordinary read/control calls cannot drive
a suspended writer. A flushed receipt proves only AsyncWrite acceptance.

Started operation drop/error permanently closes the stream, with byte progress,
and OwnedClaudeSession asks its original guardian to stop. ResultObserved still
does not auto-stop a potentially live process. macOS whole-tree cleanup remains
unproven; this work introduces no containment claim or production enablement.

## Upstream sources and qualification limits

Inspected official sources on2026-09-16:
- https://code.claude.com/docs/en/agent-sdk/user-input
- https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/_internal/query.py

The SDK cancels abandoned requests without writing a reply. Auto-approved tools
do not necessarily reach canUseTool; merely enabling a callback is not proof of
owner gating. Effective native launch must also retain auto mode and protected
operation ask rules, and verify actual installed behavior. Fixture byte exchanges
prove runtime mechanics only, not an owner click, effective sandbox, model task,
authenticated usage, real Matrix delivery or local three-runner soak.

## Installed initialization compatibility

The first native operator deny-only diagnostic failed during initialize, before
any prompt. Independent no-prompt shape inspection of the same installed CLI
found two extra success-response fields: pending_permission_requests and
pending_user_dialog_requests. Accept each only as an optional empty array.
Nonempty queues remain unsupported and malformed values refuse; a fresh session
must not inherit preexisting approval/dialog work. Root fields, exact request ID,
success/error exclusivity and unique JSON keys remain strict. Retain the original
failed diagnostic rather than counting the codec fixture pass as live success.

The second operator diagnostic initialized successfully but returned an error
result before any callback. Provider-owned status commands isolated the login
context difference without reading credentials: HOME/PATH alone reports logged
out; adding USER reports the existing claude.ai login, while adding LOGNAME,
SHELL, TMPDIR or CoreFoundation metadata separately does not. Forward USER only
as operator OS identity metadata in this diagnostic. This does not establish a
managed account binding or authorize importing provider keys/tokens.

The third diagnostic reached one actual Claude permission request and flushed
one deny through the native owned driver, with zero allow frames. It then
observed an error result. Preserve that negative result and diagnostic exit1:
the callback transport was exercised, but no successful model task, native Host,
owner Matrix approval or soak is asserted. Leader exit/signals were observed;
macOS whole-tree cleanup remains false. The original failed records and private
workspaces remain retained, not replayed as domain dispatches or cleaned by a
fabricated task result. The installed CLI was rechecked as2.1.270 and hashed.
