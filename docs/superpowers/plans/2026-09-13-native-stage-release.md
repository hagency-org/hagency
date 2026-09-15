# Native stage instance alias repair

> Execute inline with the existing stage contract; obtain independent code review before publishing a replacement bundle.

The live readonly validator rejects a frozen observer whose instance is spelled
through macOS's system `/tmp` alias. The Python observer already accepts that
specific alias and requires byte-exact backend argv. Rewriting the binding would
contradict those process facts and would corrupt the claimed attempt.

Keep strict canonical paths everywhere else. For the instance field only, accept
a normalized absolute Darwin `/tmp/` descendant after checking the root-owned
system symlink resolves to `/private/tmp`, the mapped descendant has no symlink,
and the original spelling resolves to that exact descendant. Retain the original
string and argv. Check this again whenever stage evidence is revalidated.

- [x] Add fixture cases for both stages using the real macOS alias; observe the current validator fail.
- [x] Cover unrelated and descendant symlinks, lexical traversal, and target replacement at recheck.
- [x] Add the narrow private validator in native-stage-evidence.mjs; preserve all existing pin/identity checks.
- [x] Run both stage test files, lint, contract parse/lint/lifecycle and relevant skill distribution checks.
- [x] Independently review the change and update ADR/skill documentation.
- [ ] Update PR163 with the reviewed source repair and verification results.
- [ ] Publish only a new immutable bundle after review. Preserve all running frozen files and actual failed outcomes; an expired observer cannot be reused.

Verification: the final evidence suite passed all60 cases on macOS ARM64 with
Node22.22.0. The unchanged release/distribution cases passed in the preceding
86-case run (22 release and6 distribution cases). Focused ESLint passed.
Independent final review found no remaining P1/P2 issue after strengthening
two negative-test boundaries. Existing CI runs these files on both Mac
architectures.

`npm run verify:ci` passed, including584 executable specification bindings and
the kernel/CLI smoke tests. Optional live multi-side/agent checks skipped
without a configured runtime; this does not constitute live E2E acceptance.

The task contract parses and lints with agent-spec1.4.0 (16 scenarios; warnings
retained). Lifecycle did not pass: its automatic worktree boundary paths were
misresolved as absolute paths; explicit repository-relative changes pass the
boundary check, but all16 behavior scenarios remain skipped with no verifier
covering their steps. The actual Vitest results are separate evidence and do
not convert those lifecycle skips into passes.
