# Fixed native durable HTTPS proof — source only

This opt-in development harness connects the existing private NativeV3 collection lane to the actual native resolver and verified HTTPS transport. **No native HTTPS campaign has run for this increment.** The ordinary regression suite uses synthetic responses and no public networking. Production `NATIVE_COLLECTION_ENABLED` remains false, normal coordinator startup remains collection-disabled, standalone controls remain refused and the old synchronous collector remains disabled.

This is an operator-selected transport test using this project's own published synthetic Apache-2.0 fixture. It is not an independent public publisher, broad-discovery benchmark, relevance measurement or release gate. None of the frozen benchmark tasks, seeds or thresholds change.

## Exact approved source and limits

| Item | Fixed value |
| --- | --- |
| Selected host | `raw.githubusercontent.com` |
| First attempted URL | `https://raw.githubusercontent.com/robots.txt` |
| Only permitted seed | `https://raw.githubusercontent.com/Venin-Client-Systems/entity-workbench/df4dbff063d274effb9fa6385095938751bb300e/fixtures/brief.txt` |
| Expected seed original | 863 bytes; SHA-256 `1b728541f9939c78bd3432482c1b5894ff70ffc017ebbe07a2f4b0535ba99371` |
| Method | GET through the existing Rust transport |
| Charged attempts / transport calls | At most 2, in robots-then-seed order |
| Expansion hops | 0 |
| Cooperative campaign / outer process | 20 seconds / 30 seconds |
| Build limit | 600 seconds, offline, separate from the network budget |
| Retries / redirects / alternate seeds | None |

