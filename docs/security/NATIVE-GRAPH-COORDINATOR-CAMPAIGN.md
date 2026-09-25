# Native graph coordinator campaign

This is a source-only, explicitly ignored development campaign. No candidate execution is established by this document. Normal application startup still supplies `GraphExecution::Unavailable`; there is no environment/PATH runtime discovery, public graph command, native UI or release claim.

The campaign test lives below the private coordinator graph module and attaches a `cfg(test)` observer to one explicitly verified opaque runtime. It uses the unchanged fixed Python adapter and supervisor. It never modifies the Python worker, confinement policy, process limits, accepted graph semantics or canonical publication rules.

## Fixed four cases

The one test invocation runs these cases in order and stops at the first failure. No failed or unobserved case is relaunched.

1. **Ordinary path and retained publication.** The fictional six-entity fixture captures `a → c`, requiring nodes `a,b,c` and original-linked assertion IDs `r1,r1-parallel` then `r2`. A test-owned SQLite trigger rejects the first graph terminal update. The coordinator must retain the exact result and owned attempt in `PublicationPending`. Removing that trigger and requesting the exact private publication retry must complete without a second adapter call, child launch, assignment, request or result. A synthetic document and collection run are queued after the graph and before capture. Both must remain queued, with zero executor calls, during the owned interval; afterward both must execute exactly once and reach the fixed blocked terminal states. The collection outcome is a known-quiescent policy refusal before any response head/body, so it cannot introduce even an empty response original. The historical graph may correctly become stale as these competitors advance the workspace.
2. **Unreachable.** The same canonical fixture asks for isolated `f`; the completed immutable result must be `unreachable`.
3. **Genuine larger capture.** One canonical transaction creates 700 fictional entities with 36-character IDs and 699 accepted, anchored chain assertions/observations. The normal private capture must produce more than 64 KiB and at most 1 MiB of request bytes, without padding. The exact unique 700-node path and 699-hop provenance are retained. This does not change the older canonical recipe's 64 KiB bound or its historical evidence.
4. **Observed-live child cancellation.** The host records a successful non-reaping `waitid(...WNOHANG|WNOWAIT)` observation of the owned child before asking the coordinator to perform canonical `CancelProcessingJob`. The callback waits at most five seconds for that caller token. The same supervisor must confirm process-group stop/reap and cleanup, and the job must durably become cancelled with no graph record. This proves a cancellation request after a live-child observation. It does not establish that Python or NetworkX computation had started, or that the child remained alive at the exact signal instruction.

## Observation and ownership

The observer is not global and has no production IPC. Its launch callback runs after the `ProcessGroup` owns the child and inside the outcome closure. Callback failure still flows through the unchanged stop/reap path. Termination failure retains its precedence. Launch and live observation have distinct timestamps; cancellation observation does not imply stop. Prepared profile SHA-256 and all six staged file identities bind the actual assignment. Successful delivery requires both runtime inventories; cancellation deliberately has no post-runtime inventory claim inside the adapter.

The observer retains at most 1 MiB of request, 64 KiB of wrapper and 128 KiB of result bytes, and will expose them only after confirmed termination and successful cleanup. Immutable graph record bytes must equal these exact raw request/result bytes. The ordinary retry compares the entire observer receipt before and after publication retry, including one call/launch and raw identities.

The task-owned workspace and app-local runtime copy use persistent fresh directories. The test intentionally avoids implicit coordinator destruction when supervision/cleanup is not confirmed. In that failure case it records only the host observer receipt and stops; it does not read child outputs, compare canonical tables, verify the prefix afterward, clean paths, or launch another case. The coordinator's own quarantine/terminal handling remains unchanged. An outer test timeout is likewise unverified and causes no output/canonical/runtime post-read, retry or cleanup.

Every successful case retains all fixed canonical tables (including schema, history, events, metadata and SQLite sequences), the original name/byte/hash set, exact job/record outputs and before/after identities. Allowed changes are restricted to the named graph job, the two named ordinary-case competitor jobs, their appended audit rows, and one graph result for successful cases. No prior audit row or other canonical fact may change. Sequence checks account for SQLite's insert-on-conflict sequence advances. The independent Python validator reads the closed database in read-only mode and checks every table and actual original bytes against the retained snapshot. It also checks raw wrapper/result hashes, exact fixture topology, review denominators, R/Q/C/P and immutable record linkage. Reopen plus exact queue UUID replay must leave all canonical tables unchanged.

## Explicit invocation and limits

After source review and separate authorization, the parent can invoke:

```sh
python3 scripts/test_graph_coordinator_native.py \
  --prefix /private/reviewed-runtime-prefix \
  --artifacts /private/fresh-campaign-directory \
  --execute-reviewed-campaign
```

These paths are harness inputs only. They do not configure the normal application. The harness requires a clean source tree, pins every tracked file plus commit/tree, verifies the supplied fixed runtime inventory, makes a fresh verified app-resource copy at `app-engines/python`, builds the exact release test binary offline, and records that binary's SHA-256. The prefix itself and the copied prefix are independently checked after all four successful cases. No source, original prefix or previous campaign is overwritten.

The offline build has a 600-second outer limit; the one four-case native test has a 900-second outer limit. Each coordinator wait has a 180-second bound, the post-publication synthetic competitor wait is ten seconds and the live-cancellation handshake is five seconds. The existing child limit remains 30 seconds wall time and 30 seconds CPU time. The child limit does not include inventory hashing, staging, canonical capture or publication. Retained snapshots are limited to 64 MiB, graph records to the existing 16 MiB and parent/case receipts to their fixed bounded readers. The source runner initializes its failed report before metadata acquisition, so source/preflight failure cannot leave a previous success.

## Source verification and remaining limits

Ordinary source checks cover the real 700/699 canonical capture, acceptance of the exact real graph claim/publication change set, rejection of unrelated table/original changes, per-assignment observer isolation, the caller cancellation handshake, raw-byte identity and unknown-termination/cleanup gates. Python tests never import NetworkX or start a candidate; Rust native tests remain ignored unless explicitly selected.

The first fixture check failed because the temporary path used the macOS alias rejected by the existing no-link workspace policy. The fixture now uses the canonical temporary parent; the failure log is retained. An initial Python mutation-test setup tried to mutate an empty table; it was corrected. A subsequent negative test made explicit the appended-history revision validation, which is now checked in both Rust and Python. Peer review found that detached successful receipts were not bound to the selected canonical row bodies. The repaired independent checker requires unique key/body identity, exact before/after job and competitor bodies, exact inserted graph record equality and both complete logical snapshot identities; offline substitution regressions cover that repair.

No successful native campaign, installed-app activation, signed helper, Windows/Linux confinement, hard RSS ceiling, supervisor-crash child containment or full release readiness is claimed here. Historical canonical probe receipts remain pinned to their original source; the single trailing blank-line normalization in `runtime_support.py` matches root commit `8b94b17` and is not applied retroactively to those identities.

The frozen source checks passed 639 ordinary Rust tests (560 unit and 79 integration), with 32 native tests explicitly ignored. Strict debug/release all-target Clippy and formatting passed. Python checks passed 349 tests with four explicit skips in both normal and optimized mode; the eight focused campaign-validator cases also passed. All 105 tracked historical schema-directory files remained byte-identical. These are source-validation results, not a native campaign result.
