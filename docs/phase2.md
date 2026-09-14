# Persistence and process execution

The backend remains synchronous. Embeddings should call `operations` and
`launcher::launch_command` with an `OperationContext`, and wrap each operation in
`context.run(...)` to receive its result, notices, and lifecycle events. No GUI,
worker pool, daemon, or terminal adapter is included.

## Concurrent mutations

Each managed repository has a persistent sibling `.NAME.loadbot-lock` file.
Configuration and shortcut transactions use a sibling lock beside the TOML file.
`std::fs::File::try_lock` provides a nonblocking OS-backed exclusive lease.
An occupied lease returns typed `persistence::Busy`, with the resource path and
retry guidance. Closing the handle, including process exit, releases the lock.
Lock files are never removed or replaced: deleting them would split protection
between different file identities. A damaged lock generation is an explicit
error requiring inspection, not permission to proceed unlocked.

Repository operations acquire their repository lease before inspection and keep
it through clone/update/commit/push and cleanup. High-level tool launching also
holds the tool repository lease. Different repositories remain independent.
Catalog registration acquires the configuration lease only for the short merge
and save, preserving registrations another process added during a clone.
Ordering is repository first, then configuration; configuration callbacks must
not acquire repository leases. All acquisition is nonblocking.

`shortcuts::save` and removal lock the entire read-modify-write transaction.
Confirmed removals use `remove_if_matches` so a definition replaced while the
user was deciding is not removed. `config::update` and `catalog::update` are the
transaction APIs for embedding code; their callbacks must be short and must not
prompt. The whole-document `save` functions and low-level Git primitives are
building blocks, not read-modify-write APIs: callers composing them must hold
the corresponding lease. Use the high-level operations whenever possible.

An `OperationContext` releases repository leases around `Interaction` decisions.
It reacquires them and compares a generation recorded in each lock file before
applying the answer. Any intervening Loadbot mutation, even one that failed,
requires a retry. Tool operations also compare their configuration/catalog
snapshots after a decision. Remote URL decisions recheck Git URLs; updates
recheck the branch, commit, and dirty state before fast-forward merging.
Uncooperative external programs do not honor advisory leases. Existing Git/path
safeguards still apply, but these leases are not a security boundary against
external filesystem edits. Listing, status, and completion do not acquire leases
or create directories/files.

## Replacement and recovery

All TOML writers use one implementation. It serializes before replacement,
creates an exclusive unique temporary file in the destination directory, writes
and synchronizes the contents, and uses `tempfile::NamedTempFile::persist` to
replace the destination. Existing ordinary file permissions are copied. On Unix
this uses rename; on Windows the dependency uses `MoveFileExW` with replacement.
The old file is never first renamed out of the way. Write/replacement failures
leave the previous destination in place and clean only this operation's
temporary file.

Unix additionally synchronizes the parent directory after replacement. A failure
at that point returns `DurabilityUncertain`: the new file is already visible and
must not be described as rolled back. Reports classify this as partial work and
clone cleanup retains the checkout. Windows does not provide the same directory
sync guarantee through this implementation. Filesystem, network-share, storage
hardware, and power-loss behavior can weaken durability; this is not a claim of
universal crash-proof atomicity. Windows ACLs follow replacement-file semantics;
this is not an ACL-preserving backup system.

Interrupted writes may leave unique `.loadbot-write-*` files. Readers ignore
them, and subsequent writers neither reuse nor delete them. The committed file
remains authoritative. Legacy Phase 1 `*.toml.loadbot-backup` files cause an
explicit recovery-required error, even if the main file is absent. No reader
silently substitutes an empty configuration. Inspect both files, retain the
valid data, and explicitly restore/remove the legacy backup while other Loadbot
processes are stopped. Recovery never guesses which copy the user intended.

## Observation and cancellation

`OperationContext.process` is a cloneable `process::Control`. Set its `observer`
to receive structured start, PID, stdout/stderr byte chunks, exit status, and
cancellation/error events. `context.run` additionally emits operation lifecycle
events. Callbacks run on the executor thread and must return promptly; transfer
events to the GUI through a bounded queue if necessary. Do not block callbacks
waiting for UI decisions. Bytes may split text characters; decoding is the
consumer's responsibility. Stdout/stderr ordering is preserved within each
stream, not globally across streams.

