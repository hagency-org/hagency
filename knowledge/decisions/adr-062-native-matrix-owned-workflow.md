---
kind: decision
id: ADR-062
title: "Qualify one authenticated Matrix to native completion workflow offline"
status: Accepted
---

## Context

The native components need an actual offline workflow that joins authenticated Matrix intake, owned execution, MCP completion and final send custody.

## Decision

### Decision and scope

Add an integration test in the native `hagency` binary package. It consumes only
public `hagency-matrix`, `hagency-store` and `hagency-execution` APIs. The test
driver performs scheduling explicitly; there is no autonomous scheduler, service
availability change, domain schema change or production trust/cleanup shortcut.
The binary package gains only test dependencies on the existing Matrix crate and
pinned Tokio Rustls. Fixture support stays under `tests/` and reuses the Matrix
test suite's bounded local TLS server and synthetic public test certificate.

This joins ADR047/054 authenticated intake, ADR059 notice/final send custody,
ADR053 retained owned execution, ADR057 generated native MCP configuration and
ADR060 explicit Done plus held completion. It does not replace their component
contracts, real platform qualification or the migration plan's remaining gates.

### Actual path

1. Create a fresh registration, resource allocation and provisioned engagement
   through the native domain admission/effect methods. Provisioning is synthetic
   fixture setup, not a real Palpo enrollment result. Fix one host identity and
   private SDK store; verify its full MXID and device by real local HTTPS whoami,
   SDK sync and full room state before resolving the root session.
2. Receive the human's original `$question` with its complete authenticated MXID
   and exact `m.mentions` through the public Collector intake. Read its actual
   immutable inbox projection and create a verified task intent with a host
   definition and scoped request key. Never inject an admitted event directly or
   fabricate a Matrix trust/delivery observation.
3. Require both normal dispatch enqueue paths to refuse the pending task. Send
   the existing notice claim over real TLS, inspect Sending while its HTTP
   response is held, and confirm the task remains inactive. The synthetic server
   returns the actual observed `$notice` receipt. Only then do inputs activate.
   `$question` remains the task's original thread root; `$notice` is separately
   stored as activation evidence. Replaying the same delivered notice causes no
   second HTTP write.
4. Enqueue the real active inbox sequence using `enqueue_inbox_dispatch`. Verify
   the exact frozen body, sender, original event, session and task in the owned
   dispatch scope. The test driver selects one host-owned fixed Unicode directory
   and resource ID, then claims the dispatch through the normal domain API.
5. Start the existing Operation with the actual native offline app-server fixture
   and actual `hagency mcp` executable. Their owned pipes and guardian/Job custody
   are production paths. The helper uses the generated names-only private
   configuration to call the same fresh writer over authenticated loopback HTTP.
   It explicitly calls `complete_task_with_reply` with the assigned task and full
   body. No model text implicitly marks Done. Wire assertions retain on-request,
   workspace-write and default disabled network settings; they do not prove the
   fake peer enforces a real Codex sandbox.
6. Preserve the canonical Done/epoch independently of missing terminal Codex
   output. The fixture intentionally supplies no successful turn completion after
   its explicit finish. Only the Operation's retained actual owner may admit the
   held final after leader exit, accepted stop and whole-tree cleanup. No caller
   constructs or submits a StopReport. The retired runner capability cannot read
   or mutate the task again.
7. On a qualified platform, claim the existing final, preview its original route
   and send through the real Collector. Assert Sending precedes actual acceptance,
   original body/Markdown/thread/transaction survive unchanged, no capability is
   projected, and the actual accepted event settles Delivered. Exact replay uses
   the journaled receipt and performs no second HTTP write.

All SQL in the new harness is read-only assertion code. It neither constructs
canonical state nor bypasses the single domain writer. The ordinary API custody
repository and canonical domain repository share the same fresh directory but
retain their existing separate database ownership. The test closes the actual
server, repository and SDK owners after the retained Operation finishes.

### Refusal evidence and bounds

- macOS still cannot prove whole-tree cleanup for this ordinary POSIX guardian
  path. Its real subprocess test requires leader exit, explicit CleanupUnknown,
  canonical Done, retained workspace lease and no final claim/HTTP PUT. Passing
  this refusal is not positive end-to-end final-delivery qualification.
- A real notice HTTP403 or dropped response remains conservatively uncertain.
  Neither activates inputs, creates a dispatch, launches a child nor permits a
  final. Retrying the uncertain notice cannot silently write again.
- An encrypted private room rejects a plaintext event even if its JSON includes
  forged verification metadata. No task, child or final is created. ADR065 amends
  this fixture to require a durable terminal rejection and healthy transport after
  actual bad-to-good encrypted continuation tests. The original ADR062 version
  retired the entire transport; its earlier local evidence remains historical.
  This check alone remains privacy refusal evidence. ADR059 separately exercises
  actual verified
  encrypted DM/group sending and SDK decryption; this test does not claim a full
  encrypted DM plus native helper workflow. That later workflow requires a real
  test-only SDK crypto setup, never a production `verified=true` setter.

The shared TLS fixture bounds connections, request count/size, body reads and
shutdown; the Collector uses its existing finite SDK and HTTP budgets. Execution
uses the original 30-second operation and two-second response limits, existing
owner stop/join bounds and fixture watchdog. No delays extend production timeouts
or defer revocation to force helper acknowledgement. Helper ACK/exit may race the
real completion fence; canonical writer receipt and actual retained cleanup stay
the independent evidence.

### Remaining qualification

Local macOS execution cannot qualify Linux/Windows final publication. The same
positive branches must run in actual native CI, and that evidence must be reported
separately. This slice uses no live models, accounts, endpoints or deployment
state. It does not qualify real-model behavior, sandbox efficacy, physical
workspace/ancestor custody, simultaneous POSIX custodian loss, approval application,
live crypto enrollment, automatic unknown-send recovery or native cutover.

The pinned Codex configuration/protocol semantics remain those verified in
ADR036/057 against Codex0.153.4 source3d2ee51ca2d5db578f328aa75e20aa22c0197c9a.
This integration adds no new upstream protocol assumptions.

## Consequences

The fixture exercises public component APIs and preserves platform-specific publication refusals. It does not introduce an autonomous service driver or qualify live models, enrollment and sandbox behavior.

## Alternatives Considered

Stubbing a successful cleanup or reply receipt would bypass the integration this test is meant to exercise. Treating the fixture driver as production scheduling would also overstate native availability.
