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
