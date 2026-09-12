---
kind: decision
id: ADR-096
title: "Bootstrap one owned development attempt after authenticated Matrix refresh"
status: Accepted
tags: [rust, bootstrap, execution, workspace, development]
---

## Context

Native serve currently constructs the protected writers and Salvo App, but has
no owned execution driver. ADR093 supplies a sealed StartedWorkspace after a real
Started acknowledgement. Its consumer-free Operation immediately continues toward
child launch; polling that slot does not ensure registration precedes the first
MCP request. ADR092's proposed file service requires a real same-process entry
path, not a fixture that constructs a capability or directly calls Operation.

Reopening DomainRepository recovers Leased/Started/Parked attempts and reconciles
invalid routes. It does not blanket-clear every available Matrix transport;
Collector::close explicitly fences its transport. Regardless of persisted
availability, a new development driver must authenticate its CURRENT configured
token/account/device and observe the full room state through Collector before
claiming. An already fenced generation remains unavailable; startup must not
rotate a generation, remap a SID or change SQL flags to make old work runnable.

This prerequisite preserves the accepted host-exclusive stable workspace and
ancestor contract from ADR053/058/093. It does not require hostile same-UID
namespace isolation or claim that fixed Codex tool paths remain anchored after
an unsupported ancestor replacement.

## Decision

### Independent Palpo service profile

The approved ADR109 service partition adds a separate --palpo-transport opt-in.
Without either startup flag the service stays passive. Palpo configuration does
not require a development runner, executable digest, Matrix SDK token or workspaces.
Bootstrap validates each selected fixed private profile before constructing its
original Store and DomainStore. The additive open_with_options API preserves
open's existing development-only behavior for current callers.

After binding and polling the real server, Bootstrap synchronously retains one
original Palpo worker join before its next await. That task verifies the existing
canonical registration, attaches to the original custody Store and awaits the
adapter's joined loops. Close cancels all original owners before awaits and joins
this worker before closing either writer. The existing two-second close observation
bound retains an unfinished join; cancellation of the close caller does not discard
it. A consumed result is retained rather than awaited twice. Fatal adapter status,
peer publication acknowledgment and known task completion remain distinct. A failed
worker join is explicit Unknown. Abrupt owner abandonment requests cancellation;
it is never a successful close acknowledgment.

Add a shared non-test Bootstrap used by the real hagency serve command. An explicit
--development-driver flag selects the fixed private development-driver.json beneath
the fresh state directory. Without that flag owned development execution remains
disabled; the independent Palpo profile above can still run when explicitly selected.
The closed configuration is at most 16 KiB, rejects duplicate/unknown fields, and
describes exactly one attempt per service start. No automatic claim/retry loop or
continuous scheduler is introduced. Canonical Done follow-ups and recovery reports
remain queued for their existing general admission paths; the current owned
operation cannot execute them, so this profile must not consume them.

The profile selects one configured Matrix account/SDK identity, at most 16 encrypted
room scopes and at most 16 already-private workspace roots. Token, SDK key and
optional local TLS CA are fixed private state children. The endpoint is a validated
HTTPS origin. Configured identity is an expected identity, never a claim that
credentials or membership have already been verified. Exact transport/room
generations are fixed; unavailable old generations refuse rather than rotate.

Runtime selection is a private configuration-owned absolute installation path with
its expected bounded SHA-256 digest and the fixed codex-app-server development
protocol profile. The guardian and MCP helper are this same native executable.
Arguments remain app-server and mcp; model/reasoning remain resource-owned.
Configuration cannot supply runtime arguments, arbitrary environment entries,
sandbox/approval overrides or a runner capability. The initial environment uses
only the fixed fresh runtime-home paths and required platform SystemRoot. A test
peer needs no alternate Bootstrap or test-only route: the executable fixture
provisions its fixed peer installation through this same host profile. Hash/path
checks detect installation inconsistency under the trusted-ancestor contract;
they are not executable-handle custody or real Codex sandbox qualification.

