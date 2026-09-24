---
kind: decision
id: ADR-115
title: Explicit finite v1 approval wire profiles for native and retained peers
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION]
---

## Context

Native cards preserve forty-hex request IDs. Retained JavaScript uses thirty-two
hex, and the original v1 schemas only describe that length and two actions. The
accepted scoped approval implementation already emits additional task and always
choices. Robrix additionally rejects Windows scope paths. None of these gaps is
fixed by shortening an identifier or treating a successful send as interoperability.

## Decision

Amend the paired v1 schemas explicitly. The exact request grammar is
`^approval_(?:[0-9a-f]{32}|[0-9a-f]{40})$`. Both profiles keep the same binding and
decision semantics; length selects validation limits only, never authority.
Unupgraded receivers remain incompatible with native requests.

Preserve the retained profile's body16384, description4096 and preview8192
JSON-Schema character ceilings. The native profile uses49152-character ceilings
for these fields and private scope descriptions, alongside the existing producer's
stricter **49152 encoded UTF-8 bytes for the entire packet**. JSON Schema cannot
express that aggregate encoding rule; callers must enforce it separately. No
producer fact is truncated and no legacy field limit silently increases.

Request-only `upstream_rpc_id` is an optional integer0..9007199254740991 or nonempty
string of at most255 characters. Native cards include it and preserve its type.
`upstream_request_id` remains a nonempty display string. Neither is a permission
key or echoed into the closed verdict detail.

Finite action sequences are once/deny, once/always/deny, and
once/task/always/deny. Reusable actions require a nonempty explicit description and
absolute workspace in `reusable_scope`; task additionally requires its task ID.
Native reusable scope includes the persisted kind (exact_command, network_host,
permission_profile) and task ID. Retained scope permits a null task for always.
No unknown fields, actions, duplicate/reordered buttons or absent scope become
valid. Verdicts admit the four canonical choices, while the backend independently
checks whether that exact pending request and current scope permit the choice.

Remote workspaces are presentation strings, not client filesystem paths. The
contract recognizes POSIX, drive-rooted and complete UNC paths independently of
the receiver OS and preserves the original string. Client origin admission is a
separate prerequisite: a matching global script notification is not a human
approval click.

## Consequences

Capture the native corpus from actual original-writer test packets and validate
it beside the real retained producer. Schemas describe payloads only: they do not
establish encrypted room, sender, original UI control, current ownership, task,
grant, lease or runtime-response authority. Client parser/click qualification,
encrypted SDK transport and executable coordination remain separate gates.

## Alternatives Considered

Rewriting native IDs breaks original lookup. An unbounded pattern hides malformed
identities. A new v2 namespace would require producer, intake and timeline-filter
changes despite unchanged decision semantics. This reviewed finite v1 amendment
keeps those semantics explicit and documents the receiver upgrade requirement.
