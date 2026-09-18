spec: task
name: "Connect the original physical account step to verified inline approval"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, native, matrix, provisioning, custody]
---

## Objective

Continue ADR-147's full inline physical factory by connecting its implemented
registration-token account step to the production approval caller and private
native bootstrap configuration. The final factory still owes both ADR-016
profiles, managed home/template, rooms, SDK enrollment, runtime and routes.
Account observation is not full provisioning and must not close the two missing
physical-completion/session-route selectors or make the console roster green.

## Constraints

- Only the already verified representative verdict and canonical approve result
  may enter this Host-owned step. Claim exactly provision_{engagement_id}, inline.
- Before that physical step's approval, fetch the actual verify-only project
  authority again; its cached request snapshot cannot authorize account creation.
- Capture one immutable current registration/origin/credential/key/root profile.
  Keep secrets out of JSON driver documents, domain, console and runtime input.
- Use only the bounded private registration-token primitive; no effect worker,
  registration retry, account adoption, appservice impersonation or sandbox change.
- Recheck the exact original Started effect, payload, fence, Reserved engagement
  and registration through the original DomainStore immediately before each
  registration POST and before accepting the resulting account observation.
- Record the admitted job before claiming. Retain observed opaque accounts and
  failed/unknown jobs under a finite non-evicting Host registry. A dropped intake
  receiver must not discard the admitted owner. Claim uncertainty cannot relaunch.
- Replayed observed account steps perform no POST and grant no new room, SDK,
  dispatch or route authority. Restart cannot reconstruct an unacknowledged claim.
- On an account-step failure retain original effect uncertainty, never NotApplied
  based on timeout or synthesize Active/Applied. On account success leave the
  effect Started until the remaining physical factory is implemented/observed.
- An absent explicit private account-step profile retains the current truthful
  pending-effect behavior. This development checkpoint is not production cutover.
- Cargo tests remain offline. No live server, credential, state or client writes
  are part of this task's automated tests. Do not run formatters or commit/open PR.

## Allowed changes

- native/hagency-matrix/src/provisioning.rs
- native/hagency-matrix/src/token_provision.rs
- native/hagency-matrix/src/config.rs, lib.rs, intake.rs
- native/hagency-matrix/tests/intake/provisioning.rs
- native/hagency-store/src/domain/verified_ingress.rs, domain_worker.rs
- native/hagency-store/tests/domain.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency/tests/bootstrap.rs, native/hagency/tests/bootstrap/
- this spec, ADR-147's implementation checkpoint, docs/progress.md

## Scenarios

Scenario: Verified approval performs the original physical account step inline
  Test: native_provisioning_inline_account_observed
  Level: integration
  Test Double: local TLS Matrix peer; actual Collector/DomainStore/private encrypted files
  Given an observed request and representative verdict with an explicit private Host profile
  When the production approval path approves and claims the exact effect
  Then registration and returned-token whoami physically execute inline, an opaque account is retained, the effect remains Started, and no Active state or session route is fabricated

Scenario: A retained successful account step is not repeated on verdict replay
  Test: native_provisioning_inline_account_replay
  Level: integration
  Test Double: local TLS Matrix peer; actual Collector/DomainStore/private encrypted files
  Given the original inline account step has been observed and retained
  When the same representative verdict is replayed
  Then no account registration or replacement occurs and the same fence and private custody remain

Scenario: An uncertain account attempt remains spent under replay and restart
  Test: native_provisioning_inline_account_unknown
  Level: integration
  Test Double: local TLS peer with admitted lost response; actual original private files and domain writer
  Given the original registration POST has no conclusive response
  When intake resumes or a fresh Host sees the retained effect
  Then the original effect is Uncertain, no second POST occurs, and no completion/route is invented

Scenario: Private profile and non-representative input do not confer account creation
  Test: native_provisioning_inline_account_refusals
  Level: integration
  Test Double: local TLS peer, synthetic malformed Host profile and forged verdict; actual domain writer
  Given a wrong profile binding or a verdict outside representative authority
  When configuration is constructed or intake verifies the verdict
  Then it refuses before any account registration or claim

Scenario: Actual current writer scope is required at each account write boundary
  Test: native_provision_account_current_scope
  Level: integration
  Test Double: actual SQLite Immediate transaction and canonical claimed effects
  Given an original exact Started effect and registration
  When fence, payload, engagement state or registration changes
  Then the original current-scope check refuses without canonical mutation

Scenario: Revocation during the actual UIA exchange prevents another registration write
  Test: native_provisioning_inline_account_scope_change
  Level: integration
  Test Double: local TLS Matrix peer and actual canonical revocation through DomainStore
  Given the original first POST is admitted and the real engagement is then revoked
  When its token-only UIA challenge arrives
  Then current original-writer validation refuses the actual second POST and retains the original custody

Scenario: The admitted inline owner survives loss of the outer intake receiver
  Test: native_provisioning_inline_account_owner_drop
  Level: integration
  Test Double: held real local TLS POST, actual Collector/DomainStore/private custody
  Given an account POST is admitted inside the owned inline intake job
  When the outer receiver is aborted
  Then the same owner completes returned-token whoami and retains the account, while competing intake remains Busy and no new claim or POST is launched

Scenario: Fresh approval authority is required before the physical claim
  Test: native_provisioning_inline_account_authority_changed
  Level: integration
  Test Double: actual local TLS project-state change between request and approval
  Given the admitted request's project snapshot was valid
  When the fresh approval snapshot changes its owner binding
  Then verification refuses before approve, account claim or registration POST

Scenario: Retained uncertain Host jobs consume finite non-evicting capacity
  Test: native_provisioning_inline_account_capacity
  Level: integration
  Test Double: real original DomainStore claim calls returning no claim; no synthetic completed agent
  Given sixteen admitted Host jobs retain claim uncertainty
  When another account job is admitted
  Then capacity refuses before claim/network, while the original jobs still replay their retained uncertainty

Scenario: Native bootstrap loads only an explicit protected account-step profile
  Test: native_bootstrap_token_provisioning_profile
  Level: integration
  Test Double: actual private configuration/key/token files and native bootstrap preparation
  Given an explicit registration_token_account_step_v1 profile
  When bootstrap reads its separate protected credentials
  Then the fixed Host capability is attached without exporting credentials or changing default driver behavior