The fixture hash was checked against the **local Git object** at the immutable source commit and against the current checked-in fixture; this check made no network request. The implementation operator recorded review of [GitHub's Acceptable Use Policies](https://docs.github.com/en/site-policy/acceptable-use-policies/github-acceptable-use-policies) on 2026-09-26 for this bounded non-personal research/archive test. The fixture is the project's own public synthetic original. This records the operator's source selection and review, not perpetual permission, independence, or permission to collect other GitHub content. No new terms or source request was made by the harness author during implementation.

Runtime robots handling remains authoritative. A fully retained usable 200 or 404 robots response is required before the seed can be attempted; disallow, unsupported crawl-delay, redirect, denial, malformed/oversized response or other policy failure stops the run honestly. Existing policy does not follow robots redirects. A seed redirect cannot consume a third request. No convenient replacement URL, retry or relaxed access rule is introduced if this source is blocked. Only the exact two URLs are admitted by the additional closed guard.

The disclosure preview binds the exact input, policy, selected host, robots URL and disclosure object through the same Rust validation/hash function as the public API. URLs disclose only the fixed public fixture path. DNS hostnames and connection metadata are disclosed. No query, case contents, addresses, credentials, cookies or account data are sent. The existing fixed User-Agent is `EntityWorkbench/0.1 (analyst-directed public collection)`; transport disables proxy use, automatic redirect/retry, cookies/referer and connection pooling, uses verified TLS and requests identity encoding.

Two permitted transport calls do not imply two DNS wire packets or exactly two TCP connection attempts. The native resolver may use cached/shared OS work, and the connector may attempt another address within the validated pinned snapshot. The proof makes no complete-RRset or shared-provider-quiescence claim.

## Real coordinator, closed test seam

`coordinator_native_https_proof.rs` is compiled only under `cfg(test)`. Its live ignored entry point is macOS-only. The normal executable has no new command, feature, URL override, runtime mode or environment activation mechanism.

The harness obtains the normal pure preview, confirms its hash/input and privately queues one canonical NativeV3 record with a unique request key. It then calls the **existing** `JobCoordinator::with_execution`, using the normal shared ownership, claim, reserve, transport-outside-mutex, settlement and joined shutdown paths. Public production queue admission is deliberately not activated or claimed tested by this native seam; synthetic API tests cover that contract separately.

A closed guard wraps the actual `collection_transport::fetch` before DNS/socket resources can be created. It checks the fixed input/limits, exact run ID, generation 1, stable canonical lease, sequence 0/1, exact URLs, ordering and the original campaign deadline. Request purpose and ancestry are verified afterward from the canonical receipts; the transport ticket itself carries the URL rather than those fields. Any refusal is sticky and fails the campaign. A third call, redirected URL, changed scope, lease, generation or run never reaches `fetch`. The canonical reserve has already committed its charge before the guard runs; a refusal never refunds it. All resolved addresses still pass the existing destination policy, and only the observed pinned snapshot reaches the connector.

Each actual returned observation is emitted before settlement and compared exactly with its published lossless receipt. Complete error/robots bodies retain originals; incomplete bodies have no invented hash. A passing positive proof requires a usable robots response, a complete status-200 seed whose retained original matches the frozen bytes/hash, native resolver metadata, identity encoding, no stop/uncertainty, preserved ancestry and two charged attempts. Current promoted text remains unreviewed mutable Evidence interpretation; no immutable web derivative or accepted anchor is claimed.

After joined shutdown, the harness reopens the workspace and compares the canonical run/receipt data, then creates and restores a normal backup and verifies the same run, receipts and referenced originals again. No accepted observation is created. Unknown completion retains quarantine/ownership and cannot claim ordinary shutdown, release, successful reopen recovery or backup proof. Pending publication is not hot-retried; it remains a failed observation for explicit later review.

## Source-bound execution and retained failures

The runner requires a clean signed source commit and the reviewed trusted public allowed-signers file. It builds the exact test binary offline, embeds the source commit in the test build, records its SHA-256 and selects exactly one ignored test. Runtime source identity must match the compiled source. A unique canonical UUID binds every queued/launch/observation/final event and its fresh evidence directory. No source/binary change around execution is accepted, including after failed or partial runs.

`report.json` is created incomplete before metadata inspection. Build/native logs and the fresh workspace, originals and restored backup remain in an ignored `artifacts/native-durable-https/<nonce>/` directory. Only metadata and hashes belong in a future sanitized public observation; source bodies need not be published. The Python validator requires the exact event sequence, independently recomputes the preview hash, verifies fixed URLs/counts/source/nonce, compares pre-publication observations with canonical receipts, and refuses duplicate keys, missing events, synthetic mode, changed fixture hashes, unsafe candidates and unresolved cleanup. Zero selected tests cannot pass.

At the outer 30-second limit, the runner attempts one kill signal to the owned test process group and waits up to five further seconds for its child process to exit. The first timeout remains the primary failure even if signaling or reaping fails. The receipt records signal delivery and confirmed child exit separately, without claiming that either proves native resource deallocation, termination of every descendant, or cessation of shared OS/provider activity. An unconfirmed exit requires operator review; the runner never launches a replacement. Trusted OS calls are not forcibly preempted by the transport polling loop.

Malformed or truncated event output retains the parsed, duplicate-key-free object prefix plus an explicit decoding failure; it cannot skip the bad event and pass. Partial events are diagnostic observations, not a successful validated campaign. Raw logs remain available. A later evidence-hash/write failure preserves the initial failure and cannot turn an incomplete run into success. There are no automatic reruns.

No native invocation is authorized merely by checking in the following reproduction command. The exact signed source must first be reviewed for the fixed scope:

```sh
python3 scripts/test_native_durable_https.py \
  --allow-fixed-https \
  --allowed-signers /path/to/reviewed-public-allowed-signers
```

Offline checks, which perform no DNS/HTTPS calls:

```sh
cargo test -p workbench-core --lib fixed_https --locked --offline
python3 -m unittest discover -s scripts/tests -p test_native_https_receipts.py -v
python3 -O -m unittest discover -s scripts/tests -p test_native_https_receipts.py -v
```

Windows native HTTPS, active native HTTP cancellation, installed-platform behavior, public admission, UI enablement, broad coverage and complete release remain separate requirements. The earlier Windows DNS cancellation observation remains provider-uncertain even when caller storage was released; this harness does not reinterpret it. Offline negative fixtures preserve that distinction without invoking Windows FFI.
