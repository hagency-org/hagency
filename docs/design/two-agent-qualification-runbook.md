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

Current operator target (2026-09-16): local Codex, Claude Code and OctosCode on
the Mac; mini3 supplies Palpo only. See
`knowledge/requirements/req-local-three-runner-qualification.md`. The remote
Codex account prepared earlier is unused and its login helper is disabled. The
generic managed-login procedure below is not an instruction to authenticate on
mini3. Native local adapters/authentication and macOS process ownership must be
qualified before claiming the current three-runner end-to-end target.

### Fresh managed model authentication

Before starting the qualification fleet, initialize its separate private native
state and run `hagency account prepare --state-dir <state>`. Preparation creates a
namespace, not a provider session. Do not copy the existing service's credentials
or substitute its runtime HOME for the new agents' homes.

The operator authenticates the retained account from their own terminal:

```sh
hagency account login --state-dir <state> --id <prepared-account-id> \
  --login-binary <absolute-pinned-codex-binary> --device-auth
```

The fixed option invokes the provider's device authorization flow. The operator
completes provider consent; terminal output is not a qualification log. The
native process clears ambient provider values, retains the exact account
namespace and records only the exit-derived readiness receipt. A successful CLI
command alone is insufficient: refused/unknown provider exits also leave an
inspection result, so verify the observed, unexpired account readiness before
enrolling the selected resource. Do not infer readiness by inspecting auth files.
Default login without `--device-auth` retains the browser callback flow.

Keep the same physical state path and mount for preparation, login and serve.
Run account commands while the isolated service is stopped; they refuse another
writer. Enroll the real resource through the existing account-management surface
and preserve its association for both agents. This shares only the explicitly
selected managed account; project workspaces and task contexts remain per-agent.
Actual model calls and two-agent isolation still require live verification.

### Matrix and client qualification

1. Build the pinned native binary: `cargo build --locked -p hagency`.
2. Provision (or point at) a real Palpo homeserver and register two agent
   accounts plus the owner account, each with a second real device where the
   claim needs one.
3. Start the service against that homeserver: first provision the state dir
   with `hagency init --state-dir <fresh empty dir>` — `Bootstrap::open`
   fail-closes unless the dir yields a readable `operator.token`, and only
   `init` writes it (ADR-127's installer ordering; it refuses a non-empty
   dir on purpose) — then run
   `hagency serve --state-dir <dir> --listen 127.0.0.1:13300` (the loopback
   listen is fixed; see ADR-127). A real-homeserver lane additionally sets
   `--palpo-transport`; development legs set `--development-driver` and read
   `development-driver.json` from the state dir.
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

The record never carries homeserver names, room aliases, credentials, bearer
tokens, client secrets, or any device key material — identities are sha256 hex
digests, never names. Until an operator run is recorded,
`native/hagency/qualification/two-agent.json` is a checked-in placeholder and
the validating test is red on purpose; that is the honest state, never a skip.
A red validating test proves the record's shape and verdicts, not that the run
happened.
