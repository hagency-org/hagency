spec: task
name: "Scan the packaged native entrypoints for residual Node references"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, release, packaging, audit]
---

## Intent

Bind the M8 item-6 gate (the migration plan's definition of done, line 3):
the packaged native runtime must not invoke Node.js or hide an embedded
JavaScript runtime. Today that property is proven by manually reading the
two unit templates; this slice makes it a **bound test** — a scan over the
native packaging surface as it actually exists: the two deploy unit
templates by path, plus the native installer's **rendered output** of those
same two units (produced inside the test by running the installer's own
render step). ADR-134 (versioned release) governs the packaged artifact
set; ADR-134's already-bound staged-tree scan
(`native_release_entrypoints_scan_finds_no_node`,
`tests/release_cutover.rs:295`) covers the staged release tree — a
different surface; this spec binds the **templates-and-render** surface, so
the two selectors are distinct checks, not the same one twice.

## Constraints

### Must
- Scan the two native unit templates by path: `deploy/io.hagency.native.plist` (launchd) and `deploy/hagency-native.service` (systemd) — exactly the files `deploy/` carries for the native deployment.
- Produce and scan the installer's rendered output of those two units in-test: run the install script's own render step (the `sed` substitution of `__INSTALL_DIR__`/`__STATE_DIR__`/`__USER__` over each template) against temp paths, and scan the rendered invocation lines — this is what the installer writes (`install/install-native.sh` Linux and Darwin arms render exactly these two units).
- Assert none of the scanned invocation lines (ProgramArguments / ExecStart / Exec words) references `node`, `npm`, `npx`, `__NODE_BIN__` or a `.js`/`.mjs` script path.
- Run as a bound test in the native test suite — not a manual reading, not a CI-only grep outside the gate.

### Must Not
- Do not scan or constrain the RETAINED deployment's files (`deploy/com.hagency.supervisor.plist`, the retained `.service` units, `install/install-macos.sh` and its node checks): they run Node by design and are removed at M9 cutover, not by this gate.
- Do not claim "generated hook templates" or "the versioned-artifact ExecStart strings" — the tree carries no hook templates under the native packaging paths, and the current templates' invocation lines carry the unversioned `hagency` placeholder; the scan asserts what exists.
- Do not modify any template or the installer — this slice adds the scan only.
- Do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/scripts/ (the scan script or test module)
- native/hagency/tests/
- specs/task-rust-native-package-scan.spec.md
- docs/progress.md

### Forbidden
- Live services, credentials, deployed state.
- deploy/** and install/** (read-only inputs: the templates and the render step are asserted, not changed); .github/workflows/**.

## Acceptance Criteria

Scenario: The packaged native entrypoints reference no Node runtime
  Test: native_package_entrypoints_reference_no_node
  Level: integration
  Test Double: the two deploy unit templates as shipped, plus the installer's own render step run in-test over temp paths
  Given the native packaging surface — deploy/io.hagency.native.plist and deploy/hagency-native.service, and the installer's rendered output of each produced by its own sed substitution inside the test
  When the scan runs over their invocation lines
  Then none references node, npm, npx or a Node script path
  And the assertion is a bound test whose failure names the offending file and line

## Decisions

**The surface is exactly what the tree carries.** Two templates, one
installer, two rendered units — no hook templates exist under the native
packaging paths, and every `*hook*` file in the repo is retained JS this
scan excludes by design. The rendered-output half exists so the scan also
covers the exact text the installer writes at install time, not only the
template it reads. The retained Node deployment is out of scope the same
way: it keeps running Node until M9 removes it.

## Out of Scope

The staged-release-tree scan (ADR-134's, already bound), the release
workflow's enablement (operator), and any change to the retained
deployment's files.
