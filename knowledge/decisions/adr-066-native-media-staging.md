---
kind: decision
id: ADR-066
title: Retain bounded media in a private capability journal
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Retained encrypted or generic media needs a bounded private journal whose storage custody remains separate from dispatch and Matrix authority.

## Decision

This ADR027 slice follows ADR058 file snapshots and ADR061 attachment crypto.
A separate hagency-media-store crate owns bounded storage, leaving media codec
and domain/outgoing authority independent. A generic opaque HostNamespace is only
a storage partition. Its bounded host-only factory and every staged handle lack
Deserialize; a future host adapter must separately bind partitions to current
workspace/dispatch/Matrix authority. Neither hashes nor storage imply authority.

The host supplies an already provisioned and opened cap_std directory. The store
retains that actual capability and a single fixed-name private journal file;
operations never resolve caller paths. A single-owner file lock guards the held
journal. Owner permissions and Windows current-SID ACL checks reuse the existing
private-storage policy through a narrow handle-only checker. Existing path checks
retain their additional pathname/symlink/identity checks. Physical provisioning,
malicious same-user mutation and mount control are outside this primitive.

Snapshot, Encrypted and CheckedBytes are consumed as real custody. Original bytes,
kind and exact descriptor JSON are bound to the host operation ID and partition.
Retries restore original ciphertext and keys, never call encryption again. A
restored StagedMedia is a distinct copied result; it cannot impersonate a Snapshot
with original source/ancestor handles. Checked bytes still do not authenticate the
source key or sender. No secret-bearing type implements Debug or serialization.

One append-only private file contains a partition header and bounded checksummed
intent/payload/commit frames. Intent persistence precedes payload and the final
commit sync precedes its receipt. Interrupted bytes remain counted. Errors
quarantine new writes without trimming tails or inventing a replacement ID.
Reopen verifies complete records and preserves incomplete tails as unknown;
only a validated prefix is inspectable. Checksums detect corruption, not forgery.
No eviction or automatic cleanup is provided.

Sync evidence is explicit: file-and-directory sync acknowledgement is distinct
from FileSyncedDirectoryUnconfirmed. Windows may reject directory descriptor
flush; that is an inspectable unconfirmed outcome, not durable admission or a
future upload permission. The eventual transport adapter must require its own
necessary durability class. Even acknowledged OS flushes do not qualify hardware
power-loss behavior. Synchronous bounded IO can still block in the kernel; there
is no asynchronous cancellation or detached worker in this slice.

### Exact storage and recovery contract

`Store::create` uses successful relative create_new only; `Store::open` never
creates a missing entry. The one journal name is fixed in code. The header binds
the partition, and every intent binds its exact operation, kind, lengths, hashes
and previous committed frame. Duplicate operation records or corrupt complete
frames refuse reopen. Incomplete header/payload/trailer bytes are never truncated;
a valid prefix plus incomplete tail reopens quarantined and exposes only individually
revalidated committed records. Invalid initial headers have no recoverable prefix
and fail closed. The log is not a replacement for domain send/approval receipts.

Preflight failure returns the original unadmitted Media in a non-Debug
StageFailure; an existing pending object cannot be overwritten. Once append IO
starts, the Store retains the exact Media on failure and reports that custody
explicitly. This includes original snapshot/source handles and encryption keys.
Exact committed replay may drop duplicate input only after checking the same
operation digest and stored bytes. Unknown data is never re-encrypted or assigned
a replacement operation. A future asynchronous adapter must retain this actual
owner until its synchronous operation and recovery have finished.

There is an unavoidable zero-byte boundary: if even the first intent write fails
without storing any bytes, the current owner retains/quarantines its Media, but a
later process cannot discover that nonexistent record. The real read-only-file
write-failure fixture proves restart NotFound and no content/key recovery. Missing
storage is absence of evidence, never proof that upload or delivery did not happen.
The future domain adapter must record the original operation BEFORE staging and
must not turn this absence into permission to re-encrypt, retarget or resend.
This differs from a complete commit frame whose response was lost: validated reopen
and new sync recover its original bytes and exact descriptor without re-encryption.

