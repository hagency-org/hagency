spec: task
name: "Scan the packaged native entrypoints for residual Node references"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, release, packaging, audit]
---

## Intent

Bind the M8 item-6 gate (the migration plan's definition of done, line 3):
the packaged native runtime must not invoke Node.js or hide an embedded
JavaScript runtime. Today that property is proven by manually reading two
plist/unit files; this slice makes it a **bound test** — a scan over every
packaged native entrypoint asserting none references `node`, `npm`, `npx`
or a Node script path. ADR-134 (versioned release) governs the packaged
artifact set; this spec adds no decision, it binds the check.

## Constraints

### Must
- Scan the packaged native entrypoints by path: `deploy/io.hagency.native.plist` (the native launchd unit), `deploy/hagency-native.service` (the systemd unit), and every generated hook template under the native packaging paths (`deploy/`, the installer templates it generates from, and the versioned-artifact ExecStart strings they carry).
- Assert none of them references `node`, `npm`, `npx`, `__NODE_BIN__` or a `.js`/`.mjs` script path in any invocation position (ProgramArguments/ExecStart/exec lines).
- Run as a bound test in the native test suite — not a manual reading, not a CI-only grep outside the gate.

### Must Not
- Do not scan or constrain the RETAINED deployment's files (`deploy/com.hagency.supervisor.plist` and friends): they run Node by design and are removed at M9 cutover, not by this gate.
- Do not modify any packaging template — this slice adds the scan only; a template fix is a separate change.
- Do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/scripts/ (the scan script or test module)
- native/hagency/tests/
- specs/task-rust-native-package-scan.spec.md
- docs/progress.md

### Forbidden
- Live services, credentials, deployed state.
- deploy/** (read-only inputs); install/**; .github/workflows/**.

## Acceptance Criteria

Scenario: The packaged native entrypoints reference no Node runtime
  Owed Selector: native_package_entrypoints_reference_no_node (parked — the name is owed by the implementing slice and binds only when it lands; no Test: line here yet)
  Level: integration
  Test Double: the packaging templates as shipped on the tree, scanned by the bound test
  Given the packaged native entrypoints — deploy/io.hagency.native.plist, deploy/hagency-native.service, and every generated hook template under the native packaging paths
  When the scan runs over their invocation lines
  Then none references node, npm, npx or a Node script path
  And the assertion is a bound test whose failure names the offending file and line

## Decisions

**The retained Node deployment is out of scope by design.** The scan's
inputs are the NATIVE packaging paths only; the retained JS supervisor units
keep running Node until M9 removes them, and a scan that failed on them
would conflate the two deployments.

## Out of Scope

The release workflow's enablement (operator), the versioned artifact build
itself (ADR-134's, landed separately), and any change to the retained
deployment's files.
