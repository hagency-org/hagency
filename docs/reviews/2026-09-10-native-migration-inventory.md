# Native migration source inventory

The M0 candidate list is now source-derived and checked. It does not establish
native API parity or complete M0. Work was isolated on `feat/rust-inventory` from
`a41ab10`; the original checkout and all services remained unchanged.

| Surface | Observed source declarations |
| --- | ---: |
| Explicitly classified helpers, service files and installers | 138 |
| Express route registrations | 199 |
| Express middleware registrations | 3 |
| Custom dispatcher branches | 12 |
| Raw HTTP listeners | 2 |
| CLI commands across root/remote profiles | 32 |
| MCP registrations across root/remote mirrors | 32, representing 16 distinct tool names |
| Next proxy method exports | 4 |
| Next proxy allowlist rules | 61 |

Counts describe declarations and patterns, not distinct deployed endpoints. A
middleware path is not an extra API, mirrored MCP declarations are not additional
tools, and proxy allowlist rules are not backend registrations.

The old regex list had 201 literal candidates and 112 helpers. It omitted the
SSE installer's variable argument and the global function-only middleware, and
counted a commented `app.delete` example. The AST list resolves the SSE route to
`/api/stream` at `backend-v2.js:7938`, connects delivery queue registration at
`backend-v2.js:7988`, and separately records the App Service receiver, optional
edge dispatcher and fleet protocol. Two fleet protocol call sites cover inbound
and outbound adapters; a recorded call does not prove the deployment enables it.

The Next catch-all is `/api/hagency/[...path]`, with GET/POST/PUT/DELETE exports.
The generated static sentinel is not a forwarding server. Its exact regex read
and write allowlists retain source locations and separate M7 migration gates.
No application module was imported to enumerate these surfaces.

Helper policy review found important distinctions. Autodeploy watchers are
installed runtime/update behavior. Team provisioning is runtime provisioning.
The audit CLI invokes mirror/package/dependency checks, and the stable watcher
invokes CD preflight: those are dual-use verification helpers. Their existing
JavaScript may remain on development machines, but native runtime callers must be
replaced before a Node-free deployment can pass. Literal path mentions are
navigation aids; they are not a complete shell call graph.

## Reproduce

Install development dependencies with `npm ci`, then run from this checkout:

```sh
node native/scripts/inventory.mjs --check
node node_modules/vitest/vitest.mjs run tests/native-migration-inventory.test.js
```

After intentionally reviewing source or policy changes, regenerate with
`node native/scripts/inventory.mjs --write`. Both the policy and generated snapshot
belong in review. The checker fails for unresolved supported registration forms,
new/missing helpers, unclassified listeners, missing reviewed module links, parser
version changes and fixture drift. It never rewrites the snapshot during a check.

Espree is directly pinned to 11.2.0; its existing lock entry was retained and only
the root development dependency metadata changed. Installation validation uses a
separate cache directory, not the shared development dependency symlink.

## Remaining gates

| Gate | Evidence and remaining requirement |
| --- | --- |
| Detector scope | Supported AST forms and explicit limitations are in ADR-035. Arbitrary reflection, generated routes, higher-order aliases, external SDK internals and shell execution semantics are not resolved by a hash. Extend detection or record an unresolved gate when introducing these forms. |
| Complete M0 traceability | Events, schemas/stores, feature flags, generated workspace wrappers and optional feature behavior still need accepted requirement and exact behavioral test mapping. Source roles do not supply it. |
| Supported Agent versions | `lib/frameworks/index.js` lists Claude, Codex ACP, Codex, Hermes and Octos adapters. That list is not native support. Each exact runtime version/OS pair needs protocol, authentication, sandbox and cleanup acceptance. |
| Hardware and budgets | `.github/workflows/rust.yml` names ubuntu-24.04, macos-15 and windows-2025 CI images, not qualified product hardware. Embedded CPU/libc/RAM/storage and desktop targets still need selection and measured latency, memory and disk budgets. |
| Runtime custody and terminals | Native process proofs are separate contracts. Persistent terminal parity, effective runtime sandboxing, complete macOS descendant custody and guardian-loss handling remain explicit M4 gates. |
| Release integration | Actual Matrix/Palpo/browser workflows, packaged dependency audit, soak/failure testing and controlled M9 cutover remain mandatory. This task did not run a model, contact a homeserver or deploy a binary. |

## Validation

Exact inventory Vitest tests, inventory reproduction, ESLint and the diff check are
recorded for this change. An isolated `npm ci --offline --ignore-scripts` checks the
pinned dependency lock; it is not a native-addon or production installation proof.
Agent-spec 1.4.0 parse/lint passed with a 100% quality score and advisory warnings.
Lifecycle was invoked with `--layers lint,boundary`, but still attempted Cargo
selectors and returned three `Skip` verdicts because these are Node/Vitest tests.
That lifecycle result is non-passing; it is not substituted for the exact Vitest
results. All 533 Node task bindings resolved after linking the existing console
development dependencies into this isolated worktree. The actual test results
and lifecycle summary are recorded in the progress entry for this commit.
