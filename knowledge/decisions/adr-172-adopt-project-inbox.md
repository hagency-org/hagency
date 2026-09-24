---
kind: decision
id: ADR-172
title: Optional authenticated project inbox in external-account adoption
status: Accepted
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

The existing native adoption command bound only an encrypted owner DM, leaving
the local-provider qualification setup without its existing project route. Add
optional project_inbox containing distinct session/workspace identifiers and an
explicit room generation (default1). Its target is always the verified request's
project room; the caller cannot supply another room or privacy classification.

Authenticate the original observer and agent tokens as before, and read project
membership again under the agent identity. Both views must agree on membership,
invite-only and encryption facts, with the owner and agent joined. Refuse before
domain writes otherwise. Existing verify_request still requires the declared
unencrypted project room; owner DMs retain exact two-member encryption rules.

Use the existing transport, room and verified-session methods. Their current
generation, privacy and mention gates are unchanged. A requested new session name
cannot silently resolve to an existing different ID. No new account, credentials,
SDK owner, execution grant or HTTP mutation is introduced by this option. Exact
replay preserves both routes. The receipt names the optional project inbox; legacy
receipts omit this field. Matrix response collection now enforces its existing
one-MiB ceiling incrementally for responses without Content-Length as well.

Offline tests use a real local TLS peer and production adoption code. They cover
both authenticated identities, separate group/DM routes, replay, missing members,
invalid/aliased input, preservation of existing session IDs, and a chunked response
that exceeds the byte limit before its withheld EOF. Live group qualification
remains separate from these fixture results.
