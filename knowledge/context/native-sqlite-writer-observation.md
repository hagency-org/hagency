# Native original domain writer observation

The preserved 22c4993 Windows close interval has four actual same-connection
CLOSE entries and no connection-drop completion before the original reply
timeout. The panic thread identifiers belong to callers; they do not identify
the dedicated writer. Source review and the exact original artifacts are retained
externally in 22c4993-sqlite-close-source-audit.md and its hash manifest.

The bounded observation captures a real query-only handle on the actual writer
immediately before ConnectionDropStarted, in the same Probe as the original
shutdown. Its baseline includes the later SQLite CLOSE callback and cleanup,
but is not a post-CLOSE-only measurement. The existing snapshot reads that owned
handle once. The frozen result is either native PID/TID plus checked kernel/user
CPU deltas, unsupported/unobserved, or an unavailable stage. No later query may
replace an unavailable result or attribute subsequent writer work to this attempt.

[GetThreadTimes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreadtimes)
requires query or query-limited rights and returns cumulative CPU duration in
100 ns units. The implementation subtracts checked counters before flooring to
microseconds. It does not read the API's exit-time output, which is undefined
for a live thread. Native CPU duration does not identify a particular IO call,
mutex, sleep or scheduling cause.

The handle is duplicated from the current writer, retained through original
snapshot and freed through OwnedHandle. No numeric thread ID is reopened, no
privilege is enabled, and there is no VFS shim, SQL query, extra shutdown attempt,
new wait or changed timeout. Missing native observations are diagnostic only;
they never turn an original shutdown failure into success or another error.

Validation must report macOS execution separately from Windows compilation and
actual hosted Windows execution. The non-Windows regression proves unsupported
reporting only. The Windows regression uses an actual distinct writer, snapshots
its retained handle from the caller, queries it after real writer exit, and checks
that an actual wrong-kind native handle produces a frozen unavailable result.
Existing held-close tests retain original timeout, private lock and later cleanup
assertions, while checking native identity belongs to the dedicated writer.

Strict lifecycle completes with 6/6 passing results: the exact 11-file boundary
and five selectors, each running one actual passing test. No failure, skipped,
uncertain or pending-review result remains in that local report. Its test-name
coverage does not distinguish cfg bodies: native Windows accounting assertions
remain unexecuted locally even though their selector has an unsupported-host
implementation. Source review found no additional material defect; no source was
changed after successful validation, only this final evidence prose.