Hard bounds are 16 MiB per item, 256 MiB per complete journal including metadata
and interrupted bytes, 128 records including one incomplete tail, and eight held
read results per Store. Defaults are 4 MiB, 64 MiB, 64 records and four results.
Each operation ID is 1..128 ASCII alphanumeric/underscore/hyphen/dot bytes; the
host namespace factory accepts 1..256 non-control UTF-8 bytes and stores only its
SHA256 partition digest. Namespace identity is exact bytes, not display-name
normalization. The 72-byte header and 192-byte intents bound framing before any
payload allocation; descriptors are at most 1024 bytes. Reopen validates with
64 KiB chunks, retaining a bounded metadata index rather than all media buffers.
Read results reserve only a checked item size and have RAII permits. Pending
upstream custody and caller-owned inputs retain their existing ADR058/061 permits;
these limits are per owner, not a claim about total host RSS or caller copies.

### Private handles and platform evidence

The permission helper reads the actual retained descriptor. Unix requires current
UID, private owner-only modes, and regular single-link files; Windows requires
the current SID and only its allowlisted ACL entries, with no reparse/hardlink
exception. Existing pathname private checks keep all their prior path tests.
Windows relative create adds WRITE_OWNER/WRITE_DAC, seals the newly created empty
regular single-link handle to current SID plus a protected DACL, and runs the
same strict checker. No existing entry is sealed or repaired; sealing failure
leaves an unavailable empty entry and writes no header or private data.

Pinned cap-primitives4.0.3 source was inspected: fs/open_options.rs forwards the
explicit access_mode; windows/fs/open_unchecked.rs passes the fixed single normal
component and original directory handle; create_file_at_w.rs sends that retained
RootDirectory and requested rights into NtCreateFile. No ambient-path branch is
entered for the fixed journal name. SetSecurityInfo borrows the actual new handle
and bounded live descriptor allocations, checks its candidate before/after, then
uses the existing strict current-SID policy. The creation-only public helper has
an explicit host precondition; empty length alone is not proof of create_new.

Every accepted result includes the actual sync evidence. Unix tests require both
file and retained-directory sync acknowledgement. Windows tests require strict
private creation and explicit file-only/directory-unconfirmed evidence if the OS
refuses that descriptor flush; no flush failure is silently upgraded. Directory
sync covers the journal entry under the host's protected namespace assumptions,
not root-parent provisioning, hostile same-user leaf moves, arbitrary mount changes
or hardware power loss. POSIX held-directory rename writes into the retained old
object; Windows requires actual ERROR_SHARING_VIOLATION32 while directory handles
remain alive, then successful rename after release. Normal file/record replay
cannot stand in for independent physical namespace or transport qualification.

Tests cover actual SDK ciphertext/key persistence, all frame interruption boundaries,
real refused OS writes, corruption/oversize before allocation, hard record/byte/result
limits, namespace/changed-operation refusal, pending and returned input custody,
real locks, hardlinks, symbolic/reparse entries and public mode/World-ACL rejection.
Windows public-ACL fixtures invoke only a fixed bounded icacls command against
fresh test-owned paths. Local macOS execution and Windows cross-compilation are
not Windows runtime qualification; hosted CI must execute those platform cases.
No Matrix upload/download, file tool, service toggle, schema migration, media
outbox, sender authenticity, cleanup or power-loss claim is introduced here.


#### Linux retained-directory access correction

NativeCI34543863628 at3b5db90 failed all seven media-store tests at initial
Store::create with OutcomeUnknown; all other Linux test targets passed. Pinned
cap-primitives4.0.3 rustix/fs/dir_utils.rs opens ambient directories with
O_DIRECTORY|O_PATH on Linux. Cloning the retained directory preserves O_PATH,
which cannot acknowledge fsync. The old result hid the specific OS error under
its static OutcomeUnknown category; source identifies the defective handle path.

Keep that retained capability, but on Linux obtain a second read-only descriptor
by opening fixed relative dot through it before any journal creation. Compare
device/inode against the still-held original and apply unchanged private checks.
Only the readable descriptor is used for directory flush. No ambient pathname is
resolved, no failed flush is accepted and no quarantine/permission is relaxed.
A real directory-rename fixture checks retained identity and the original O_PATH
EBADF before asserting corrected fsync and absence under the old path replacement.
Actual Linux execution of that fixture is required; local macOS and compilation
do not replace it. Windows FileSyncedDirectoryUnconfirmed remains unchanged.

## Consequences

The store preserves exact object ownership, recovery state and platform flush evidence. Windows directory-sync uncertainty and hardware durability limits remain explicit and cannot be upgraded by a digest.

## Alternatives Considered

Resolving caller paths or treating a storage partition as authorization would bypass host capability selection. Accepting a failed directory flush or dropping journal history at capacity would falsify durable staging evidence.
