# Original native startup boundaries

Original Native run 34582891529 at c677ce0 fails Ubuntu and Windows Cargo.
The full unmodified logs, exact Cargo slices, original metadata and SHA256
manifest remain in the external 2026-09-10 migration cache, documented in
`c677ce0-955463a-original-ci-evidence.md`. Later diagnostics are separate.

All four original Ubuntu file-service children remain running with empty stderr,
zero HTTP requests and no service-ready log at their unchanged 10.9-second
request or 15-second startup watchdog. This precedes any observed Matrix request;
it does not establish that the SDK or SQLite close was entered. The three failed
Windows executable children have reached server readiness and 29 original HTTP
requests, ending at room state before FileService startup refuses. The independent
atomic-fresh-directory test returns Unavailable before Store errors could map to
Unknown. Neither original log identifies its underlying backend failure.

Before readiness, the native executable constructs its Tokio runtime, validates
private state/configuration, verifies the configured executable, opens custody
and domain repositories and workers, creates Shared/FileOwner/App, binds the
listener, initially polls the server and starts its Driver. Executable verification
includes canonicalization, metadata checks and an actual bounded SHA256 read loop.
An absent ready log cannot distinguish these operations. In particular, expensive
debug executable hashing is a hypothesis, not an observed original cause.

The fixed TRACE-only `hagency_startup_observation` target records boundaries on
these original paths. Hash entry/completion surrounds the actual read/digest work
inside executable verification. Separate media markers distinguish atomic create,
private validation, directory open and Store open without replacing their errors.
Private-policy refusal is distinct from other original private errors; it does
not report a SID, ACL, filename or underlying backend text, and does not establish
which private check refused.

Default logging does not enable TRACE. Only the disposable original child fixture
sets the new target to TRACE alongside its existing info logging. Its existing
retained stderr reader still reads at most 8193 bytes, with the existing truncation
flag. Two closed static projections report the last observed bootstrap phase and
media-worker phase independently. No raw output is interpolated. Missing or
truncated phases remain unobserved, never successful startup, closure, ownership
release or retry authority. These are observations from the original child output,
not a global latest-process map or a diagnostic rerun replacing that child.

The actual held-whoami test verifies original server startup, with the existing
partial order allowing Driver request publication before the parent logs serving.
A separate actual invalid-configuration child exits with its original Config
error; replacing its stderr pathname cannot change its retained observed phase.
The actual missing-journal child serves HTTP and Matrix setup but retains the
original outcome_unknown refusal, zero task attempts and no journal repair. Its
bootstrap phase is serving while its separate media phase is store_refused.

No deadline, retry, native filesystem policy, crypto operation, production
authority, worker ownership or success criterion changes. New local diagnostic
passes qualify observation behavior only. The original Linux stall and Windows
startup refusal remain open until actual platform evidence identifies their cause.
