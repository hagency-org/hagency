---
kind: decision
id: ADR-034
title: Control one original periodic loop through reviewed native requests
status: Accepted
tags: [herdr, loop, evidence, recovery]
---

## Context

The operator authorized finishing the remaining three-layer E2E blockers.
ADR-033's goal controller and actual readonly inspection passed, while the
required periodic audit and restart phases still need reliable loop controls.

## Decision

Extend the same readonly-plan CLI with fixed60second loop creation and
identity-bound pause/resume/delete. Preserve pre-send fences, exact process
facts, scoped JSON-RPC correlation, exclusive evidence and no automatic retry.
Require original ID, creation time, prompt digest and interval on subsequent
controls. A successful mutation must also agree with a fresh complete list.

The plan supplies reviewed prompt bytes; this does not establish their semantic
safety by hashing alone. The E2E plan must use its original audit-only duty.
Only the middle owns these controls in the live chain; tests use local child
fixtures and root's source work does not execute the lower's business task.

## Limits

An active schedule does not prove that any natural tick ran. Pause does not
interrupt an active turn. get/set and list/control are not atomic compare-and-set
APIs; conflicting post-intent observations remain unknown without rollback.
The dedicated-session helper refuses additional or unbound loops rather than
acting on ambiguous state. Remaining fault/restart/receipt/Matrix gates stand.
