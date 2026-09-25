# Opt-in native DNS evidence

This development campaign exercises the actual macOS DNSService resolver and the real private transport's before-start cancellation/deadline paths. It does not activate a public command, durable live collection, or the production coordinator's collection lane. Ordinary tests remain offline; the native campaign is ignored unless explicitly selected and separately enabled by a fixed environment value.

The existing discovery benchmark lists proposed seeds with access review still required. The earlier `example.com` smoke observation is explicitly not terms approval. Consequently this campaign makes **zero HTTP requests** and records HTTPS as `not_attempted / reviewed_access_not_established`. It does not fetch robots, terms, documents, redirects, or search results. Useful discovery, website access review and the complete release gate remain separate.

The exact authorised scope is:

| Case | Entry point | Name | Maximum native subscriptions | Required observation |
| --- | --- | --- | --- | --- |
| Pre-cancel | Private `fetch`, default real configuration | `example.com` | 0 | Cancelled before DNS/HTTP; owned operation ended |
| Already expired | Private `fetch`, default real configuration | `example.com` | 0 | Deadline before DNS/HTTP; owned operation ended |
| Public candidates | Actual `NativeResolver` | `example.com` | 1 | Nonempty policy-valid observed candidate batch, deallocation before return |
| Negative name | Actual `NativeResolver` | `ew-native-proof.invalid` | 1 | Actual callback error `kDNSServiceErr_NoSuchRecord` (-65554), stopped Network, deallocation before return |
| Active deadline | Actual `NativeResolver` | `example.com` | 1 | Successful subscription creation followed by Deadline and deallocation |

All cases run serially, without retries or alternate names. The two names are fixed public test material, contain no case data and are not supplied by the caller. No account, hosted search provider, key, credentials or remote operator is involved. The five cases can start at most **three DNSService subscriptions**. That is not a claim of three DNS wire packets: mDNSResponder may use a cache, issue multiple A/AAAA queries, or share daemon work. The callback report retains flags/error codes and counts; it does not publish local addresses, hostname, resolver configuration or answer addresses.

The active-deadline case has a private `cfg(test)` hook immediately after actual successful subscription creation. It expires only that execution window, then observes the ordinary check and destructor. It neither mocks FFI nor waits for a nondeterministic network timeout. The hook and thread-local lifecycle instrumentation do not exist in a non-test build. It proves the exercised native cleanup path, not a measured worst-case cancellation latency. The pre-cancel test alone does not establish stopping an already active resolver.

Each DNS stage has the existing five-second cooperative bound; the whole campaign stops starting work after twenty seconds. The current v2 harness keeps the caller's overall window separate from the internal DNS-stage bound, so a negative observation can be reported as Network after that stage rather than overridden by a coincident caller deadline. Its [separate source-bound observation](verification/macos-negative-dns.json) passed all five cases; the recorded v1 campaign below remains unchanged. The runner launches the exact compiled test binary with a thirty-second outer timeout. It kills and joins that owned process group on timeout, retains partial output and marks failure. Trusted DNSService creation, processing and deallocation system calls are not forcibly preempted by the Rust polling loop. A timed-out process is not a successful deallocation observation.

Apple's installed SDK `dns_sd.h` is the primary local reference and its hash is recorded. The corresponding [DNSService API header](https://github.com/apple-oss-distributions/mDNSResponder/blob/main/mDNSShared/dns_sd.h) defines `MoreComing` as queued batch information and documents serialized deallocation. Returned candidates are the bounded observed snapshot; `authoritative_complete_set` remains false. Deallocation and return from the local operation do not establish cessation of shared OS DNS daemon/provider activity.

## Reproduction and evidence

First commit the audited source with a verifiable signature and keep the checkout clean. Supply an SSH allowed-signers file containing the reviewed **public** signing key; this does not disclose or access a private key. Then run:

```sh
python3 scripts/test_native_collection_transport.py --allow-fixed-dns --allowed-signers /path/to/allowed_signers
```

The explicit flag authorizes only the fixed DNS scope above. Omitting it writes a failed/not-authorised observation without starting metadata checks or the test process. The runner refuses unsupported platforms, dirty/unsigned/unverifiable source, ambiguous binary selection, incomplete cases, failed cleanup, or a source/binary change around execution. Cargo builds offline with the existing lockfile; there are no new dependencies or downloads. It records commit/tree, signature verification, lockfile/runner/binary hashes, compiler, actual OS version/architecture and SDK header hash.

Every invocation has a unique ignored `artifacts/native-collection-transport/<id>/` directory. `report.json` exists before metadata collection, starts incomplete and retains failure phase/reason. Build/native logs remain there with hashes; partial native events survive timeout. Logs may contain development paths and are local evidence, not public artifacts. A sanitised verification record can reference their hashes and campaign ID after review. The offline Python evidence tests only test report integrity; their mocked events never establish native execution.

Source and binary identity are checked after failed/partial campaigns too. A later log-hash or report-write failure preserves the initial failure and adds an evidence error; it cannot turn an incomplete observation into success. Report replacement is atomic, so a failed final replacement leaves the previous incomplete report. When the filesystem itself cannot retain a final update, the returned/printed result reports that visibility failure.

The native case requires exact name enumeration, success flags and subscription/create/deallocate counts. An empty libtest selection, negative lookup timeout, generic daemon failure, duplicate case, or missing callback cannot pass. `NoSuchRecord` remains distinguishable in the report even though the existing private resolver maps its error to Network. No resolver policy or production outcome mapping is weakened for the campaign.

Windows has a different owned-context and provider-uncertainty contract and remains `not_run` here. Actual Windows cancellation/completion evidence is a separate native CI dependency. Compilation, callback unit tests, and this Mac development observation do not prove Windows execution, clean offline installation, complete RRsets, live durable receipts, HTTPS policy approval, or product readiness.

## Recorded development observation

The single authorised campaign from signed clean source `2a1d8d9399377b18ec263903104fa566d34114a7` ran on macOS 26.6.2 arm64. **Four cases passed and the negative-answer case failed.** Public resolution returned two policy-valid observed candidates in 1,537 ms. Pre-cancel and already-expired transport calls started no subscription. The active-deadline case created and deallocated its native subscription in 1 ms. The negative name produced no callback within 5,000 ms, returned Deadline, and deallocated its subscription. All three actual subscriptions were deallocated; no HTTP requests or repeat campaigns ran.

The subsequent offline primary-header review explains the negative observation: `kDNSServiceFlagsReturnIntermediates` (0x1000) documents delivery of intermediate NXDomain results, and states that NXDomain errors are not returned when the flag is absent. The resolver tested in that campaign passed flags zero. The test's stricter `NoSuchRecord` criterion remains unmet for that configuration. This was a test-expectation/configuration mismatch; it is not proof that negative DNS answers were received, nor evidence that the subscription leaked. The failed observation has not been converted into success. The separate [negative-handling correction](MACOS-DNS-NEGATIVES.md) has its own synthetic tests and native campaign record.

The callback contract says fields other than the error are undefined on error, so an address-family inference from those fields is invalid. The resolver still fails closed on the caller's deadline. The [source-bound verification record](verification/native-transport-macos.json) preserves the failed campaign, exact case observations, source/binary/log identities, ordinary checks and the earlier repaired offline test-fixture failure.
