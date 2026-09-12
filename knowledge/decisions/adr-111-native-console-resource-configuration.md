---
kind: decision
id: ADR-111
title: Retained additional resource configuration with finite native authority
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

ADR108 provides native observations and publication in the retained console.
The original resource wizard still needs native create and edit commands. Native
Resource already holds profile and ceiling fields but not friendly names runtime
policy rate caps or a managed credential binding for first-resource enrollment.

## Decision

Retain the original four-step wizard and allow an additional configuration only
from an existing resource's actual private account association. Allow model and
reasoning selection from the canonical embedded policy and explicit preserve clear
or monthly ceiling operations. Preserve exact missing/null periods when untouched.
Framework provider seat and preset identity are never supplied by the browser.
New private preset IDs are generated once by the host and inserted without upsert;
new resources default to published in that same commit. Edits retain withdrawal
and stable IDs and refuse profile changes with reserved or active engagements.

A separate explicit configuration ticket grants only these commands. Read-only
and publication tickets do not gain this scope. One original session owns one
concrete non-Clone non-Deserialize mutation access. SQLite IMMEDIATE precedes its
nonblocking gate; the writer never acquires the global session map. Check original
deadline expiry and retirement after locks and before mutation/commit and after
commit. A lost reply or authority after possible commit remains unknown without
automatic retry; later reads describe current state without attributing a create.
Busy logout retains a visible retry and never claims revocation.

Use the original static wizard document and query selection with native full
navigation. No Node server is deployed. Native unsupported inputs are absent:
name daily cap execution policy endpoint key extra args auth-home and account IDs.
Drafts survive same-selection refresh without advancing expected revision and
clear upon navigation credential replacement or logout. No model choice or
configured association establishes authentication runtime readiness or quota.

## Validation and limits

The accepted boundary is 36 paths. Real private SQLite fixtures cover creation
edit CAS commitments gate clocks caller loss and restart. Actual bilingual Chromium
uses retained assets and the native executable with empty runtime PATH. Default
tests are browser-tool independent; enabled browser lane prerequisites are fatal
when missing. Full M7 and first-resource enrollment remain open.

First-resource enrollment requires a separately contracted host-only versioned
managed-profile/account binding derived from actual normalized host credential
namespace and key scope, consumed by the actual runtime. No new browser account
ID or random seat substitutes for that prerequisite. A future canonical managed
preset must own friendly names rather than a second browser metadata store.
