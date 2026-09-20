---
kind: decision
id: ADR-174
title: Bounded retries for explicitly rate-limited Matrix observations
status: Accepted
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

The new local factory service and the prior successful Codex service both received
Remote(429) at21:33:15 UTC. Their continuous drivers stopped and transports were
fenced unavailable. Preserve both original failures. Their exact failing endpoint
was not retained; a later direct sync GET succeeded and does not identify it.

The low-level JSON GET path may now repeat only after a complete429 response.
Use at most four attempts under the original request deadline and existing body,
header, TLS and cancellation bounds. Honor valid integer Retry-After seconds and
retry_after_ms with the larger value, a10ms minimum, and exponential backoff.
With neither hint, start at1second. Invalid/unrepresentable hints or a delay that
cannot fit the remaining budget return the original429 without an early retry.
A final failure still follows the existing conservative fencing path.

No transport error, malformed200, authentication error or other status retries.
POST, PUT and binary transfers remain single-attempt; possible external effects
are never reconstructed from a status or resent. Fresh200 still passes the normal
identity/membership/session validation before any authority or scheduling.
Retired live transports need explicit new generation provisioning, as before.

Offline actual TLS tests distinguish delayed successful reads, both retry hints,
missing/malformed hints, count/deadline/cancel bounds and no mutation/uncertain
transport retry. These are not live service recovery evidence by themselves.

## Verification

TLS3 pass within Matrix library193. With transport9/token-provision9/upload7/
download6, all224 Matrix tests pass. Strict all-target native/Matrix/execution
Clippy and locked build pass. Explicit fresh-generation recovery restores both
isolated services with their original SDK stores; root3 then passes actual
two-agent/two-round private-DM execution in Robrix. Earlier429 failures remain
recorded. This is recovery after the fix, not a controlled live429 injection or
a claim that every server quota can fit the bounded retry interval.

## Amendment (2026-09-19): connect-phase redial and GET connection reuse

Operator decision after the two-agent soak. "No transport error ... retries" above
is narrowed for one case; everything else in this decision stands.

Evidence. Live fleets against the existing homeserver ended after 6, 10 and 16
minutes, and 4 of 11 soak fleets the same way: one failed dial ended its worker for
good, and once took the other two workers with Generation within 240 ms. A private
diagnostic build printed what the client discards: `ConnectError("tcp connect
error", TimedOut)` -- the connect budget elapsing with no TCP handshake. An
independent probe saw the same port unreachable in the same second while the same
host answered on other ports; separately, the product's dial failed while the
probe's own dials succeeded, a per-connection loss a fresh source port avoids. The
client dialled a fresh connection for every request (`Connection: close`, no idle
pool) -- thousands per fleet per ten minutes, each one a chance to meet this. No
recorded decision explains that setting; it arrived with the first transport
commit beside "implicit HTTP retries ... disabled" (ADR047). The TypeScript bridge,
with keep-alive and backoff, has run for days on the same endpoint.

Decision.

1. A JSON request -- GET, POST or PUT -- whose failure is connect-phase (the dial
   failed before a connection existed, so no request byte left) is redialled. That
   repeats a dial, never a request: a write is still sent at most once. It shares
   the four attempts and the one original deadline with the 429 rule; waits start
   at 100 ms and double, are cancellable, pass the pacing gate like any attempt, and
   a wait that cannot fit the deadline is not started. When attempts run out the
   failure is the same Transport word: the collector fences exactly as before, and
   a write's caller still treats its outcome as unknown. This is stronger ground
   than a 429: there the peer answered; here it never heard.
2. A TLS verification failure is "connect" to reqwest too, but it is a refusal and
   is never redialled. tokio-rustls reports it as an InvalidData io::Error wrapped
   by the connector; `io::Error::source()` skips a wrapped error, so the classifier
   descends through `get_ref()`.
3. JSON GETs use their own client with keep-alive reuse (two idle connections, ten
   seconds idle). Every POST, PUT, upload and download keeps the original client:
   a fresh connection and `Connection: close`, so a stale reused connection can
   never make a write uncertain. A POST/PUT 429 is still not retried, and uploads
   and downloads stay single-attempt: their bodies are streams and their custody
   marks "possible" for the whole transfer.

   First written as GET-only, because the write paths commit their "possible"
   marker before dialling and say "even connect errors ... are uncertain". That
   statement is about what a caller can conclude from the word Transport, and it
   still holds. It does not bear on a redial made where the failure is still known
   to be connect-phase. The same day, live: `POST keys/query` -- a read the Matrix
   API spells as a POST -- failed one dial in the approval intake; as a single
   attempt it stopped the approval pump and ended the coordinator and an agent
   with OutcomeUnknown. Hence all JSON methods.

Not decided here, deliberately. A lost GET response -- including one lost on a
reused connection the peer has just closed -- is still not retried; the short idle
bound is the mitigation, and the spec scenario "a lost GET response ... exactly one
request" stands. A fenced worker still does not resume, so restart and recovery are
unchanged. Which device in front of the endpoint drops the dials is not
established. The second Matrix client in the `provision` command is untouched.

Verification: five offline tests in `http/connect_retry_tests.rs` (a late peer for
a GET, bounds and cancellation, a late peer for POST and PUT with exactly one
request received, the TLS exclusion, reuse versus fresh write connections); the
Matrix crate's 235 tests pass. Live evidence is
recorded in docs/progress.md once a single fleet has run past the earlier
lifetimes.

### Correction (2026-09-20): a dial that times out was not reaching the redial

The amendment above was written for `ConnectError("tcp connect error", TimedOut)`,
and as landed it could miss exactly that. A send is awaited under a header wait
that is armed before the dial starts, and the shipped connect and header budgets
are both 5 s (ADR047), so the header wait ended first or tied: the failure
surfaced as Timeout, which is never redialled. The offline tests only refused a
connection, which fails at once, so they could not see it; a review of hosted CI
logs found it by reading the ordering.

Within one attempt the header wait now never ends before the connect budget plus a
100 ms margin. No default, validation rule or configured budget changes; when the
header budget is already the longer one it is untouched. A dial that times out is
therefore always observed as the connect failure it is, and is redialled.
`native_matrix_send_wait_never_ends_before_the_connect_budget` pins the bound for
the shipped tie and for both orderings. Still not covered offline: a real
black-holed dial, which needs a saturated listener and is not deterministic on
hosted runners.
