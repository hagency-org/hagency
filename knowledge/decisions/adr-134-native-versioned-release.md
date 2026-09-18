---
kind: decision
id: ADR-134
title: "Versioned native release: one workspace version, per-platform artifacts, residual-Node scan"
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
liveness: auto
tags: [release, packaging, versioning, native]
---

## Context

The service review's F7: the native workspace version is `0.1.0`
(`Cargo.toml:6` — the single `[workspace.package]` version every crate
inherits via `version.workspace = true`, e.g. `native/hagency/Cargo.toml:4`),
the retained product is `1.2.0` (`package.json:3`), `release.yml` has **no
cargo step** (its gate, stamping and packaging derive everything from
`package.json`), and the migration plan §9 step 1 requires "a versioned
release plus rollback kit" — which cannot be met by extending `release.yml`
alone, because the native artifact has no name in that namespace. M8 items 1
and 6 add the packaging, uninstall and residual-Node scan duties; ADR-136
pauses Windows, so the release targets are Linux x86_64, Linux aarch64 and
macOS arm64.

## Decision

**Version source.** Exactly one: the `[workspace.package]` version in the
root `Cargo.toml`. During the migration the two versions are **explicitly
separate channels** — `package.json`'s `1.2.0` versions the retained JS
product and its tarballs; the workspace version versions the native binary.
They are not reconciled and not required to meet; convergence to one number
happens at M9 retirement, not before. **A tag means different things per
channel:** `vX.Y.Z` on the retained channel must equal `package.json` (the
existing `release.yml:34-47` gate fails otherwise); the native channel uses
`nvX.Y.Z` (an `n` prefix, never a plain `v`), where `X.Y.Z` must equal the
workspace version — the native workflow refuses a tag/workspace mismatch the
same way. A native release therefore never collides with a retained release
tag and each workflow triggers on its own prefix.

**Artifacts.** Three per release: `hagency-nvX.Y.Z-x86_64-unknown-linux-gnu`
(from an `ubuntu-24.04` runner), `hagency-nvX.Y.Z-aarch64-unknown-linux-gnu`
(`ubuntu-24.04-arm`), and `hagency-nvX.Y.Z-aarch64-apple-darwin`
(`macos-15`), each `cargo build --release --locked -p hagency`, plus one
`SHA256SUMS` covering all three. No Windows artifact (ADR-136). Each
artifact name carries the version, so the SR-1/SR-2 units' `ExecStart`/
`ProgramArguments` can point at a versioned path and unit/binary cannot
disagree silently (as those ADRs require). Signing/notarization: **none
claimed yet** — recorded as an open item for the M8 signing-policy slice,
not silently omitted.

**Residual-Node scan.** The release tree must contain **no Node entry
point**: no `package.json`, no `node_modules/`, no `*.js`/`*.mjs`/`*.cjs`
shebanged entry, no `node` in any wrapper the package ships. The check is
`native_release_entrypoints_scan_finds_no_node` (below): it unpacks the
staged release tree and fails on any of those findings — never skips. The
native binary's own no-Node guarantee is already tested
(`native_binary_survives_crash_without_node`,
`native/hagency/tests/cli.rs:98`); the scan extends it from the process to
the package. (The retained remote tarball keeps its Node — different
channel, different artifact, not scanned by this check.)

**Cutover identity.** The binary reports the workspace version through the
existing clap `version` wiring (`native/hagency/src/main.rs:8-10`, so
`hagency --version` prints it); ADR-134 adds **no** field to the `/ready`
payload — the readiness contract (`/health` 200-while-live, `/ready` 503
boundary, component vocabulary) stays exactly as brief 21 F2/F3 fixed it,
and cutover step 2 ("record deployment identity, expected versions") is met
by `hagency --version` plus the artifact filename, not by widening a
diagnostic route.

**Workflow.** `.github/workflows/release-native.yml` — `workflow_dispatch`
**only**, with the `nv*` tag trigger present but **commented out** and a
comment explaining why: the native release is not yet the release of record
(the JS product is; M9 has not happened), so an accidental `nv` tag must not
publish anything. Enabling the trigger is a one-line uncomment that belongs
to the M9 cutover slice, and the comment says so. The draft builds the three
targets, computes `SHA256SUMS`, and uploads artifacts; it creates no GitHub
Release while dispatched manually (that, too, is gated on the tag trigger).

## Consequences

Good, because one version source exists, tags cannot collide across
channels, artifacts are self-describing, the residual-Node claim is a test
not a promise, and the workflow cannot publish by accident.
Bad, because two version numbers coexist until M9 (an operator must read
the right one for the right channel), and the disabled trigger means the
first real native release needs an explicit enable step.

## Alternatives Considered

- Derive the native version from the retained `package.json` — rejected:
  couples the channels and makes native patch releases impossible without
  touching the JS product.
- One shared `vX.Y.Z` tag for both — rejected: one tag would have to satisfy
  two version gates that move at different speeds; the release review
  already flagged the sub-tag semantics of the retained globs.
- Add `version` to `/ready` — rejected: widens the readiness contract that
  brief 21 F2/F3 froze and that SR-1/SR-2 evidence depends on.

---

## Note: how the upgrade procedure is tested without the release workflow

The upgrade procedure this record defines (install N → install N+1 →
restart; rollback by pointing the unit back at N's artifact) is exercised
by a bound test that produces both versions **in-test**: two local builds
of the same tree with different workspace-version constants — the same
constant `hagency --version` reads — each built to its own path and named
as this record's versioned artifacts, so both coexist in the install dir
exactly as the procedure assumes. No release workflow, tag or network
artifact source is involved; that is the tested form of the procedure, and
`specs/task-rust-native-upgrade.spec.md` binds it (the upgrade-continues
and rollback-restores selectors).

The local N+1 copy and its content-addressed cache include the native source's
workspace-root compile-time inputs: `lib/role-capacity.json` and the four shared
agent-home entry templates under `docs/`. The same original paths drive both
copying and fingerprinting; a changed template invalidates the cached artifact.
No duplicated or substitute template content is generated by the upgrade test.
This repairs the missing-input local build, not tagged release qualification or
production cutover authorization.
Each uncached child build backs up the exact current N artifact and restores it
even if child build/staging fails. An old same-version backup cannot substitute
an earlier source tree; restoring N does not turn a failed build into success.
