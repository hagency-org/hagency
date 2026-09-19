spec: task
name: "Synchronize retained private directories using a usable directory handle"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, linux, custody, filesystem]
---

## Objective

Fix Linux factory home/context publication failures caused by syncing the
O_PATH descriptor returned by cap-std directory opening. Reuse the account
store's existing readable same-object directory synchronization on Unix and
its existing WindowsDirectorySync implementation on Windows.

The Linux whole-Matrix regression also found nested home copies failing before
registration: chmod on a cap-std O_PATH directory descriptor is invalid too.
Create nested directories with private mode atomically, retain/check their real
handles, and synchronize each actual copied directory's entries, not only the
top-level target. Do not suppress permission/sync errors or widen copy limits.

## Constraints

- Open dot relative to the retained directory, never its ambient pathname.
- Preserve private permissions, original object identity, and real sync errors.
- Do not suppress EBADF, skip durability, adopt partial files, extend deadlines,
  or turn successful synchronization into runtime/dispatch/cleanup authority.
- Account synchronization keeps its existing behavior. Home copies and original
  task-context records use the same internal helper; no public proof setter.
- Tests remain offline. Linux qualification uses isolated synthetic state only.
- No live deployment/restart/reset/re-soak, dependency, formatter, commit or PR.

## Allowed changes

- native/hagency-store/src/private.rs
- native/hagency-store/src/domain/accounts.rs
- native/hagency-store/src/agent_home.rs
- native/hagency-store/src/task_context.rs
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- this spec, docs/progress.md, docs/agent-knowledge.md

## Scenarios

Scenario: Sync uses the original private directory even after namespace replacement
  Test: native_private_retained_directory_sync
  Given a retained private directory and a different replacement at its old name
  When synchronization runs using that retained object
  Then a usable same-object descriptor is synced and public permissions still refuse

Scenario: Real copied factory homes and retained context permit Linux dispatch
  Test: native_provisioning_factory_sequential_dispatch
  Given actual offline factory ownership for both Matrix account kinds on Linux
  When copied home and original context publication complete and two dispatches run
  Then the real helper sees the correct distinct tasks with original authority preserved

Scenario: Nested copied directories are private and their original entries sync
  Test: native_agent_home_nested_copy_private
  Test: native_provisioning_inline_home
  Test: native_provisioning_inline_appservice
  Given actual retained source and target directories with nested files
  When the bounded original home copier runs on Linux
  Then nested creation, exact bytes and private permissions succeed without chmod on O_PATH or skipping directory synchronization

Scenario: Original task context publication and refusal semantics remain unchanged
  Test: native_retained_task_context_bind
  Test: native_retained_task_context_refusals
  Given real writer-produced Started scope and original private context custody
  When publication or changed-scope replay runs
  Then only the exact original context can succeed and refused custody cannot rearm
