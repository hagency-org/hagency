---
kind: requirement
id: REQ-APPROVAL-CANONICAL-PROJECTION
title: "Durable canonical approval projection"
status: accepted
satisfies: [REQ-OWNER-UI-APPROVAL]
tags: [approval, matrix, durability]
---

Approval creation and every accepted state transition must atomically persist a monotonic revision and durable Matrix projection work. Send plans and receipts use exact CAS identity. Failed pre-rename persistence is invisible in memory and on disk; failures after atomic replacement retain the committed state and degrade persistence health. Legacy migration emits no actionable request or public notice work.