Clone `context.process.cancellation` and call `cancel()` from the caller's thread.
Tokens are one-shot and belong to the caller; create a fresh token for a retry.
Cancellation is checked before work and polled while subprocesses run. Short
persistence steps finish safely. Completed mutations remain in `report.notices`;
`report.status()` distinguishes success, busy, cancelled, and failure, while
`report.is_partial()` identifies completed mutations before an error.
Interrupted Git commits/pushes can have side effects even if Git did not return
success. Inspect Git state and the streamed diagnostics before retrying; there
is no automatic undo of local commits or remote pushes.

Remote reconciliation checks cancellation before starting its short configuration
transaction, then defers cancellation through both URL writes and verification.
On failure it attempts both restorations and verifies the original local URL
values, preserving an absent explicit push URL. Recovery command failures remain
in the diagnostic; unverified recovery returns `RemoteRecoveryIncomplete` and a
failed report with possible partial work. Successful reconciliation is recorded
before further cancellation checks, so later cancellation retains that completed
step. The repository lease remains held throughout the transaction and recovery.

CLI launched tools default to `Mode::Inherit`, retaining stdin/stdout/stderr and
the terminal. A GUI must set `tool_mode = Mode::Stream` and
`process.terminal = false` for noninteractive tools. Captured/streamed execution
has null stdin; it is not a terminal. Interactive tools require the existing CLI
terminal until a separate GUI terminal adapter exists. CLI Git can still prompt
through its terminal while its stdout is captured. No PTY subsystem is added.

Both pipes are drained concurrently in 8 KiB chunks through a bounded 16-chunk
queue. Stream mode retains no output. Internal Git capture is limited to 4 MiB
per stream (Rot identity inspection to 1 MiB); overflow is an error rather than
silently parsing truncated machine output. Observers continue receiving output
when the capture limit is exceeded. Synchronous operation scopes lend process
control to nested Git inspection helpers and restore it on return/unwind; they
are local to the calling thread.

## Process ownership

Linux children run in a separate process group. In inherited CLI execution,
foreground terminal ownership is transferred and restored. Cancellation kills
the group, reaps the direct child, and checks for running group members; orphan
zombies are left for their parent/init to reap. Windows children start suspended,
are assigned to a Job Object with kill-on-close, and are then resumed. The job
owns descendants; cleanup checks that its active-process count reaches zero.
Failure to establish Windows ownership fails the launch and stops/reaps the
suspended child rather than running it without cancellation protection.

This executor owns subprocess descendants: after the direct child exits,
remaining group/job members are terminated so they cannot silently retain pipes
or continue mutating a checkout. Commands intended to launch detached daemons
are therefore not supported. Linux process groups are not a sandbox: programs
that deliberately escape with `setsid`/`setpgid`, and processes on the far side
of WSL, SSH, or another external service, are outside these ownership guarantees.
Use native Windows interpreters and non-daemonizing local tools for managed
cancellation. The Phase 1 shell argument and PATH-order fixes remain intact.

Cleanup errors return `CleanupIncomplete` and retain the checkout; they are not
reported as completed cancellation. Output pipes retained past the cleanup
deadline produce this error rather than an endless drain. OS-uninterruptible
processes can delay reaping: the backend does not declare cancellation complete
while its direct child is still alive. Caller observer code must not panic or
block indefinitely. Abrupt application termination releases leases; on Windows
it also closes the job. Linux abrupt application termination is not a substitute
for orderly cancellation of its process group.

## Verification status

Focused tests use temporary directories, local repositories, and controlled
test subprocesses. They cover conflicting document/repository transactions,
OS lock release on process exit, decision revalidation, failed replacement,
legacy recovery detection, simultaneous large output, spawn/nonzero status,
process-tree cancellation, and saved/committed work followed by cancellation or
push failure. Existing CLI and terminal tests remain enabled.

Native Windows execution and final rustfmt/Clippy checks require CI in the current
development environment. Phase 2 is not complete until the final Linux and
Windows required checks pass. PowerShell setup tests are separate from Windows
Rust coverage.
