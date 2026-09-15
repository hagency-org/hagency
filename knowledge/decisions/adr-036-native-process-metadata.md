---
kind: decision
id: ADR-036
title: Query observer argv and cwd through a narrow Darwin collector
status: Accepted
tags: [darwin, process, recovery]
---

The existing Herdr public process query accepts a pane, not an arbitrary PID.
The fault observer runs outside the lower pane. Its real token array and cwd
must not be inferred from ps display text or a recorded launch command.

Use a standalone C helper with host SDK sysctl/libproc declarations. Preserve
exact argc and empty tokens, validate strict UTF8 and complete buffers, emit
only pid/cwd/argv, and fail closed on unsupported or incomplete observations.
The caller retains the existing birth collector and brackets metadata with
fresh identity snapshots under a deadline. Separate syscalls are not atomic.

Herdr's internal argv parser supplies implementation context but cannot be
copied unchanged: it tolerates unterminated values, rejects empty arguments
and replaces invalid UTF8. KERN_PROCARGS2 also places environment data after
argv; it enters the temporary kernel buffer but is never parsed or published.

The initial layout investigation also reproduced a real empty-argv0 ambiguity.
Skipping all NUL after exec_path consumes argv0 and can serialize the first
environment variable as the final argument. Observe target width through
PROC_PIDTBSDINFO/PROC_FLAG_LP64 and verify it before/after collection; skip only
the exact XNU padding `(width-((16+exec_path_bytes_with_NUL)%width))%width`.
At the resulting argv boundary reject empty argv0 immediately. The parser
receives width4 or8 from actual metadata, never from a CLI override.

Source basis: Apple XNU f6217f891ac0bb64f3d375211650a4c1ff8ca1ea,
bsd/kern/kern_exec.c exec_extract_strings and bsd/kern/kern_sysctl.c
sysctl_procargsx. The independent owned-process experiment and precise links
are retained in the E2E evidence native-process-metadata-layout-review.md.

This supports the already authorized recovery-control preparation. It does
not change frozen observers, install global dependencies, launch managed
processes, or imply cross-platform/full-chain acceptance.
