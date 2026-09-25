# Windows native DNS observation contract

This opt-in development harness observes the existing private `GetAddrInfoExW` resolver. It does not activate durable collection, issue HTTP requests, change resolver behavior, or satisfy Windows 11 or complete-release acceptance. No native Windows observation has been recorded for this increment. Host tests and Windows-target type checks exercise different claims and must stay separate from a later actual Windows run.

The actual implementation uses `GetAddrInfoExW` with `NS_DNS`, an `OVERLAPPED` structure and a manual-reset event. It does **not** use `DnsQueryEx` or a completion-routine callback. The test-only hooks record actual API return codes and resource disposal; they do not replace the native APIs. Production changes only bind existing API return values so those hooks can observe them under `cfg(test)`.

## Fixed scope and acceptance

One campaign contains these five ordered cases. It cannot select another name or retry a case.

| Case | Native launches | Required observation |
| --- | --- | --- |
| Pre-cancelled transport for `example.com` | 0 | `Cancelled` before the request, without Winsock/event/context creation. |
| Pre-expired transport for `example.com` | 0 | `Deadline` before the request, without resolver resources. |
| Native success for `example.com` | At most 1 | One to 64 validated public candidates, normal completion and disposal of the returned list, caller context, event and Winsock reference. |
| Native negative for `ew-native-proof.invalid` | At most 1 | `Network` with observed `WSAHOST_NOT_FOUND` (11001) or `WSANO_DATA` (11004), and completed caller-context cleanup. This is not an authoritative NXDOMAIN or complete-RRset claim. |
| Cancellation after native launch for `ew-native-cancel-proof.invalid` | At most 1 | An actual initial `WSA_IO_PENDING` (997), actual cancellation call, `QuiescenceUnverified`, explicit caller-context state, then `RecoveryRequired` with zero new native launches. |

The cancellation token is set by a private test-only hook immediately **after** the actual `GetAddrInfoExW` call returns. It does not force that call to become asynchronous. Synchronous completion is retained as `not_exercised`, makes the campaign incomplete, and is never retried. The cancellation hook precedes the transport's post-DNS window check, so even a synchronous success cannot advance to TLS or HTTP. The two middle cases call the resolver directly and cannot issue HTTP.

The limits are three native launch calls, no HTTP, no retries, 20 seconds of cooperative campaign time and a 30-second outer native process bound. The existing resolver has a five-second DNS stage and at most one second of caller-context cancellation cleanup. OS calls are trusted local operations and are not preempted by the polling interval; the outer process bound is separate. Native launch counts are **not** wire-packet counts. The OS resolver and its configured provider may perform their own work. No case contents, account details or private addresses are supplied.

If an earlier lookup reports unknown provider quiescence, remaining cases are `not_attempted`; the harness does not continue direct resolver calls after that uncertainty. Cancellation runs last because the normal transport then quarantines the process. The receipt requires the follow-up refusal without another Winsock startup or native launch.

## Completion and resource ownership

