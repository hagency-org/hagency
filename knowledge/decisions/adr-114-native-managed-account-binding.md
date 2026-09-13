---
kind: decision
id: ADR-114
title: Fresh native credential namespace binding and actual managed launch
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

ADR111 additional configurations retain an existing Resource seat association.
No native registry currently qualifies how that association reaches runtime
credentials. A seat ledger row is not a host account binding.

## Decision

Part A owns fresh private Codex default credential namespaces under explicitly
selected native state. Effective authentication provider identity and quota remain
unknown; neither directory existence nor model selection establishes readiness.
There is no live auth inspection import copy or subscription-only claim.

Schema23 stores bounded preparations an original private identity key immutable
bindings and per-preset account generation associations. Native namespace identity
is versioned keyed and derived from actual local physical directory identity plus
fixed credential-source kind; it never aliases legacy ~/.codex or imports quotas.
Original preparation IDs and created directories survive unknown effects without
repair overwrite unlink or a new identity retry. Missing key or replaced roots
refuse instead of reconstituting authority from saved JSON.

One original non-Clone non-Deserialize managed handle retains current directory
proof and retirement. First creation and ADR111 clones atomically retain its exact
association; edits never rebind it. Owned claims and the actual Host validate the
same account generation preset and seat and derive HOME and CODEX_HOME only from
that original binding. Managed resources refuse the legacy fixed-home path.

The original writer retains SQLite-before-nonblocking-session-gate ordering and
rechecks original deadline and account authority after waits and around commit.
No global account/session mutex is held over SQLite or spawn. Retirement fences
new enrollment publication and launch; historical commitments and process cleanup
obligations remain. A possible commit or unproven cleanup remains unknown.

Offline account prepare inspect and retire commands use the original exclusive
native state owner. They refuse Busy while serve owns it and expose only safe
namespace state. No provider process Matrix Agent or approval is created by prepare.

## Validation and limits

Exact part A boundary: 38 paths. Nine bound selectors use actual files SQLite
locks native CLI and isolated runtime probes. Schema22 preservation and immutable
managed association are required. The existing stable host path/ancestor premise
remains explicit; retained-handle checks do not establish hostile namespace
isolation. Part B's retained browser workflow waits for qualified part A API.

Already-pinned hmac sha2 cap-std cap-fs-ext and the existing platform path crate
are the only dependency graph changes. No crate version is upgraded.

---

## Amendment — part B, and the two surfaces over part A (MA-S3a)

ADR-114:52 records that part B's retained browser workflow "waits for qualified
part A API". **Part A — part B's stated precondition — is built** (Schema23:
`account_identity_key`, `managed_accounts`, `resource_accounts`). **Part B is
therefore owed, and this amendment is it**: a console and CLI surface with
**no readiness**. `AccountChoice.authentication` stays `"unknown"` and `quota`
stays `None` (accounts.rs:224-225); the `AccountRow` DTO omits both.
**Opacity is unchanged**: no live auth inspection, no import, no copy, no
login, and neither directory existence nor model selection establishes
readiness. MA-S3b adds the readiness enum when the operator answers D-ADR114.

**The identity triple is the schema's, and none of it is a path.** `023:17`'s
CHECK names `namespace_identity`, `identity_tuple` and `seat_id` as the columns
that must exist exactly when a row is live. **None crosses the wire.**
`identity_tuple` is `canonical::encode` of
`{version, deployment, namespace:<DirectoryIdentity>, source}`
(accounts.rs:373-375), and `DirectoryIdentity` is `{platform, volume, object}`
(directory_identity.rs:8-12) — no path field exists to leak. The wire carries
the five-key `AccountRow` only: id, ordinal, state, revision, profile.

The store half was folded into this slice because no sibling MA-S3a-store slice
exists: no lane owns it and the backlog row licenses only the console and
main.rs, which was the gap.

---

## Amendment (MA-S1): the observed provider-login readiness fact

**D-ADR114 is decided: observe.** The operator's decision in force ends the
ambiguity this record's opacity clause carried: the provider login is
**observed at the host, never driven by the native side** — the operator runs
the provider's own login themselves, and native records what resulted.
Readiness and expiry are **derived facts**, not properties native asserts.

**What the opacity clause forbids, restated under the decision.** "No live
auth inspection" forbids **inferring** readiness from credential files, and
forbids reading, copying or exporting a token. It does not forbid
**observing** the outcome of a login the operator performed: that is the one
input retained's own caveat names as able to distinguish "the directory
exists" from "a valid session is in it" (`backend-v2.js:13541`). The
inference prohibition stands unchanged.

**What is observed, and where.** A host-only, one-shot login inside the
retained namespace (`HOME`/`CODEX_HOME` set exactly as
`apply_codex_environment` sets them, `accounts.rs:140-158`; ambient provider
keys refused; the child inheriting the operator's terminal, no captured
stdout), producing exactly a **derived mode** — `subscription`, `api_key`, or
`unknown` — and an **expiry**: the provider's own when it reports one,
otherwise a bounded default TTL, so no fact is ever immortal. The receipt is
written in the same SQLite-before-effect ordering materialise uses, so an
**interrupted login is `uncertain`, never ready**, and no read promotes it.

**What is never stored.** No credential, no token, no refresh material, no
`auth.json` body, no path to one — not in any column, any JSON blob, any log
line, any fixture, or any CLI output. The naming rule is ADR-014's
`/credential/` guard, pinned by a test.

**The store shape.** Migration **028** is MA-S1's per the backlog ledger
(§0.2): the readiness/expiry fact and its provenance in new tables created
`IF NOT EXISTS` — never `ALTER TABLE managed_accounts` (the 025 replay
hazard) — with the attempt-before-spawn ordering above. The fact's
consumption rule is fixed here so MA-S2 cannot drift from it: `usable`
requires `outcome='observed'`, matching generation, and unexpired, evaluated
**at read time**; a read never writes; where readiness is unknown the
dispatch **parks** with the named reason `account_readiness_unknown`.

**The console DTO is not this amendment's.** `AccountRow` stays five keys;
the readiness field is MA-S3b's, landing in its own commit when the fact
exists to serve.