Bootstrap exclusively owns the existing Store and DomainStore, one Collector,
one HostDriver and one WorkspaceAccess slot. App receives a bounded status handle.
The real router/socket must be ready before the driver can start a child. Startup
errors close or retain the already-created owners in order; they do not leave a
hidden executing service. SDK operations use the existing Collector-owned finite
tasks and cancellation semantics, including retained negative observations.

The driver first completes actual Collector whoami, SDK sync/full state and each
configured full room observation. Failure produces a fixed unavailable or unknown
status and performs no claim/child launch. A success is not remote-atomic ongoing
permission: later authoritative domain invalidation still fences the operation,
and this slice does not add continuous Matrix event ingestion.

The new host-only OwnedClaimProfile is bounded, has no Deserialize/Debug authority
format, and records the exact expected transport identity, configured encrypted
room generation/privacy and workspace IDs. DomainStore::claim_owned_dispatch_for_host
adds these compatibility checks inside the current claim transaction while
retaining all task, scope, lease, unresolved-work and resource guards. It admits
only a non-Done canonical-task dispatch without a recovery-report grant on a
current verified Matrix route, the supported
Codex resource profile and exactly one configured exclusive workspace. The clock
is sampled after the worker queue AND BEGIN IMMEDIATE acquisition. Existing
claim_dispatch behavior is preserved for its other callers.

The claim returns its newly random RunnerCapability once to the same host process.
A lost/unknown claim result stops this one-attempt driver. There is no second claim,
raw secret restoration, repeated Started request or serialized workspace authority.
No compatible work produces an explicit no_work terminal status; unsupported
queued work is not claimed and requeued in a loop. The driver uses the original
writer and actual Operation for scope freezing, Started, usage, runtime IO and
cleanup. It owns the Operation and retained Report on one fixed OS thread, keeping
blocking join/drop away from HTTP handlers. Existing operation/response/DB limits
remain unchanged.

Add an opt-in Operation::start_requiring_workspace; ordinary Operation::start and
its existing handoff behavior remain unchanged. The required mode exposes one
WorkspaceRegistration after the real Started ACK. Splitting it yields the sealed
StartedWorkspace and one non-clone LaunchAck. WorkspaceAccess::register consumes
the original binding and capability, validates through the binding's original
writer, stores it in the single bootstrap-owned slot, and only then permits the
trusted driver to acknowledge registration. No callback or additional unbounded
queue runs on the execution worker.

The worker waits for that one acknowledgement before any child creation, under
the original absolute operation deadline and cancellation flag. Missing, dropped,
late or lost ACK means no child and no reissue of Started or the handoff. After
registration and other required awaits, the worker checks original current
scope/lease and the retained root again immediately before launch. Registration
orders local ownership; it neither creates execution authority nor overrides
revocation. All exit/unwind/drop paths retain ADR093 retirement and Report custody.

WorkspaceAccess is a cheap host-only shared handle to one slot. It exposes no
capability, path, raw Workspace or route and has no runtime HTTP setter. A future
FileService may perform a bounded exact-capability lookup yielding an opaque
access guard. The guard retains the actual StartedWorkspace and invokes its
original-writer validation around capture. This prerequisite exposes no source
read endpoint, MCP file tool, upload or Matrix file publication. The driver never
clones/reopens Host profiles for another run, so source/result budgets cannot reset
through repeated attempts.

The result channel owns one Box<Report>; wait_boxed keeps that same allocation
through the bootstrap's nested async return/selection path. Existing wait still
returns Report for established callers. A real macOS debug executable exposed
stack overflow in the old by-value Report/oneshot polling path; boxing the outer
future alone did not resolve it, while this single retained result did. No thread
stack, deadline or process ownership budget was increased. The read-only
retains_process_custody getter describes local physical ownership only: a known
pre-child Pending result differs from unknown spawn or incomplete stop evidence.
It never authorizes lease release or canonical completion.

The fixed status projection distinguishes disabled, refreshing, claiming,
registering, running, completed, no_work, unavailable, outcome_unknown and closed.
It contains fixed stage/error enums and registration presence only, never paths,
IDs, tokens, model text or raw errors. completed means the owned development
attempt returned its known result, not canonical Done or file delivery. The precise
protocol/cleanup/settlement categories remain separate. agent_execution and
production_api_parity remain false; the one-attempt development status is separate.

