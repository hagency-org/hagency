# Two-agent qualification runbook (MA-M8b, ADR-144)

Operator-run. This is the qualification half of the two-agent acceptance: the
three claims an in-process fixture cannot honestly prove, executed against a
REAL Palpo homeserver and a REAL owner client, recorded as a tracked evidence
file, and validated by the always-present test
`native_two_agent_qualification_records_its_evidence`
(`native/hagency/tests/qualification.rs`). That test fails — never skips —
while the record is missing, partial, stale or carries no verdict per claim;
hosted runners skip it by name in `.github/workflows/rust.yml` because they
will never have a real homeserver.

## What is being qualified

1. **`homeserver_admission`** — a foreign homeserver's membership and power
   levels decide admission, not a fixture's state list.
2. **`e2ee_second_device`** — E2EE against a second real device with real key
   upload/claim.
3. **`approval_round_trip`** — the approval round-trip through a real owner
   client: a human sees the card and answers.

## Procedure

1. Build the pinned native binary: `cargo build --locked -p hagency`.
2. Provision (or point at) a real Palpo homeserver and register two agent
   accounts plus the owner account, each with a second real device where the
   claim needs one.
3. Start the service against that homeserver with
   `hagency serve --state-dir <dir> --listen 127.0.0.1:13300` (the loopback
   listen is fixed; see ADR-127).
4. From the owner's real client: drive the two agents into ONE shared room,
   open one direct room per agent, hand a task across, observe the approval
   card, answer it, and let both agents spend tokens.
5. Verify each DM body on the receiving device (E2EE), confirm the shared
   room contains no DM body, and read the usage attribution per engagement.
6. Rewrite `native/hagency/qualification/two-agent.json` in the SAME COMMIT
   as any launch-path change: drop `placeholder`, fill every field the test
   validates (schema, versions, full commit hash, timestamp, homeserver
   identity as a sha256 DIGEST — never a hostname —, both engagements, the
   shared room marked delivery-only, both DM directions with
   `plaintext_verified`, two usage rows, one passing verdict per claim each
   naming its evidence, and a non-empty `unproven` list), and run
   `cargo test -p hagency --test qualification` locally to confirm.

## Refusals

The record never carries homeserver names, room aliases or credentials —
identities are sha256 hex digests. A red validating test is the honest state
until an operator run is recorded; it is never converted to a skip.
