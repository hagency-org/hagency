---
kind: decision
id: ADR-104
title: "Qualify retained local NTFS directory sync for private media storage"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

ADR103's original Windows run 34578962296 at dfcad753 passed under two separately
established ordinary same-user tokens. Actual same-object private RW directory
handles on local mounted NTFS acknowledged flushes; unchanged Store staged the
original encrypted object and a separate process restored its exact ciphertext,
descriptor and checked plaintext. Five earlier originals remain failed. This is
local NTFS OS acknowledgement and normal process-restart evidence, not hardware
power-loss qualification or production Store opening. Production currently uses
a duplicated read-only cap-std handle and normally reports directory-unconfirmed.

The disposable probe's process exit on unexpected native pending status cannot
become a production library cleanup rule. ADR066/077 inspection and exact custody
must remain available without converting incomplete evidence into upload authority.

## Decision

Use a Windows-only opaque private-storage directory sync owner, implemented
inside hagency-store's audited private boundary. It opens only fixed relative dot
through the caller's retained cap_std Dir. It privately owns the freshly opened
RW File and exposes no File/raw handle/Clone/Debug/serde constructor. Original
and candidate overlap for existing private checks and complete volume plus 128-bit
identity comparison. Identity/private mismatch refuses before journal creation.

Only actual local NTFS, disk device 7 and mounted characteristic 0x20 with optional
named 0x20000 are supported. Read-only volume, every other characteristic, unknown
query and unavailable RW access cannot produce a qualified owner. They preserve
explicit directory-unconfirmed storage inspection; there is no ambient path,
read-only-handle sync fallback, retry loop or broader Windows/filesystem allowance.
The ordinary-token probe proves no privileged dependency. Production neither
adjusts tokens nor requires every unrelated ordinary token privilege to be disabled.

The factory returns Some(sealed owner) only after a completed actual
profile observation; None means no directory-sync authority and stays inspectable.
Each positive Store sync still requires the original journal sync followed by an
actual retained NTFS directory sync, with private checks before/after. No synthetic
SyncEvidence setter is added. Real directory-sync failure remains the existing
FileSyncedDirectoryUnconfirmed outcome; journal failure preserves OutcomeUnknown
and original staging quarantine/custody. Preparation/restoration still require
FileAndDirectorySynced and exact original operation, namespace and receipt.

Pending-call custody is part of the accepted boundary. The factory owns a unique fresh
synchronous NtCreateFile object; no caller can clone or run competing IO on it.
Native query uses one initialized fixed aligned buffer and IOSB. A documented
pending completion continues waiting on that original SYNCHRONIZE handle in the
same calling blocking worker, retaining all buffers/handles until actual completion.
It never returns/unwinds live IO storage or spawns an unowned task. Immediate and
final statuses must succeed. Unexpected completion-wait failure or a signalled handle with an inconsistent
completion leaves that same calling worker parked with its original fixed storage
and handle. It cannot return, start another query, or produce an admission. It
never exits the application. The parent accepted this exceptional retention policy before implementation.
The controlled actual-handle gate tests caller result loss before the native
call; it does not claim to reproduce a native pending result.
There is no new public deadline, timeout extension or hard-cancellable disk claim:
existing synchronous flushes already require the caller's owned blocking worker;
FileService timeout/close remains unknown while that original worker is retained.

Keep media-store unsafe-forbid and use only existing pinned cap-std/cap-fs-ext and
windows-sys versions. The exact factory/type API and owned completion handling are
listed in the external `adr104-interface-manifest.md`. Native FFI is confined to
`hagency-store/src/private/windows_directory.rs`, separate from the platform
helper currently owned by the parent task. Existing Unix behavior, domain schema,
Matrix routing, unknown-upload recovery and production activation flags are unchanged.

Native acceptance must exercise default Store::create/open with its original
ordinary cap-std Dir, rather than handing it the probe's prequalified RW Dir.
The ordinary-token two-process fixture must qualify original encryption/staging,
held-directory rename refusal and exact restart restoration. Original failure
artifacts remain separate. Public/nonprivate objects, mismatched live directories,
unsupported/unknown profiles, flush failure and pending completion must not produce
qualified preparation/restoration or release original ownership early.

## Consequences

The bounded local NTFS profile can supply actual Windows file-and-directory sync
through production Store opening. This removes one storage prerequisite only;
it does not by itself qualify a complete FileService, Matrix event, task Done,
provider delivery or migration cutover. Unknown filesystem/directory outcomes
remain inspectable without qualified restored/upload custody.

The helper adds one privately owned candidate File and fixed native query storage
per Store factory. It creates no global registry or background thread. A filesystem
operation may block its original owned worker; safe physical cleanup cannot be
inferred from an outer timeout. The rare unexpected pending/wait-failure policy is
explicitly a permanently retained fixed allocation and handle while that worker
is parked. It is never reclaimed by a timeout; process termination ends that
physical custody. No bounded completion or allocation reclamation is claimed.

## Alternatives Considered

Reusing the read-only duplicate fails its documented write-access precondition.
Accepting every successful directory flush would include filesystems documented
to acknowledge without the required NTFS directory-structure behavior. Raw volume
flushes, privilege adjustments and ambient path reopening cross the accepted scope.
Copying the probe's process exit or dropping stack IO on pending is unsuitable
for a library. A competing global worker/registry would duplicate FileService's
existing owned blocking worker and obscure its finite custody bounds.


Pinned capability trace: cap-std 4.0.3 `Dir::open_with(".")` reaches
cap-primitives `manually::open`, `MaybeOwnedFile::into_file`, `open_unchecked`,
and `CreateFileAtW`. The dot is normalized to an empty relative native name with
the retained RootDirectory still set. It does not enter `reopen_impl`,
`ReOpenFile`, or ambient `CreateFileW`. `CreateFileAtW` adds SYNCHRONIZE and,
without FILE_FLAG_OVERLAPPED, FILE_SYNCHRONOUS_IO_NONALERT. The helper fixes its
own options and never accepts caller flags.

Primary references: [NtCreateFile synchronous object contract](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-zwcreatefile),
[NtQueryVolumeInformationFile](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntqueryvolumeinformationfile),
[IO_STATUS_BLOCK](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/ns-wdm-_io_status_block),
and [WaitForSingleObject](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject).
These distinguish synchronous operation, final completion, and actual waiting;
no asynchronous completion is inferred from a returned positive status alone.
