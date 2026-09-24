---
kind: decision
id: ADR-154
title: Bounded Claude streaming observations before native Host admission
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

## Decision

The local three-runner requirement needs a native Claude adapter. Start with the
official Agent SDK's streaming JSONL envelope: fixed auto-mode CLI arguments,
initialize/user/interrupt input, permission requests, request-tagged control replies,
cancellations and session-scoped runtime events. The codec observes data only.
It neither launches a process nor grants any permission, marks tasks complete,
publishes usage or claims stopped descendants. Production Host keeps its explicit
Claude refusal until the original owned authority and provider callbacks are wired.

Bounded pull decoding and unique-key JSON parsing are required at the boundary.
Move the existing strict parser to a private shared runtime module; preserve its
64-level recursion fence and every Codex protocol behavior. Claude limits are
one MiB per line, 64KiB per user prompt and ten seconds per partial line. Clock
rollback, malformed input and capacity failures close the decoder irreversibly.
Unknown event kinds refuse rather than becoming a success or approval.

The codec's permission callback shape is the official SDK transport mechanism,
not yet an amendment enabling a second approval authority under ADR005. An owner
approval adapter still needs original private Matrix/domain custody and exact
upstream request settlement; no allow encoder is introduced by this slice.

## Upstream references and qualification boundary

The locally observed CLI version is2.1.270. Reference sources inspected on
2026-09-16 (moving upstream references, not a compatibility qualification):

- https://code.claude.com/docs/en/agent-sdk/streaming-vs-single-mode
- https://code.claude.com/docs/en/cli-reference
- https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/_internal/query.py
- https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/_internal/transport/subprocess_cli.py

Fixtures exercise the documented envelopes offline. Actual installed-CLI
handshake/turns, permission round-trip, scoped MCP tools, usage, native macOS
ownership and real Palpo/Robrix soaking remain mandatory separate evidence.
The existing signed-in local CLI is not proof of those properties.
