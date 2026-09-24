spec: task
name: "Consume the original inline factory owners and derive the activated session"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, provisioning, runtime, matrix, custody]
---

## Objective

Connect the existing original inline home/account/rooms/enrollment job to the
existing owned WarmRuntime, genuine Applied/Active acknowledgment and derived
session_{engagement_id} route. The original initialized process/reactor and SDK
must be the owners consumed by the agent's first real dispatch. No fixture
activation may qualify the target engagement. Complete deployment profiles,
AS receiver/registration generation, native fleet, effective sandbox, canonical
completion/recovery, live two-agent and sustained Palpo/Robrix qualification
remain required by the full goal, not replaced by this implementation contract.

## Constraints

- Concrete Host-only WarmHostPlan, no serde/Debug/Clone, generic callback or
  public ready/physical-proof setter. It prepares the one original per-agent
  Host from the original materialized home and writer-produced provision scope.
  WarmTaskBridge retains its fixed native helper/loopback origin/private context
  directory as one owned capability, without getters or replacement inputs.
- Use the original account registry association, frozen resource and current
  readiness. Do not reopen the writer, borrow an unrelated account, export
  credentials/configuration, duplicate an SDK owner or introduce a launcher.
- Explicitly approve the internal path dependency hagency-matrix ->
  hagency-execution for this concrete Host bridge; no external crate/version.
- Capture the original initialization deadline before writer/preparation delay.
  Reuse the fixed app-server launch, original bounded worker/reactor/command,
  sticky warm claim, physical Ready observation and retained task-context bridge.
- Final original SDK and warm/home/account checks precede activation. The
  writer validates exact original scope/registration/fence/payload/Started/
  Reserved/account and invokes the shared existing effect kernel atomically.
  Lost acknowledgment cannot authorize a route, dispatch or success replay.
- Active SDK handoff retains its original enrolled owner. Resolve the derived
  null-root session from actual created owner-DM identity and current Collector
  observations, not seeded agent/current rows. The original workspace is the
  actual home workdir. First dispatch consumes the retained WarmRuntime once.
- The concrete factory requires the original configured approval Collector.
  Validate its fixed bot/registration/endpoint and original writer scope before
  launching. After the exact Active ACK, authenticate the target's protected
  approval room through that same Collector before publishing a route. Never
  seed a target approval binding, rebind a foreign project's room or disable
  native approvals. A failed post-ACK observation retains Active history but
  cannot yield a route or dispatchable agent. Dynamic fleet/approval pumping
  remains a separate required integration gate.
- Original jobs survive caller loss; failures and partial/unknown jobs cannot
  rearm. Post-activation failure never submits backward Unknown or fabricates
  success. Cleanup cancels/joins on an ownership worker, not an HTTP/UI worker,
  and does not certify whole-tree stop or release unknown capacity/leases.
- Factory shutdown admission retains the original coordinator busy permit in
  an owned job through caller loss. Drain every finite retained agent even if
  another close fails; partially failed admitted shutdown reports Unknown,
  not a known pre-effect Busy. Each individual owner's exact result remains
  retained. Close each retained agent's dispatch admission before drain awaits;
  a busy SDK cannot authorize new work after admitted shutdown. Coordinator
  Busy before admission changes no owner.
- First-dispatch admission is sticky before queuing its ownership-worker
  handoff. Failed handoff/Operation drop must join only on that blocking worker,
  never an async caller. A lost queued waiter cannot select a second capability
  or rearm the original warm owner.
- Defaults, existing checkpoint markers and IO/deadline/live/parked budgets are
  unchanged. No cold fallback, synthetic Done, shared private fixture keys,
  live Cargo tests, formatter, commit/PR, reset, deploy or cutover.

## Allowed changes

