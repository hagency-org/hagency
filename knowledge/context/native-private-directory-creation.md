# Atomic private directory creation

The original Windows Native run 34622719221 at 22c4993 records three full file
workflows at serving, after 29 HTTP requests, with media phase
private_policy_refused. The atomic media-creation unit also returns Unavailable.
Those original failures are preserved in the external CI cache. They establish
the validator interval, not a recorded owner SID or specific ACL predicate.

[Rust 1.95 DirBuilder::mkdir](https://github.com/rust-lang/rust/blob/1.95.0/library/std/src/sys/fs/windows.rs#L1152)
calls CreateDirectoryW with NULL security attributes. Microsoft documents
[inherited default directory ACLs](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createdirectoryw)
and separately specifies that a
[new object's owner comes from its creating token's default owner](https://learn.microsoft.com/en-us/windows/win32/secauthz/owner-of-a-new-object).
The token's default owner need not be TokenUser. The current private checker
requires TokenUser ownership; its existing explicit owner/DACL creation branch
is skipped after recovery has already made the directory. Therefore the original
source necessarily refuses default creation for a token whose default owner
differs from its user, despite a correctly inherited private DACL. Applying this
condition to the particular original CI token remains an inference until actual
native token/object observations are available.

The bounded repair uses the already accepted explicit owner and protected DACL
at the atomic Windows CreateDirectoryW call. The private checker remains strict.
The creation-only API does not observe a path to decide freshness and cannot
modify an existing entry. Its validation does not invoke another creation if
the new path disappears. Existing directory validation, Store create/open
selection, original locking and missing-journal refusal remain independent.

The Windows regression will observe the actual current token's default owner,
an original default-created directory, and the explicit-created private directory.
It will compare actual owner/ACL facts without assuming elevation or modifying
the token. Local macOS execution and Windows compilation cannot supply that
Windows runtime evidence; the next hosted original run remains required.

Local execution passes the new Unix creation/unchanged-entry/concurrent-creator
regression (1 test), original media freshness/exclusive-lock/missing-journal
regression (1 test), and complete actual FileService target (5 tests). The rebuilt
package-only list verifies the expected 18 hagency library and 5 FileService tests
from this worktree. Windows all-target compilation passes for hagency-store and
hagency; Windows hagency-store Clippy passes with warnings denied. Those checks
do not run the Windows owner/ACL assertions. The next hosted execution must
report its actual boolean token/default/explicit owner observations and retain
any original workflow failure separately.

Strict lifecycle completes with 4/4 passing results: the exact eight-path boundary
and three selectors, each executing one actual passing test. There are zero failed,
skipped, uncertain or pending-review results. The first selector also launches
unrelated workspace binaries; its sampled pre-Rust dyld delay is preserved in the
external evidence, and the original invocation completed without intervention.
The lifecycle's test-name coverage does not distinguish cfg branches: its Windows
assertion label is not Windows execution evidence. Actual Windows validation stays
pending. Final local hagency/hagency-store Clippy with warnings denied also passes.
