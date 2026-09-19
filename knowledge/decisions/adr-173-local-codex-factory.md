---
kind: decision
id: ADR-173
title: Original local Codex binding in the configured native fleet
status: Accepted
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

ADR159's provider-owned LocalCodex profile worked only for ordinary dispatch.
The native factory refused it and substituted per-agent HOME, so the authorized
local topology could not use the actual multi-agent service. Pass the original
Host's retained LocalCodex binding into its WarmHostPlan without exposing paths,
credentials, a serialized capability or a replacement readiness fact.

The factory reuses those original directory handles. Every provision requires
the selected unmanaged Codex/OpenAI resource, while workspace, copied project,
helper context and Matrix SDK stay per-agent. Retained warm bindings check local
provider identities before and after writer observations and during initialization,
idle, activation and handoff; ordinary dispatch keeps its existing live watcher.
Managed accounts cannot fall back to this selection.

This completes ADR161's deliberately separate factory budget join. The configured
active operation may use the existing twenty-minute maximum. Initialization
still captures one absolute deadline of min(active budget, thirty seconds).
Warm initialization reserves its actual live slot but does not validate an owner
wait against a phase that cannot run turns or service approvals. The real active
handoff still checks ApprovalHost::fits and refuses an insufficient operation.
No owner wait starts before its original request and no phase restarts a deadline.

Offline tests must exercise the actual configured service, two real local fake
runners, authenticated TLS/SDK traffic, task helpers, private approvals and file
attribution. Local temporary provider directories contain only synthetic data.
Live acceptance is separate and remains on the authorized isolated fleet.

## Verification

Configured local-profile two-agent/two-round task/file/approval/reply fixture
passes; warm8 selectors pass across the suite and corrected deadline fixture.
The historical configured-fleet2 tests also pass when explicitly run on macOS.
Library42/bootstrap20/CLI8 and strict all-target Clippy pass. Live root3 then
provisions two real local agents and passes four actual Robrix private-DM tasks
on ADR174, with exact isolated files and rendered encrypted replies. Group task
intake and live fleet file approvals remain separate gates. See the execution map.