- ./Cargo.lock
- native/hagency-matrix/Cargo.toml
- native/hagency-matrix/src/provisioning.rs
- native/hagency-matrix/src/provisioning/factory.rs
- native/hagency-matrix/src/collector.rs
- native/hagency-matrix/src/approval_intake.rs
- native/hagency-matrix/src/lib.rs
- native/hagency-execution/src/factory.rs
- native/hagency-execution/src/host.rs
- native/hagency-execution/src/warm.rs
- native/hagency-execution/src/lib.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain/provision_runtime.rs
- native/hagency-store/src/domain/accounts.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/provision_runtime.rs
- native/hagency/tests/inline_factory.rs
- native/hagency/tests/inline_factory/**
- knowledge/decisions/adr-053-native-owned-dispatch.md
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- this spec, docs/agent-knowledge.md, docs/progress.md

## Scenarios

Scenario: Final activation validates the same original scope in the effect transaction
  Test: native_provisioning_original_activation_scope
  Given the actual original Started scope and sticky warm claim
  When registration/resource/account/producer/state changes before activation
  Then the shared kernel refuses without writing Active; this writer-only test is not physical factory proof

Scenario: Inline approval consumes physical owners before completing its own effect
  Test: native_provisioning_effect_completed
  Given actual Matrix intake, original home, new account, joined rooms and enrolled SDK
  When the original native owner initializes and is freshly qualified
  Then the product itself acknowledges Applied/Active with no target fixture activation

Scenario: Genuine activation creates its exact derived owner-DM session
  Test: native_provisioning_session_route
  Given the actual inline factory and independently joined encrypted owner DM
  When the original SDK hands off after acknowledged activation
  Then exactly session_{engagement_id} binds that actual DM and actual home workspace

Scenario: First dispatch consumes the initialized original owner
  Test: native_provisioning_factory_first_dispatch
  Given the genuine factory's same SDK and initialized native owner
  When a real scoped task dispatch uses the native task helper
  Then the original process initializes once and maintains canonical task state through the original acknowledged worker path

Scenario: Failed final authority cannot fabricate activation or rearm owners
  Test: native_provisioning_factory_refusals
  Given the actual retained factory at its final physical checks
  When current owner/membership/registration is lost
  Then negative custody remains original and no replacement launch/Applied/route appears

Scenario: Failed authenticated approval binding cannot publish an activated route
  Test: native_provisioning_factory_approval_refusal
  Given genuine activation and the original configured approval Collector
  When its authenticated private room loses its joined owner
  Then activation history remains exact, target approval authority and routing are absent, and no replacement initialize appears

Scenario: Losing the original intake waiter does not replace its factory owners
  Test: native_provisioning_factory_waiter_loss
  Given the original factory initialized once and its final SDK read is retained
  When the outer intake waiter is dropped before that original read completes
  Then the accepted original job can finish with the same owner/SDK and exact route, never a second launch

Scenario: A matching bot configuration cannot substitute a foreign writer
  Test: native_provisioning_factory_foreign_approval_writer
  Given a configured approval Collector with the exact bot/endpoint/registration tuple but a different producing DomainStore
  When the actual factory presents its original Started provision scope
  Then the original owner check refuses before native initialize, retaining uncertain effect custody without a route

Scenario: Losing an admitted shutdown waiter cannot release its original permit
  Test: native_provisioning_factory_close_waiter_loss
  Given a genuine factory and its retained original warm/SDK owners
  When the admitted shutdown waiter is dropped before its owned job runs
  Then original coordinator work remains Busy until actual closure finishes, closed authority cannot dispatch, and no owner is replaced

Scenario: Coordinator Busy refuses shutdown before touching the factory
  Test: native_provisioning_factory_close_busy
  Given the original coordinator holds its busy permit for an actual authenticated read
  When factory shutdown is requested
  Then known Busy leaves the original warm/SDK authority current and the same admitted read can finish

Scenario: A busy agent SDK retains negative admitted shutdown custody
  Test: native_provisioning_factory_close_agent_busy
  Given the genuine agent's original SDK holds its busy permit for an actual authenticated read
  When the coordinator admits finite factory shutdown
  Then aggregate OutcomeUnknown cannot reopen dispatch admission, the original read may finish, and later closure drains only that retained owner

Scenario: Failed or lost dispatch waits retain the original ownership-worker handoff
  Test: native_provisioning_factory_dispatch_custody
  Given the genuine initialized owner and a controlled single blocking-worker fixture
  When failed admission is queued and its original waiter is retained or lost
  Then the async caller never joins that owner, a second attempt is refused before queuing, and original closure performs no task IO or replacement initialize
