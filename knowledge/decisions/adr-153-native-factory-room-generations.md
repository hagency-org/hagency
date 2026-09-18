---
kind: decision
id: ADR-153
title: Coordinate original factory project-room observations across agents
status: Decided
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Evidence

The configured executable two-agent fixture physically provisions both targets
in the same project. The second real join changes its joined-member snapshot.
The collectors' fixed generation1 then conflicts with the shared domain room
scope. One intake returns Conflict, fences the room and transport, and the other
returns Generation. This is a real composition defect, not permission to ignore
the changed snapshot, put the agents in different projects or reset their SDKs.

## Decision

The original warm factory Host owns one bounded async project-observation guard,
shared by its original coordinator and accounts/Collectors before enrollment. Only its project
Group observations use this coordination; ordinary configured collectors and all
Direct/approval room rules remain unchanged. The guard covers the prior domain
read, authenticated full-state GET, publication and recheck. It carries no
credential, snapshot, generation, route or positive authority. Waiting is
cancellable and bounded by the existing SDK wait limit, not a larger deadline.

A new Host-only bounded writer command refreshes the current project Group
snapshot in one transaction. It compares the exact available generation captured
before that GET with the original current row. Absence must remain absence;
unavailable or changed generations refuse. An identical authenticated snapshot
reuses its current generation; changed joined membership advances exactly once.
Encryption, invite policy, privacy, owner and registration must still match;
changes there refuse instead of automatically downgrading or adopting policy. The existing
observation validator checks the original active engagement, registration,
transport, owner, full members and project/privacy before any write. A changed
generation runs existing route/approval/reply retirement; missing owner/sender
remains a negative snapshot. No unavailable scope is automatically revived.

The returned observation records the writer-selected generation. An absent/lost
reply is not positive evidence and invokes existing conservative failure fencing.
SDK configuration/identity, enrollment custody and credentials are not rebuilt.
There is no Host or runner setter for successful factory results. Old group
sessions are not rebound or reused; unrelated current owner DMs continue to use
their own unchanged scope. General group-session restart/rebinding and explicit
recovery of negative scopes remain separate required work.

## Verification

Repository/writer tests must cover unchanged and changed snapshots, exact
generation CAS, first observation, unavailable and foreign scope, unsafe members,
retirement of old group work and isolation of an unchanged private DM. The actual
configured service must pass the original two-agent test in the same project,
for both account kinds, without seeded target authority or relaxed limits.
These are offline gates, not new Palpo/Robrix soak or entire-port qualification.
