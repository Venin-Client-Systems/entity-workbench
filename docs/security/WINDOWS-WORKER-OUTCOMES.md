# Windows worker outcomes and preparation cancellation

This is a prerequisite for connecting the proved Windows document recipe to the
canonical job coordinator. It changes the isolated worker crate only. Core
parsing, extraction schemas, UI and application packaging are not activated here.

The previously successful native recipe campaign remains bound to checkout
`34c238886a249b5799e8e5cac90d828d325f127e`, run `36047073221`, attempt 1. Its
results and the preceding negative campaigns remain recorded in
[Windows Java workers](WINDOWS-JAVA-WORKERS.md). They do not establish native
execution of this later change. The full hosted campaign must run on its exact
new checkout before the modified launcher is considered verified. Hosted Windows
Server results do not establish Windows 11 clean installation or release approval.

## Typed outcomes

Callers match enum variants. They must not infer outcomes from message text or
arbitrary numeric exit codes.

| Worker outcome | Meaning and required coordinator treatment |
|---|---|
| `Blocked` | A required configuration, runtime inventory or boundary precondition was unavailable/rejected. It never authorizes fallback. |
| `Cancelled` | The cooperative token was observed. After process creation, this is returned only after acknowledged termination and successful cleanup. |
| `ResourceLimit` | A directly observed wall-time, tree-byte, tree-entry, tree-depth, handle or output-byte limit was exceeded. |
| `InvalidResult` | Missing, malformed, ambiguous or mismatched output failed validation. No derivative is accepted. |
| `Api`, `Io`, `Exit` | Other failures retain their sanitized category/code. In particular, an unexplained exit is not labelled memory exhaustion. |
| `Cleanup` | Process exit is known, but assignment/profile cleanup failed. Even an otherwise valid result is rejected. A preceding failure remains attached. |
| `TerminationUnverified` | Process exit could not be acknowledged. The assignment is retained and must not be traversed, repaired or accepted. Both the termination cause and preceding operation failure are retained. The core must suspend further jobs using its existing `WorkerExitUnverified` path. |

Shared filesystem checks retain `Blocked` for invalid runtime configuration.
At worker scratch/profile/index inspection boundaries, their policy rejections
become `InvalidResult`; quota, cancellation, API and I/O outcomes are preserved.
This prevents an unsafe output tree being described as an unavailable runtime.

The shared completion function enforces termination before cleanup, and cleanup
before accepting a result. Cancellation does not override either failure. If
assignment to the Job Object fails, the still-suspended process is terminated
directly and waited on; lack of acknowledgement also becomes
`TerminationUnverified`. Job-handle Drop may attempt another termination, but
that best-effort action does not upgrade the recorded outcome to verified exit.

Temporary-directory ownership begins immediately after creation. Profile setup
and synthetic-control path canonicalization run inside explicit completion
ownership. A profile that was created but could not finish initialization is
explicitly closed; if that cleanup fails, `Cleanup` retains the setup error.
No process has been created at this stage, so it cannot be mistaken for
`TerminationUnverified`.

The synthetic unconfined Java control uses the same outcome precedence. A control
cleanup or termination failure aborts the campaign. It remains a synthetic
diagnostic control, never a fallback for application work.

## Preparation checkpoints

The same caller-owned cancellation closure is checked before runtime verification,
while traversing runtime entries, during runtime hashing/copying, while applying
the existing runtime ACLs, before process creation, and after token verification
before resuming the suspended process. The running monitor and final Java
acceptance still check the token.

Runtime hashing and copying use exact-length streams with a maximum 64 KiB read
between checkpoints. Cancellation after a read prevents that chunk from being
consumed. EOF, growth and digest mismatch remain failures. Copying opens the source
through the existing no-follow, single-link reader and creates a new destination;
it cannot overwrite an existing assignment file. A cancelled partial copy is
removed through the normal assignment cleanup path.

These checks provide cooperative cancellation, not a guarantee that an individual
filesystem call can be interrupted. The existing bounded manifest and assigned
input/request/snapshot reads or writes retain their bounds. Cancellation is checked
before worker creation after that preparation. Cleanup is deliberately not
cancelled once it is safe to perform.

No JVM arguments, parser/search policy identities, ACL masks, capabilities, process
limits, file/output budgets, child-process policy, handle inheritance or native
termination timeout are increased. No dependency is added. Public application IPC
still cannot supply a JVM flag, executable, assigned filename or cancellation hook.

## Verification required for this change

Portable tests cover outcome precedence and preservation, prohibition of cleanup
on unknown exit, explicit resource classification, malformed and mismatched
replies, cancellation before missing runtime/scratch access, cancellation during
runtime inventory verification and before/between/after bounded reads. These
tests exercise the same helpers used by native assignment completion; injected
termination errors are not represented as actual Win32 failure reproduction.

Native-only tests are prepared for:

- Cancellation before runtime copying, before process creation and before resuming
  a real suspended AppContainer process. Every case requires `Cancelled` and an
  empty assignment parent. Phase observation is thread-local and compiled only
  into tests; it cannot be selected through a runtime request or worker IPC.
- Cancellation after one 64 KiB runtime-copy chunk, exact source preservation,
  rejection of destination overwrite and rejection of a hardlinked source.
- Explicit closure of a real disposable AppContainer profile after a synthetic
  initialization error, with verified removal of its assigned folder.
- The existing hostile launcher checks now require the typed wall-time, disk,
  handle, cancellation, output-size and invalid-output outcomes. The unexplained
  memory-worker exit remains an exit outcome. None of the existing permission,
  delivery-denial, cleanup or Java role controls is waived.

Run the existing Windows AppContainer workflow on the exact candidate source. It
already includes all crate unit tests and both actual native probes. Preserve
source/runtime/probe identities and the receipt from every success or failure.
Cross-compilation and Clippy are source checks only; they cannot substitute for
these native executions. Core queue-suspension, extraction-v2 publication and
application integration tests belong to the following activation slice.
