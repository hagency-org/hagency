---
kind: decision
id: ADR-093
title: "Bind owned execution and file capture to retained host workspace roots"
status: Accepted
tags: [rust, execution, workspace, custody]
---

## Context

ADR053 selects an exact exclusive workspace resource from the writer's frozen
owned-dispatch scope, but originally stored only its host path. ADR058 snapshots
through a retained directory capability supplied independently by a host. A file
service needs those two paths to share one actual directory object and one exact
execution attempt before reading a source under ADR027.

The accepted trust boundary requires the host to provision private directories
and keep their ancestors and configured path stable throughout execution.
`private::check_handle` explicitly does not establish physical namespace
provisioning or protection from a malicious process sharing the OS identity.
ADR058 also leaves external hardlink/mount manipulation and cross-boundary moves
to OS/sandbox provisioning. This decision preserves those obligations; it does
not introduce hostile same-UID isolation as a universal file prerequisite.

Pinned Codex `rust-v0.153.4`, commit
`3d2ee51ca2d5db578f328aa75e20aa22c0197c9a`, illustrates why an fd alias alone
would not establish a stronger guarantee. Its `linux-sandbox/src/bwrap.rs:327`
calls cwd normalization; lines 996–999 canonicalize it and lines 345–352 emit a
canonical `--chdir`. Writable roots likewise resolve symlink targets before
`--bind` at lines 568–583. Child `fchdir` or `/proc/.../fd` paths would not prove
that subsequent tool and sandbox opens stay on the held object after an
unsupported host ancestor replacement.

## Decision

`Host::new` keeps its host-only configuration API and opens each selected root
once. Its immutable map admits 1–16 bounded resource IDs, fixed canonical paths
of at most 4096 encoded bytes, actual private directory handles, and no duplicate
live directory objects or canonical parent/child roots. A narrowly scoped
platform helper compares two live Unix device/inode pairs or Windows
`FileIdInfo` volume plus all 128 identity bits. Unsupported queries refuse;
there is no truncated Windows identity fallback. Comparisons detect inconsistent
configuration; the held handles provide custody. Neither is a global identity
or proof of hostile namespace isolation.

The original retained file handle is duplicated into ADR058 `Workspace`.
All relative source reads use that instance. Reopening the configured path is
only a consistency check before launch and around source access, never a new
source root. Both `Launch.directory` and Codex Settings retain the same fixed
host path under the existing ancestor-stability obligation. No guardian,
sandbox, process group, descriptor inheritance or Windows Job policy changes.

A successful original `start_owned_dispatch` acknowledgement creates a private
binding between this retained root, the full original capability digest and the
writer's exact scope fingerprint. The binding also retains the ORIGINAL
`DomainStore`; callers cannot substitute a stale/copied database for current
validation. Lost start results yield no binding and no child. The operation's
single mutex slot exposes it once through
`Operation::take_workspace_binding(&mut self) -> Option<StartedWorkspace>`.
The method is nonblocking; None denotes not ready, already taken or unavailable.
There is no unbounded queue, callback or additional task.

`StartedWorkspace` has no public constructor, Clone, Debug, serde, resource-path
getter or unchecked Workspace accessor. Its host consumer calls
`validate_current(&RunnerCapability)` against the retained original writer
immediately before copying and again before publication. `snapshot` accepts the
same capability, a validated `RelativeFile` and the consumer's byte limit. It
checks original association, operation liveness and root consistency before and
after the synchronous ADR058 copy. Snapshot alone is NOT fresh domain authority;
ADR092's host worker must perform those current writer checks after its own queue
wait. No model input chooses another root, database, room or thread.

Default file capacity is 4 MiB and four held snapshots per selected root.
`Host::with_file_limit` selects a smaller 1-byte through 4-MiB profile before
execution, duplicating the same held roots rather than reopening paths. The
snapshot request refuses before any source read if this profile exceeds its
consumer's configured limit. It never reads up to a larger default and rejects
only afterward. Workspace permits remain shared by every capture from a binding;
held snapshots retain them. Filesystem syscalls themselves are not cancellable
and allocator/descriptor overhead is not an exact physical-memory claim.

A scope guard retires new access on every execute return or unwind. The existing
operation cancellation flag refuses access immediately; a late-taken handoff is
still retired. Report retains the binding/root separately before a child can
exist, after the actual owner in drop order, including unresolved cleanup.
Held snapshots retain their original source/root capabilities and immutable
copied bytes after execution retires. No cleanup uncertainty releases a new
execution grant or current file permission.

## Consequences

This supplies a bounded development-host source association for the next file
service slice. It does not implement MCP send_file, a file worker, staging,
upload, file-event composition, delivery, or production activation. Native
runtime sandbox and provisioning qualification remain open. The host must still
prevent cross-Host root reuse and unsupported mount-topology overlaps; per-Host
alias/nesting rejection is not a global lease registry.

Fixtures provision actual private roots rather than changing production
permissions. Actual offline native children write through configured cwd and
source snapshots return those bytes. Tests also cover full capability mismatch,
revocation against the original writer, retired early/late handoffs, actual
post-Started worker unwind, lost start acknowledgement, copy profiles/capacity,
root replacement and full native directory comparison.

The alternate-writer test opens a real copied database. Normal reopen correctly
fences its Started rows; an explicit negative SQL fixture then restores the
copied writer's old live rows to model an incorrectly restored stale replica.
That copy still accepts the old capability after the ORIGINAL writer revokes it,
while the sealed binding refuses. This is a database-affinity regression, not a
supported recovery procedure or proof of source authenticity.

Unix replacement tests show pre-launch refusal and immutable held snapshots;
they do not claim ongoing path-based Codex tools are isolated during deliberate
violation of the trusted-ancestor contract. Windows cap-std directory handles
refuse ordinary rename while retained, and tests require rename after release.
Windows compile checks do not substitute for actual Windows fixture execution.

Validation at this commit: 46 execution/platform tests plus eight affected
MCP/Matrix CLI tests pass. Strict cross-crate lifecycle passes all seven scenarios
and the 17-path boundary. Native and Windows GNU execution/platform Clippy pass;
actual Windows execution and combined integration remain separate CI evidence.

## Alternatives Considered

- Independent execution and file path maps: leave physical source association
  ambiguous and permit a new directory to replace the original file source.
- Path equality, repeated canonicalization or inode serialization as authority:
  none retains an object or eliminates concurrent namespace mutation.
- Child-only fchdir or descriptor aliases: useful primitives, but pinned Codex
  can convert tool/sandbox paths back to mutable names. Not implemented here.
- New mutable global workspace registry: unnecessary for one owned operation;
  a single sealed handoff is finite and preserves existing worker ownership.
- Protected bind mounts or privileged namespace provisioning: potentially a
  stronger profile, but requires separate supported platform, privilege-drop,
  mount-propagation and real framework qualification. Existing Linux recovery
  admission must continue requiring its initial user/cgroup namespaces, nonzero
  UID, NoNewPrivs and zero capability sets. This slice does not loosen it.
