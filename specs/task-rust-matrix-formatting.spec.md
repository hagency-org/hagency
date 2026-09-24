spec: task
name: "Preserve Matrix Markdown content in native formatting"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, matrix, formatting]
---

## Intent

Provide a pure bounded Matrix event content formatter, anchored to the retained
JavaScript formatter and its exact dependency versions. This is a content proof
for M6, without live transport or private-route authority.

## Constraints

### Must
- Preserve original plaintext msgtype reply relations and other event content fields.
- Preserve existing truthy formatted content without pretending it was sanitized.
- Format text and notice bodies and nested edit content with explicit recursion and byte limits.
- Escape raw HTML and emit only the retained Matrix tags attributes and href schemes.
- Preserve tables code blocks strikethrough line breaks and remove generated images.
- Derive reproducible JavaScript behavior vectors with source and locked dependency identity.
- Document parser and automatic link recognition differences and reject capacity excess visibly.
- Keep runtime formatting independent of network credentials domain state and routing authority.

### Must Not
- Do not send events or accept room owner device or session authority through content.
- Do not broaden the HTML allowlist or sanitize preformatted content under a false trust claim.
- Do not modify retained JavaScript behavior or live transport.

## Boundaries

### Allowed Changes
- native/hagency-matrix-format/**
- native/scripts/matrix-format-vectors.mjs
- native/fixtures/matrix-format.json
- ./Cargo.toml
- ./Cargo.lock
- .github/workflows/rust.yml
- specs/task-rust-matrix-formatting.spec.md
- knowledge/decisions/adr-050-native-matrix-formatting.md
- docs/**

### Forbidden
- Domain schemas SDK transports live services credentials and original dirty checkouts.

## Acceptance Criteria

Scenario: Native formatting reproduces retained JavaScript content vectors
  Test: native_matrix_format_vectors
  Given exact versioned JavaScript formatter outputs
  When native text notice edit and preformatted content is formatted
  Then plaintext relations and supported formatted output match the source oracle

Scenario: Generated HTML preserves the exact Matrix allowlist
  Test: native_matrix_format_security
  Given hostile raw HTML links images and code content
  When formatting generates HTML
  Then only approved tags attributes and safe hrefs occur without evaluating input HTML

Scenario: Content size and recursive edit processing are bounded
  Test: native_matrix_format_bounds
  Given large UTF8 bodies malformed edit content and deeply nested JSON
  When a content DTO is admitted and formatted
  Then invalid or excessive input fails without partial output or silent truncation

Scenario: Formatting cannot replace routing or encrypting authority
  Test: native_matrix_format_preservation
  Given thread direct and encrypted transport envelopes with relation metadata
  When content is formatted repeatedly
  Then source content and relations remain unchanged and formatting grants no transport authority

## Out of Scope

No event sending decryption media transfer route selection or cutover. Supplied
formatted_body remains caller-trusted passthrough, as in the legacy helper. This
slice covers the pinned shared corpus, not every syntax difference between the
JavaScript and Rust Markdown engines. Parser and automatic link recognition edge
differences are recorded in ADR050 before native formatter activation.
