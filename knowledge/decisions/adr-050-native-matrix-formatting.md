---
kind: decision
id: ADR-050
title: Pure bounded Matrix content formatting with a JavaScript oracle
status: Accepted
---

## Context

Native Matrix formatting must preserve reviewed JavaScript behavior while keeping content transformation separate from route and send authority.

## Decision

The M6 formatting slice is `hagency-matrix-format`. It replaces no transport and
changes no live JavaScript behavior. Its API is
`MatrixContent::new(serde_json::Value)?.formatted()?`, returning a serializable
content DTO with `as_value` and `into_value`. It has no homeserver, room, owner,
device, session, credential, dispatch or send authority. An extra `room_id` in a
content object is inert data. A host adapter must select its separately verified
route and apply the existing privacy, generation, encryption and send-custody
contracts; this crate cannot turn a private reply into a group reply.

The retained behavior is [matrix-markdown.js](../../lib/matrix-markdown.js) and
its bound [test](../../tests/matrix-markdown.test.js), as required by ADR023 and
the migration plan's formatting/media source inventory. `m.text` and `m.notice`
with string bodies gain `org.matrix.custom.html` and generated `formatted_body`.
Original body bytes, msgtype, reply/thread/replacement relations, mentions and
extra content remain unchanged. Truthy nested `m.new_content` is formatted
recursively; no enclosing relation is copied into or removed from the edit.
Formatting is idempotent and errors do not partially mutate the original DTO.

Existing truthy `formatted_body` is **trusted passthrough**, just as in the
retained helper. It is not sanitized or certified safe, even if it contains raw
HTML or an unusual JSON type. The caller must not treat this constructor as an
arbitrary HTML sanitization endpoint or let untrusted runtime input bypass the
text-only path by asserting that its HTML is already safe. Falsy values follow
JavaScript truthiness, including empty string, null, false and zero. Non-text
content, including encrypted envelopes, stays opaque.

Generated HTML has exactly the retained allowlist:

- Tags: p, br, strong, em, del, s, blockquote, ul, ol, li, pre, code, a, h1–h6,
  hr, table, thead, tbody, tr, th and td.
- Attributes: href/title on a, class on code, and start on ol. Empty attributes
  are omitted; table alignment/style attributes and images are removed.
- Schemes: http, https, mailto and matrix. Relative paths, fragments and queries
  stay relative. Protocol-relative URLs beginning with either slash spelling
  are refused. Scheme checking removes ASCII controls/spaces and embedded HTML
  comments exactly as the retained sanitizer does, then checks case-insensitively.
- Raw HTML parsing is disabled. All generated text/attributes are escaped by
  the renderer; code does not linkify, nor can links nest. Dangerous Markdown
  URLs remain literal Markdown, while unsupported safe-to-parse schemes lose
  their href. Generated image alt text is removed with the image.

The parser is pinned `markdown = 1.0.0`, with CommonMark, tables, double-tilde
strike and GFM automatic links; raw HTML, MDX, footnotes, task checkboxes and
other extensions stay disabled. The crate's explicit `html_flow/html_text`
construct switches preserve Markdown interpretation inside escaped HTML blocks.
The renderer adapts GFM links to the retained no-fuzzy-www policy, Unicode
punctuation boundaries and FTP anchor behavior. URL encoding uses only the
pinned mdurl 0.3.1 percent primitives; hostnames use raw punycode primitives from
idna 1.1.0, avoiding UTS46 hostname rewriting. Linkify 0.10.0 handles the extra
FTP text form; unicode-general-category 0.6.0 identifies punctuation.

We rejected markdown-it 0.6.1 after reproducing an ordinary-input panic on
`> *a*` followed by `> [`, independent of its bundled linkifier. The selected
parser renders that fixture correctly. No panic swallowing, source-specific
placeholder, vendored dependency or alternate JavaScript renderer is used.
Unsafe reference definitions are prevented from becoming definitions in one
compatibility reparse, preserving paragraph continuation and later valid
references. This is formatting adaptation, never permission or routing inference.

`native/scripts/matrix-format-vectors.mjs` executes the retained pure helper,
checks installed and locked markdown-it 15.0.1, sanitize-html 2.17.7 and
linkify-it 6.1.0, and records source and Node-lock hashes. Its 63 vectors are
byte-exact content expectations, not snapshots invented from Rust. Source and
fixture hashes normalize CRLF for Windows. Native CI installs the existing
locked production dependencies with **all install scripts disabled**, runs the
oracle in check mode, then runs Rust tests. The original bound Vitest test is
also run locally. Native source does not invoke JavaScript.

This is a bounded proof, not full syntax or transport parity. Intentional native
limits are 256 KiB admitted JSON, 64 KiB per rendered body, 512 KiB aggregate
formatted JSON, JSON depth 16, edit depth 8, AST depth 100 and at most 1024
`[ ] * _ > backtick ~` bytes per Markdown body. The marker budget conservatively
includes code; exceeding any limit returns Capacity without truncation or a
plaintext fallback. A truthy non-object edit returns Shape rather than imitating
JavaScript object spreading of malformed arrays/scalars. Unsupported AST forms
return Markdown. These differences must remain visible at future host integration.

The shared corpus proves the retained common and adversarial behaviors listed
above. It does not establish that two independent CommonMark implementations
have identical behavior on every malformed nesting, reference or automatic-link
boundary outside the corpus. More syntax coverage and server event-size/chunking
policy remain explicit M6 qualification work before activation. The 512 KiB
internal output bound is not an assertion that a homeserver accepts such an event.
No Matrix send, E2EE round trip, client display, file delivery or cutover is
claimed by these formatting tests.

## Consequences

Bounded formatting produces content DTOs and explicit errors without silently truncating. Shared vectors cover their corpus, not arbitrary Markdown equivalence or homeserver event acceptance.

## Alternatives Considered

Treating a room_id field in formatted content as routing authority would expose private replies. Plaintext fallback on malformed or over-budget content would conceal the documented native subset.
