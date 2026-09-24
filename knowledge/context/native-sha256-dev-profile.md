# Native SHA256 development profile evidence

Original Native run 34622719221 at 22c4993 has four Linux FileService failures.
Each retained original child is running at `executable_hash_entered`, with zero
HTTP requests, no media phase, and untruncated 846-byte stderr. The original
watchdogs expire after 10.904s, 10.906s, 10.905s and 15.055s. The separate restart
selector passes. This localizes those failures inside full configured-executable
hashing; it does not establish the hosted artifact size or distinguish CPU time
from I/O and scheduling. Original logs remain in the external migration cache.

The lockfile has one SHA256 package, sha2 0.10.9, default/std features. Development
and test builds use the unoptimized default profile. The configured-executable
verifier reads all bytes with a 32KiB buffer, checks its original length and bounds,
and compares the actual SHA256. No part of that production operation changes.

A local aarch64 macOS measurement uses the exact original built file protocol
probe, 2,851,880 bytes, SHA256
`9664e875defb93ea5f1d90ef998b1091597779650e4d0c52efbce89ddb33c109`.
The same full 32KiB read/hash loop takes 114ms with the existing unoptimized pinned
sha2 library and 6ms when only that library is compiled with opt-level 3. Both
digests equal an independent Python SHA256 over the same original file. The
caller and dependency libraries stay unoptimized. The cache retains the exact
measurement source, commands, input size/digest and outputs. This single local
measurement motivates the narrow profile change; it is not a hosted benchmark,
Linux artifact measurement, or successful rerun of the failed original tests.

The approved change optimizes only sha2 0.10.9 in the development profile, which
Cargo's test profile inherits. It leaves full reads, digest verification, bounded
memory, parallel tests, all deadlines, package versions and release builds intact.
Existing actual bootstrap refusal/startup and native/SDK media selectors provide
behavioral verification; no profile-text mirror test is added.

A second measurement uses the actual Cargo-generated optimized SHA256 library
and preserves its original local built probe in the external cache. The unchanged
probe bytes total 2,876,408, with SHA256
`1a7f5cf124eb940bc60f4f87fd674ea6a73338df707ee5841d7e8fa3e55673e4`.
Three unoptimized measurements take 108/109/111ms; the actual Cargo override takes
6/6/6ms. Every measurement reads those same original bytes completely, produces
the same digest, and agrees with independent Python hashing. This validates that
the narrow Cargo override implements the measured candidate; the architecture
and hosted-evidence limits above still apply.

Validation preserves the first local bootstrap-target result, 3 passed / 2 failed:
its executable and custody-shutdown tests reached protocol `unknown` rather than
`completed`, after successful startup. Panic cleanup deleted their original
receipts, leaving that protocol cause unobserved. Later explicit lifecycle
selectors pass without superseding the failed target. Media integration passes
5/5 and retained-child observation passes 1/1. Final strict lifecycle passes 6/6
(boundary plus five scenarios; the config filter runs two actual tests and each
other filter one). Its initial root-Cargo boundary syntax failure is retained
separately from the corrected final boundary result.
