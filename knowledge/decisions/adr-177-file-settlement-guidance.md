---
kind: decision
id: ADR-177
title: Guide bounded read-only inspection of nonterminal file receipts
status: Accepted
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

The paced live pair's approved file reached durable Delivered, but its Codex
caller completed with a failure message based on an earlier OutcomeUnknown
inspection. The denial on its other participant correctly created no upload.
Preserve these actual results; this is not a fully passing file workflow.

ADR097/101 intentionally report possible writes as OutcomeUnknown even while
the original owner is progressing. Do not weaken that projection, reinterpret
it as a known failure, or grant another send. Add fixed text to the MCP result
for Queued and OutcomeUnknown: inspect the same delivery_id, spaced about one
second apart, only within the original deadline and authority. Stop on Delivered,
Failed, lost authority or expiry. If still unknown at expiry, report unresolved.
Keep the exact structured receipt and isError=false. Terminal receipts receive
no polling advice. Update the catalog to state the same distinction.

This is model-facing protocol guidance, not a new scheduler, wait loop, retry,
permission or completion path. It cannot settle historical unknowns. There are
no application UI strings or translation keys to change. Live model compliance
still requires a separately recorded fresh task.

Validation: the focused native helper regression passes for both a held upload
and a held encrypted event. Releasing each original response settles Delivered;
exactly one upload and one event PUT occur. The combined native library45,
file-service7 (including truncated acknowledgements) and MCP6 suite passes.
Strict all-target native Clippy with native-console-browser, production caller
audit and diff check pass. No formatter, commit or PR; agent-spec is unavailable.

Fresh live evidence: root2 session007 uses the ADR177 binary (SHA256
156f6da48fb35a76bd58db75c558b9d5abc7f30267395d55cc4d45827f5886ac).
Actual Robrix Approve once leads to one accepted upload, delivered encrypted
attachment, canonical Done and exact ISO_CODEX_SETTLEMENT_004_OK reply. Independent
owner download/decryption verifies all31 requested bytes. The actual selected
room renders the attachment and exact reply. The service then closes with its
original observed exit0. Evidence is the private file-004-completion-evidence.json
and file-004-media-byte-evidence.json. No provider intermediate tool transcript
was retained, so this live success does not claim a witnessed unknown-to-delivered
model polling sequence; that sequence is covered by the deterministic MCP test.
