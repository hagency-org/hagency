---
kind: context
id: CTX-NATIVE-FILE-OWNED-OBSERVATION
title: "Retain the original Windows file attempt's runtime and helper observations"
status: Accepted
---

The bounded task is `specs/task-rust-file-owned-observation.spec.md`, based on
b211301. Its observation-only amendment follows accepted ADR053 and ADR101.

Original Native run 34629952845 at b856b47, Windows job 103364054653,
failed its original Cargo workspace step. The FileService target recorded four
passing tests and one failed `native_file_service_executable`: its first group
iteration failed, so that test did not execute its later positive DM iteration.
The exact original status was outcome_unknown, owned_attempt, protocol unknown,
negative settlement and whole_tree_stopped. Upload had been observed, with five
key writes, one claim and one share; its final fixed HTTP category was room_event.
At the original diagnostic snapshot the service child was still running, bootstrap
was serving and media was store_ready. The 5,634 ms observation starts before the
service spawn, not at owned-operation entry. It cannot identify a transport call.
The fixture's last HTTP category is recorded before its response; it does not
independently prove an accepted room event or Delivered receipt.

The exact source maps the fixture's response_ms=1500 to transport event_wait_ms.
Its offline app-server peer emits no event while the actual native MCP helper
does send_file, bounded get_file_delivery reads, get_task and helper shutdown.
This silent interval is a source-derived timeout candidate, not the established
cause of that original failure. EOF, helper refusal and lost authority remained
indistinguishable in the collapsed operator status. Original private helper
receipts and full runtime stderr were not uploaded. The later transport diagnostic
was cancelled while compiling at the job deadline; it is not another original
Cargo test failure and does not replace the failed original.

Report now freezes an observation from the same runner immediately after drive
returns and before coordinator stop/removal. The existing runtime error guard can
already have stopped it; the first transport termination still survives. Its
stage is the last entered Initialize, ThreadStart,
TurnStart or Update operation. Typed session and first transport termination
causes are distinct, including possible host cancellation. Pending counts and
original unfinished-write accepted/total bytes are optional. They never identify
the blocked syscall or establish whether native core applied a request. No raw
request ID, payload, error message, stderr or path enters the projection. Report
keeps this original value when later stop removes a proven stopped owner.

The existing operator-only status adds owned_failure and runtime fields while
preserving original state/error/protocol/cleanup/settlement. The fixture reads
only existing admission, read-error, delivery, task and receipt files under its
private stable workspace. Reads are capped at 8,193 bytes, with content over 8,192
explicitly oversized. Parsing emits only closed status labels. Missing, malformed,
unsupported, read-failed and preexisting receipt states do not fabricate progress.
The private context file is never read. These receipt observations occur in the
original panic snapshot and keep that original failure and child cleanup intact.

The same b856b47 Windows run separately reported actual directory-creation facts:
default_owner_is_user=false, default_acl_is_private=true,
explicit_owner_is_user=true and explicit_dacl_is_protected=true. This is actual
Windows evidence for the prior atomic creation fix, separate from the earlier
22c4993 failure whose specific owner predicate was not observed.

Local regressions use actual silent and EOF owned children, an actual original
service startup/refusal, and bounded receipt/projection tests. macOS retains its
existing unproven whole-tree custody; platforms with proven whole-tree stop also
check survival after owner removal. Local results and cross-compilation cannot
qualify this new observation on Windows. A later original hosted run is needed
to determine its own failure cause. No production timing fix is claimed here.
