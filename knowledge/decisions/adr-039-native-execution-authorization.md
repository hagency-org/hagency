---
kind: decision
id: ADR-039
title: "Pure native permission scope derivation precedes approval authority"
status: Accepted
tags: [rust, runtime, approval, security]
---

## Context

Reusable permission scopes must derive from bounded host-owned metadata before a separate owner-consent transaction can authorize their use.

## Decision

The native core derives supported reusable scopes from host-owned runner
metadata, never from a writable Agent request or presentation text. A descriptor
is a candidate for owner consent, not consent itself. Exact command scopes include
all represented additional permissions; structured network requests bind the
complete normalized host and protocol. Unknown fields or permission types produce
no reusable scope and remain once/deny-only in the future approval adapter.

Path flavor is explicit. Lexical normalization preserves path case and trailing
separators. Windows reusable scopes require a rooted drive or complete UNC share;
root-relative, drive-relative and device namespaces remain unsupported. This is a
narrower reusable-scope boundary than Node's win32.isAbsolute, not a prohibition
on a future individually approved operation. It does not establish filesystem
containment, mount identity, reparse safety or the actual runner sandbox.

Filesystem entry arrays use deterministic UTF-16 lexical order of canonical entry
JSON. Legacy code uses localeCompare, whose locale-dependent ordering is unsuitable
for portable native scope identity. Fresh native stores require no legacy grant-key
import. Ordinary read/write path arrays preserve the existing UTF-16 sort and
deduplication. Shared JavaScript vectors cover equivalent supported ordering;
dedicated native tests cover the explicit deterministic entry-order rule.

Metadata and descriptions are bounded before producing scope identities. Policy
normalization defaults YOLO off and permits an explicit true value only for Codex.
No use of this module changes a dispatch policy: operator authentication, a current
writable lease and effective runtime-policy enforcement remain separate checks.

Persistent approval records, exact private owner verdicts, agent and binding
incarnations, task-completion epochs, grant revocation and per-request native
decisions must be integrated transactionally before enabling approvals. The module
has no network, store, endpoint or runtime decision side effects.

## Consequences

Unsupported metadata yields no reusable grant. Lexical path normalization and a scope digest do not establish filesystem containment, current leases or effective sandbox enforcement.

## Alternatives Considered

Deriving grants from Agent presentation text or unknown permission fields would trust unowned input. Treating normalized paths as physical containment would claim protection this pure module cannot provide.
