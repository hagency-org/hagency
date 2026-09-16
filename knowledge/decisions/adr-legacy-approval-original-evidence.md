---
kind: decision
id: ADR-LEGACY-APPROVAL-ORIGINAL-EVIDENCE
title: "Attest legacy approval origins for read-only status"
status: accepted
satisfies: [REQ-APPROVAL-CANONICAL-PROJECTION, REQ-OWNER-UI-APPROVAL]
tags: [approval, matrix, legacy, privacy]
---

A historical request's matrixEventId is a candidate owner verdict ID, not a durable original request receipt. The trusted bridge may attest bounded normalized observations from that exact verdict and its same-room reply target. It authenticates Matrix retrieval/decryption without dispatching historical events through live event handlers. The backend independently checks only canonical tuple, role, event relationship, current registered publisher and ProjectSideStore credential consistency; a JSON attestation is not independent proof of Matrix transport.

Accepted evidence is immutable in legacyOriginalEvidence, separate from canonical requests and native private_request plans. It binds the canonical owner, original DM, request/agent/project/project-room/digest, preserved decision, candidate verdict, original event ID and actual original sender. Missing or conflicting evidence remains unresolved. Evidence never changes approval state, bindings, verdict consumption or projection row count.

An original event does not reveal historical credential generation. That field remains null. A legacy read-only status may use the same original sender's currently verified private context, pinned once in its first status plan. Before that pin, multiple valid matching private contexts remain unresolved; after it, no alternate scope/generation replaces the pin. Credential rotation blocks prepare/begin/retry while exact attempted-plan receipts remain valid. Native approval publisher and generation requirements are unchanged.

The current room/readiness/join/encryption observations are authenticated bridge claims and must be refreshed before publication; this store/API unit performs no Matrix I/O. A local-bot plaintext room uses only the ADR-003 explicit non-production plaintext-test opt-in. A current side representative may publish to the distinct ADR-016 project-side plaintext topology only after both the representative and owner are joined and an exact Matrix `M_NOT_FOUND` proves the encryption state is absent. A side room observed as encrypted fails closed because the representative has no crypto context. The bridge owns encrypt-once preparation and exact stored payload replay.

The status wire is com.agentchat.approval.status.v1, version 1, migration_kind legacy_v1, selected canonical row revision/state/decision, complete tuple and an exact m.in_reply_to relation to the original event. It carries no actions or private tool preview. Robrix additionally corroborates the original event, full sender, owner, room and tuple at display time; every legacy control stays disabled. Consumed means the runtime used a verdict, not that the task succeeded.

Writes retain existing atomic semantics: pre-rename failure restores memory/disk; post-rename failure retains committed evidence and reports degraded durability. This decision adds no blanket new recovery rule to other store operations. Historical proof retrieval and bounded bridge draining are separate implementation units.
