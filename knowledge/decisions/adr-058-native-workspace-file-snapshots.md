---
kind: decision
id: ADR-058
title: Copy bounded file bytes through retained workspace capabilities
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

File delivery needs immutable bounded copied bytes selected through a retained host workspace, without pretending the source was atomically frozen.

## Decision

`hagency-files` implements the filesystem snapshot primitive needed by ADR-027.
It has no Matrix, network, outbox, authorization endpoint, domain schema or service
wiring. Its snapshot is an immutable copy of bytes actually read, not a claim of
an atomic point-in-time source version or durable staging across host restart.

### Host authority and lifetime

The host supplies an already opened `cap_std::fs::Dir` to `Workspace::from_directory`.
There is no ambient path constructor, root handle accessor or external deserializer.
The root must be a real directory and not a reparse object. Choosing/provisioning
the root is the host's authority decision. This primitive does not verify owner
permissions or establish a protected physical namespace.

Every relative selection is opened one normal component at a time. The root and
all successfully opened ancestor handles remain alive, then the final file handle
is retained through the snapshot's lifetime. If directory or leaf pathnames are
renamed/replaced, the already obtained object capability remains the reference;
new pathname contents cannot substitute for its held handle. This is object
custody, not a promise that the object's current pathname remains under the old
root pathname. Private identity consists of those retained capabilities. We do
not serialize device/inode numbers or use a 64-bit Windows file index as a globally
sufficient identity (ReFS can use 128-bit identifiers).

The host must provision a private workspace and prevent external hardlink and
mount creation through its sandbox and OS policy. Link count checks cannot prove
point-in-time isolation against an attacker capable of linking/unlinking between
checks, mounting another tree, changing physical provisioning or controlling the
filesystem. Directory moves into or out of a private tree are physical namespace
policy, not something path syntax can secure. No production sandbox is enabled by
this crate alone.

