---
kind: observation
id: OBS-PROJECT-SIDE-GENERATION-UPGRADE
title: "Existing project-side files need durable outbound generation metadata"
status: accepted
---

Existing version1 project-side credentials can contain a valid appservice token
or representative token without `outboundGeneration`. The new protected acting
projection and approval publisher validation require that metadata. Merely
adding it when a credential is issued leaves existing deployments unable to
publish approvals; successful appservice verification does not reissue a token.

`ProjectSideStore` now initializes missing metadata on construction, for active
and staged credentials with an existing outbound token, including inactive
sides. It saves through the existing protected atomic store before exposing the
new generation. Credentials, access decisions, side activity, pending status,
bindings and approvals remain unchanged. A registration credential without a
representative token remains unable to send.

Existing valid generations survive reload and equal-token updates. Actual token
rotation retains its existing generation change. A failed migration write aborts
construction; an unchanged store is not rewritten. Invalid nonmissing metadata
is a persistence error. The load and optional save still scan retained state.

Evidence: `tests/project-side-generation-upgrade.test.js` loads old on-disk
records, checks protected API output and public projection privacy, observes
reload stability and token rotation, and forces an actual filesystem write
failure after reading the original file. No live credentials are test fixtures.
