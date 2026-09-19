---
kind: decision
id: ADR-117
title: Refuse dispatch completion while a file delivery is unsettled
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-EXECUTION-AUTHORIZATION]
---

## Context

Native file publication records `write_possible` when an upload or room event
may have reached the homeserver without a complete acceptance, and keeps that
row when the domain refuses the delivered settlement. The owned operation
completed the dispatch as soon as the runner protocol completed and process
cleanup was observed. On macOS the unqualified process-tree census failed the
attempt first, so `native_file_service_uncertainty` and
`native_file_service_restart` passed there by accident and failed on Linux and
Windows at 1baa80d.

## Decision

The domain writer owns the rule. Every completion path (`complete_dispatch`,
host `complete_owned_dispatch`, the explicit `complete_task_with_reply` hold and
its publication) checks the dispatch's file deliveries and refuses with `State`
while any delivery is unsettled. A delivery that is not `delivered` is
unsettled while it has no recorded failure, and stays unsettled while its event
or upload is `write_possible` or its upload is marked `outcome_unknown`: the
pipeline records a failure on such rows without knowing whether the homeserver
kept the write. The owned operation surfaces the refusal as its existing
`SettlementUnknown` failure and negative observation; the driver reports
`outcome_unknown`.

A delivery whose recorded failure precedes any possible write is settled
negatively and no longer blocks completion. No schema, deadline, retry,
delivery mutation or new failure kind is introduced, and the guard never
resends or cancels a possible write.

## Consequences

A runner that calls `complete_task_with_reply` while its own `send_file` is
still in flight receives a refusal and must wait for that delivery to settle.
Hosted Ubuntu and Windows runs remain the qualification for the file-service
executable scenarios; a local Linux container reproduction is evidence only.
