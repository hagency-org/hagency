# Native selected-resource domain checkpoint

Scope: REQ-RUST-MIGRATION-EXECUTION, ADR-025/028 and
`specs/task-rust-domain-allocation.spec.md`, in the isolated migration worktree.
This is a partial M2 implementation. It does not complete the full migration.

| Requirement slice | Implementation/evidence |
| --- | --- |
| Unicode and stable scoped identity | 38 vectors from the actual JS project-definition module; Cargo identity comparison |
| Exact authenticated-observation checks | Full sender/source content/reception/project binding/power/private room checks; 16 negative observation mutations, stale clock/generation and project-remapping tests |
| Durable manual-review intake | Unique content-bound request row, normalized project name collision guard, private evidence and binding in one commit; pending requests hold no tokens |
| Pool and shared-seat capacity | Existing29 budget vectors plus concurrent approvals through the domain worker; other-pool commitments only limit a declared shared seat |
| Atomic approval/outbox | SQL trigger abort at outbox insertion proves reservation and decision rollback; identical command replay never reserves twice |
| Recovery and revocation | Started effects reopen as uncertain; stale fences fail; revoke retains cleanup intent and history; failed retirement requires explicit retry |
| Safe operator resource edits | Authenticated Salvo create/list/withdraw/republish and private configuration listing; omitted publication on edit preserves withdrawal |
| Process restart without Node | Real native subprocess runs with PATH empty, stores a resource and custody receipt, is killed and reopens both successfully |

Private evidence is adapter-only. Observation structs have no Deserialize;
`VerifiedRequest` has private construction and the development API has no request,
approval or effect-completion endpoint. A future Matrix adapter must authenticate
network observations before invoking the pure verifier. A receipt string from a
fixture is not a real account creation/deactivation observation.

Open M2 work: qualification discovery and cross-family/global-role rules, legacy
role-only allocations, project-side management, owner/registration rotation,
retry of definitively failed provisioning and production wire/API integration.
M3 canonical tasks/dispatch and M4 real platform-owned runners are next. M5–M9
transport, permissions/files, console parity, release and cutover remain open.

Validation at this checkpoint is recorded in the working progress log and PR162.
The previous commit's green OS matrix does not automatically validate this change.
Native CI must run these new domain and identity tests on Windows/Linux/macOS.
