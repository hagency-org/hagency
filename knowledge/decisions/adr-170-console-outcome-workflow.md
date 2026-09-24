---
kind: decision
id: ADR-170
title: Native lifecycle console workflow for original stopped outcomes
status: Accepted
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

Add a private lifecycle-scoped stopped-dispatch list beside the unchanged roster.
Sixteen rows per page expose only identifiers, fence, reason and original receipt
availability. The existing inspection transaction decides whether current custody
permits resolution. Its inventory remains private; the browser displays only
review fields and keeps the one-use token in memory.

Continue, accept_completed and keep_blocked use ADR165 without new authority.
The browser freezes the serialized decision before POST and retains it when the
response is uncertain. Only an explicit operator retry reuses that exact request;
no effect, refresh or network handler generates another decision. No secret is
stored or rendered. Only inspection has a 2 MiB response bound, accommodating the
existing 1 MiB inventory plus task/route envelope. Other reads keep 64 KiB.
