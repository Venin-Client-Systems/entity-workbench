# Private graph executor composition

The exclusive scheduler can now hold the opaque `VerifiedGraphRuntime` through
its private `GraphExecution::Configured` variant. This composes the independently
reviewed [scheduling lifecycle](GRAPH-SCHEDULING.md) and
[fixed application adapter](../security/PYTHON-GRAPH-ADAPTER.md). Normal desktop
startup still constructs `Unavailable`; no command, setting, environment
variable, PATH discovery or public runtime attachment was added.

The pending attempt captures its scratch parent directly from the owning
workspace while the workspace and admission locks are held. Queued JSON cannot
select that path. Execution passes only that parent, the exact bounded canonical
request and the registered cancellation token into the fixed adapter. The adapter
revalidates private scratch permissions and the pinned runtime before creating
an assignment. It returns raw result bytes only through its existing confirmed
termination, inventory, correlation and cleanup checks. The canonical GraphAttempt
and coordinator ownership lifetime remain in Rust throughout publication.

Successful configuration is not a perpetual availability claim. Runtime or
scratch changes detected before execution produce a typed failed/blocked attempt;
they cannot publish an analytical result. Cancellation remains visible during
preparation and hashing. Unverified termination and cleanup failure retain their
existing precedence over cancellation. No native invocation is part of this
composition change. Actual coordinator runtime execution, durable result and
cancellation observations require a separate exact-source campaign; earlier
fixed native probe receipts do not establish those outcomes.

The retained coordinator regression suite exercises the integrated source using
synthetic executors. A separate read-only agent review checked capability
construction, workspace-derived scratch, cancellation propagation, result
authority and unchanged default activation. That engineering review is not an
independent release/security review. The existing capture and preparation latency
limits remain as documented in the two linked contracts.