Windows directory handles have a stricter mutation boundary. The pinned
[oflags implementation](https://docs.rs/crate/cap-primitives/4.0.3/source/src/windows/fs/oflags.rs)
removes FILE_SHARE_DELETE for directory opens, and
[directory helpers](https://docs.rs/crate/cap-primitives/4.0.3/source/src/windows/fs/dir_utils.rs)
do the same. Held directories cannot be renamed through the ordinary filesystem
API. The initial CI fixture incorrectly required those renames to succeed as on
POSIX and failed with Win32 ERROR_SHARING_VIOLATION32. Correct qualification must
attempt the rename, require that exact refusal, verify original bytes, then
release every retained capability and require the same rename to succeed.
This is a tested platform custody distinction, not a skipped mutation scenario
or a permission to weaken the production handle-sharing flags. Leaf-file
replacement remains a separate real rename case.

### Paths and reviewed platform behavior

`RelativeFile` accepts UTF-8 normal components separated by `/`: at most 4096 bytes,
32 components and 255 UTF-8 bytes per component. Absolute/parent/dot/empty components,
backslash, colon, control characters, wildcard/redirection syntax and trailing dot
or space are refused without normalization. Windows device names including
CONIN$/CONOUT$, optional extensions, and COM/LPT number aliases are refused
portably. Unicode and ordinary internal spaces remain supported. These deliberately
strict portable names are not a replacement for filesystem object checks.

Direct dependencies pin cap-std and cap-fs-ext 4.0.3; the lock pins the reviewed
cap-primitives 4.0.3 and preserves all prior package versions. Source was inspected,
not inferred from the API names:

- [DirExt](https://docs.rs/crate/cap-fs-ext/4.0.3/source/src/dir_ext.rs) delegates
  `open_dir_nofollow` to cap-primitives. The nofollow option addresses the final
  component; our walk passes exactly one validated component at each step.
- [Open implementation](https://docs.rs/crate/cap-primitives/4.0.3/source/src/fs/open_dir.rs)
  and [Unix flags](https://docs.rs/crate/cap-primitives/4.0.3/source/src/rustix/fs/oflags.rs)
  use directory/nofollow flags. The final read-only open also uses nonblock so a
  FIFO is rejected by handle metadata without waiting for a writer.
- [Windows open](https://docs.rs/crate/cap-primitives/4.0.3/source/src/windows/fs/open_unchecked.rs)
  and [CreateFileAtW](https://docs.rs/crate/cap-primitives/4.0.3/source/src/windows/fs/create_file_at_w.rs)
  use NtCreateFile with the retained RootDirectory. Normal single-component paths
  never enter their ambient-root/parent fallback. OPEN_REPARSE_POINT is requested;
  our metadata checks additionally reject every reparse attribute, not just the
  symbolic link kinds recognized by Rust.
- [Metadata extension](https://docs.rs/crate/cap-fs-ext/4.0.3/source/src/metadata_ext.rs)
  obtains hardlink count from opened-file metadata. It is not used as evidence that
  the source could never have external aliases. No project-owned unsafe code is
  added; platform operations remain inside these pinned dependencies.

Only regular files with link count 1 are read. Directories, symlinks, reparse points,
FIFOs and other special objects are refused. Metadata is checked on the opened file
before and after bounded copying. Changed length, copied-length mismatch or an
observed modification-time difference rejects the result.

### Copied bytes, limits and diagnostics

Snapshot exposes only byte slice, length, emptiness and SHA256 over copied bytes.
Its private Vec has no mutation accessor. Source file/ancestor/root handles and
its RAII permit stay inside private custody. Workspace, selection, snapshot and
private metadata do not implement Debug or serialization; errors are static
categories without content, path, source identity or ambient OS error details.

Host limits allow 1..16 MiB per source and 1..8 simultaneously held snapshots, default
4 MiB and 4. A permit is acquired before walking and retained through Drop, including
concurrent reads. Failed opens, reads, metadata checks or allocations release it.
Each operation reserves max_bytes+1 for its byte buffer and reads in at most 64 KiB
chunks; oversized sources/growth fail before unlimited allocation. Directory depth
bounds retained ancestor handles. Allocator bookkeeping and OS descriptor overhead
are not counted as payload bytes. There is no detached filesystem worker.

These are byte, iteration and ownership limits, not a hard kernel I/O deadline.
Nonblock is commonly ineffective for regular files; a stalled disk/network mount
can block a syscall. Network filesystems and protected workspace provisioning have
not been qualified here. A future service adapter must make an explicit worker
and unknown-outcome/cancellation design before enabling this in a live request.

### Mutation and validation limits

Other processes may rewrite a source while it is copied. Metadata comparison is
best effort: same-size writes, coarse timestamps or restored modification times
can hide changes. A controlled fixture rewrites an entire file after the first
chunk and restores its modification time; the API may return mixed copied bytes
with their correct digest. It does not guarantee detection or an atomic original
version. Once returned, later source mutations do not alter the owned snapshot.

Actual local fixtures cover Unicode/nested selections, normal/hard/symbolic links,
nonregular objects, oversize and concurrent permit exhaustion, root/ancestor/leaf
replacement, in-copy mutation and immutable post-copy bytes. Unix creates a real
FIFO; Windows creates actual symlinks and a junction. Platform fixture prerequisites
fail visibly. Local macOS execution plus Linux/Windows cross-compilation is not
three-platform runtime evidence; hosted CI must run the corresponding native tests.
Persistent staging, host restart identity, encrypted media, domain authority and
Matrix delivery remain later ADR-027 work.

## Consequences

Snapshots keep source, ancestor and root custody with their copied bytes and digest. Metadata checks remain best effort; staging durability and current dispatch authority are separate responsibilities.

## Alternatives Considered

Reopening an ambient pathname or following aliases would substitute source authority. Calling metadata equality an atomic source-version guarantee would contradict the documented in-copy mutation fixture.