Shutdown stops new driver/registration admission, signals the current operation,
retires new source access, and keeps unresolved Report/physical roots until owned
cleanup is settled or explicitly retained as unknown. There is no automatic next
attempt. One OS owner retains any unresolved report; shutdown failure cannot
consume the last owner and pretend release. Close the Collector/SDK after execution
ownership is quiescent, then close the domain and custody writers last. No timeout
increase, forced thread abort, detached last-owner cleanup or silent retry is added.

## Consequences

Positive: the service has one real, testable claim-to-registration-to-child path.
Negative: one-attempt mode deliberately cannot schedule subsequent tasks, and
unproven process cleanup keeps shutdown unavailable rather than releasing custody.

This creates the actual main-to-driver-to-Started-workspace registration seam for
one development attempt. It does not implement ADR092 send_file, delivery schema,
file workers, Matrix file events, timeline scheduling, project onboarding, runtime
approval application or production activation. Real Codex/provider/sandbox and
platform release qualification remain separate from a native offline peer.

The mandatory executable fixture runs init, provisions legitimate domain state
with only a Queued dispatch, closes its setup owner, and starts the actual serve
binary. It uses the same Bootstrap, a real local TLS Matrix peer and actual MCP
executable. It must prove whoami/full room refresh precedes claim, original fresh
capability issuance, registration precedes child startup, and a genuine canonical
task read/heartbeat reaches the original writer. It never restores availability
or Started with SQL. Fenced generations, wrong current token/device and unsafe
room state must prevent launch. No positive fixture is rescued by generation
rotation or SID remapping.

Separate deterministic real-worker tests pause registration, exercise failed/lost
ACK and cancellation, preserve no-child assertions, and verify one-slot/Report
retention. Compatible-claim tests use real SQLite transactions, queue/lock-time
clocks, foreign and unsupported queued work, real lease conflicts and receipt
loss. No unimplemented selector, platform refusal or cross-compilation is reported
as a passing executable workflow. The implementation validation is recorded in docs/progress.md: actual macOS
executable and affected tests, strict scenario/boundary validation and warnings-denied
native/Windows GNU Clippy. Hosted runtime qualification remains independent.

The concrete macOS development profile can finish its typed protocol and actual
MCP task heartbeat while whole-tree cleanup remains unknown under the existing
platform contract. Normal SIGTERM then deliberately retains the original service,
result and both writer locks and exposes outcome_unknown. The Unix executable
fixture checks that behavior on macOS and confirmed stop/reopen on Linux. Windows
has no Unix-signal fixture; shared binding/lost-reply tests and actual executable
startup are still independent Windows obligations. GNU cross-compilation is not
Windows execution evidence. No macOS unknown cleanup is called a complete shutdown.

The existing DomainStore/Store shutdown Result can become unknown independently
of their final worker fate. Bootstrap retains the original wrappers and reports
that uncertainty; wrapper presence does not prove a repository is still open,
and a timeout does not prove it closed. The driver retains its original pending
close receipt. There is no automatic replacement claim or shutdown retry.

## Alternatives Considered

- Seed a runner secret or invoke Operation directly in the integration fixture:
  bypasses actual bootstrap claiming and cannot qualify the service entry path.
- Trust persisted availability or restore flags after reopening: does not
  authenticate current credentials and can revive a deliberately fenced route.
- Poll the optional workspace slot while launching: first MCP call can precede
  registration. Explicit required ACK removes that race without altering existing
  consumer-free Operation callers.
- Add a continuous multi-agent scheduler now: broadens retry, source-budget and
  recovery ownership beyond this prerequisite. One attempt per start is explicit.
- Require privileged mounts everywhere: strengthens the accepted trusted-host
  profile unnecessarily and still would need separate real Codex qualification.
- Call this a completed send_file workflow: source association/registration is
  only a prerequisite; actual durable file delivery remains ADR092/097 work.