Microsoft documents that an asynchronous `GetAddrInfoExW` call can use the event in `OVERLAPPED.hEvent` when no completion routine is supplied, and that this event must be manually reset. The implementation retains all caller-owned buffers and the event until completion is established. [GetAddrInfoExW](https://learn.microsoft.com/en-us/windows/win32/api/ws2tcpip/nf-ws2tcpip-getaddrinfoexw)

`GetAddrInfoExCancel` can return zero and signal cancellation completion while an underlying synchronous namespace provider continues working. Its return alone cannot establish provider quiescence or authorize release of an unfinished caller context. A failed cancellation can mean an invalid/already-completed handle; the harness records the actual code without converting it into a success claim. [GetAddrInfoExCancel](https://learn.microsoft.com/en-us/windows/win32/api/ws2tcpip/nf-ws2tcpip-getaddrinfoexcancel)

After a signalled event, the implementation asks `GetAddrInfoExOverlappedResult` for the outcome. `WSAEINPROGRESS` (10036) remains pending; the existing implementation also conservatively treats 996/997 as pending. A terminal result establishes caller-context completion, distinct from provider quiescence. [GetAddrInfoExOverlappedResult](https://learn.microsoft.com/en-us/windows/win32/api/ws2tcpip/nf-ws2tcpip-getaddrinfoexoverlappedresult)

Two unknown-quiescence receipts therefore remain distinct:

- `released_after_completion`: a terminal completion was observed, the context was dropped, the event close succeeded and `WSACleanup` returned zero. The provider still may be active.
- `retained_pending_completion`: completion was not established within the cleanup bound. The context, event, possible result pointer and Winsock reference remain retained for the lifetime of the quarantined process. The receipt must show no disposal of those resources.

Neither result is a confirmed `Cancelled` transport acknowledgment. An outer process timeout records forced termination and explicitly leaves caller-context completion unproved; killing that owned process does not prove shared DNS-provider quiescence. This evidence cannot be substituted for the separate callback contract of [DnsQueryEx](https://learn.microsoft.com/en-us/windows/win32/api/windns/nf-windns-dnsqueryex).

## Source and evidence binding

The runner requires an explicit `--allow-fixed-dns`, actual Windows, a clean checkout and a commit signature verified against a supplied, already trusted SSH public key. It builds the libtest executable with `--locked --offline` and embeds that source commit in `EW_WINDOWS_DNS_BUILD_SOURCE`. Each case must match that compiled source, the runtime source and a fresh canonical UUID nonce. Zero selected tests, wrong order, duplicate JSON fields, missing cases, an unknown outcome or a nonzero process exit cannot pass.

The source tree, lockfile, runner/validator/shared helper and binary identities are recorded. Source and binary identity are checked after failures as well as successes. A unique ignored `artifacts/windows-native-dns/<run-id>/` retains an initial incomplete report, build output, flushed partial native output and final receipt. Diagnostic hashing or final-write failures do not silently replace a primary failure with success. Logs and parsed receipts have read bounds; API-code traces have a fixed count bound. Receipt validation uses explicit checks and also runs under Python `-O`.

The report records the actual OS version, Windows edition and architecture without a machine name or candidate addresses. A successful future Windows Server 2022 run would establish only the outcomes actually observed on that host. No result in this increment establishes Windows 11 installation, HTTPS behavior, all DNS providers, a complete RRset, provider quiescence or a release gate.

## Separate manual and single-branch workflow proposal

`.github/workflows/windows-native-dns.yml` does not alter synthetic source verification or AppContainer probes. Its manual route requires both an explicit fixed-DNS opt-in and the exact lowercase 40-hex signed source commit, equal to the selected workflow ref's `GITHUB_SHA`. Checkout pins that commit and retains no credentials. The existing trusted public signing key is fixed in the reviewed workflow, without its private key or identity comment; no downloaded key is trusted.

GitHub requires a manual workflow to exist on the default branch before dispatch. This harness does not bypass protected integration review to register it. [Manually running a workflow](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow)

The separately authorized initial proof route is the **first push creating exactly `verify/windows-native-dns-20260925`** in `Venin-Client-Systems/entity-workbench`. Publishing the reviewed signed commit to that branch enables the fixed DNS opt-in and runs at most three native DNS launches, without HTTP or retries. The source is that push's `GITHUB_SHA`. Before checkout the workflow validates repository identity, exact ref, `created=true` and an all-zero previous commit. Later branch updates fail this gate; workflow reruns also fail because `GITHUB_RUN_ATTEMPT` must be one. Normal integration branches and pull requests do not trigger this workflow. The parent integrator owns the one branch creation after reviewing the final signed source and will not move or recreate that branch for another observation.

The workflow verifies the signature before running repository scripts. A distinct preparation step obtains only locked Cargo dependencies for the Windows target; this build-preparation traffic is outside the DNS campaign's zero-HTTP claim. The runner then builds offline and invokes only the exact ignored native test. An `always()` artifact step retains the initial workflow receipt, build preparation log and all available runner artifacts on success or failure. Abrupt runner loss can still prevent artifact upload and must never be reported as a completed observation.

The workflow remains a review candidate until the parent integrator approves the signed source and exact trigger. Checking in this file does not authorize this implementing agent to push, dispatch or run native queries. A local invocation on an approved Windows host uses the same runner and reviewed public allowed-signers file:

```sh
python scripts/test_windows_native_dns.py --allow-fixed-dns --allowed-signers /path/to/allowed_signers
```

Synthetic verification is safe without network access:

```sh
python -m unittest discover -s scripts/tests -p test_windows_dns_receipts.py -v
python -O -m unittest discover -s scripts/tests -p test_windows_dns_receipts.py -v
cargo test -p workbench-core --lib native_windows_proof --locked --offline
```

The ignored Windows campaign is excluded from ordinary `cargo test`; `--ignored` plus its exact name and all source/scope/nonce gates are required. Historical Mac observations, including the failed first negative-DNS campaign, are unchanged.
