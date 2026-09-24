---
kind: decision
id: ADR-159
title: Explicit local provider-owned Codex binding for native dispatch
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

The operator requested the installed local tools, not a new mini3 model login.
TS runnerEnv carries an explicit provider environment; native bootstrap instead
creates a fresh runtime home. Copying authentication or inventing a managed
readiness receipt would not port that working path.

Add a separate opt-in local Codex profile to private startup configuration. It
names exactly one existing unmanaged preset/seat and two existing directories:
the provider's OS home and Codex home. The host retains their original objects,
checks owner/write permissions and identity before launch and while the original
operation runs, and retains them with unresolved process custody. Paths are not
credentials and never enter public DTOs. No auth file is read, copied, imported,
rewritten or permission-repaired. Provider-owned refresh remains the provider's
responsibility. No directory observation asserts login identity or readiness.

Selection and consumption both enforce the exact preset/seat. Managed bindings
are mutually exclusive and keep their original readiness gate. Local setup may
not enable factory/warm provisioning until that path has its own account join.
The selected local path uses the original native guardian, task MCP, private
approval, sandbox, metering and completion pipeline. Environment construction
keeps only explicit provider paths and the OS executable search path, not ambient
API keys or coordinator secrets. Existing unmanaged fixtures remain unchanged.

The host-exclusive stable-path premise remains: periodic identity checks do not
prevent hostile same-user mutation between a check and provider IO. Provider
directories may have ordinary read/search permissions (the actual local tools
do); foreign ownership or group/other write permission refuses without mutation.
This profile is Unix-only until Windows provider-directory policy is qualified.
It establishes neither multi-user isolation nor provider account identity.
