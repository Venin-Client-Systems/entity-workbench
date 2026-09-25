# Fixed owned-response cancellation proof

This is **source-only preparation** for one separately reviewed, opt-in Mac cancellation observation. No native DNS or public HTTPS request was made for this change. Production `NATIVE_COLLECTION_ENABLED` remains false; normal startup has no live collection lane, public admission remains unavailable, and the old synchronous collector stays disabled. No command, schema, source-selection policy or normal runtime timing override changes.

The proof targets **one owned response after verified HTTPS headers, before application body consumption**. Its test hook pauses there while the real coordinator commits cancellation intent and signals the existing token. The hook returns normally after observing that token; the ordinary transport body wait must then produce `Stopped(Cancelled)`. It does not synthesize a cancellation outcome. The server may already have sent the entire small response, and the OS or HTTP client may have buffered it. A passing future observation would establish cancellation and disposal of that owned response at this point, **not** interruption of server delivery, stalled wire reads, every DNS/TLS/request phase, Windows cancellation, or the complete production activation gate.

## Fixed source and budgets

The source/access review is the same operator-selected, project-owned synthetic fixture recorded in [the positive HTTPS proof](NATIVE-DURABLE-HTTPS-PROOF.md). That document retains the actual earlier positive observation separately; those report bytes remain unchanged. This is not a publisher-independence or broad-discovery benchmark. No additional access review or remote request was made during implementation.

| Item | Closed value |
| --- | --- |
| Host | `raw.githubusercontent.com` |
| First GET | `https://raw.githubusercontent.com/robots.txt` |
| Only seed GET | `https://raw.githubusercontent.com/Venin-Client-Systems/entity-workbench/df4dbff063d274effb9fa6385095938751bb300e/fixtures/brief.txt` |
| Attempts / transport launches | At most 2, robots then seed |
| Hops / redirects / retries / alternatives | 0 / none / none / none |
| Cooperative campaign / outer native process | 20 seconds / 30 seconds |
| Response handshake | At most 5 seconds, also capped by remaining transport time |
| Outer process stop accounting | One kill attempt, up to 5 seconds for exit confirmation |
| Offline build | 600 seconds, separate from network timing |

The same pure preview binds the exact input, policy, selected host, robots URL and disclosures. No local case contents, person addresses, credentials, cookies or arbitrary URL enter the proof. Robots remains authoritative. A refusal, disallow, unusable response, redirect or failed handshake fails the proof and cannot trigger a replacement request. The frozen fixture identity is retained as **source-selection identity**, not as a claimed downloaded seed hash: this cancellation proof must not retain a complete seed original or promote seed text.

The existing closed guard executes before `fetch` creates native resolver/socket resources. It checks the exact input, sequence, URLs, run, generation, stable lease and original campaign deadline. Request purpose/ancestry is checked against the resulting canonical journal, because the transport ticket carries a URL rather than a purpose field. Charges are committed before the guard and never refunded. Two transport launches do not imply two DNS wire packets or exactly two TCP attempts; only bounded, validated observed candidates may be tried, with no fallback resolver. No full DNS answer set or shared-provider quiescence is claimed.

## Existing coordinator and ordinary stop path

All additional control seams compile under `cfg(test)`. The native ignored entry point additionally requires macOS. The executor still uses the existing native resolver, public-address validation, pinned verified TLS, no proxy/automatic retry/redirect, identity encoding and incremental bounded body path. No alternate networking worker or separate coordinator is introduced.

A single-use bounded channel reports the actual response head after remote pin validation. The controller emits cancellation intent, then calls `JobCoordinator::cancel_collection` with the exact run and generation. That normal method commits durable intent before signalling the active token. Its acknowledgement and the worker's returned observation can race; the validator accepts only the two legitimate orders after intent and requires both before the final record. A dropped receiver, repeated gate entry or expired handshake is an explicit failed proof, never a substitute cancellation acknowledgement. The hook cannot start another request and has no public timing option.

A passing future receipt must show all of the following:

- A usable, complete robots original; two irrevocable charges and exactly two guarded launches.
- Actual native observed candidates, no resolver uncertainty, identity encoding and a status-200 seed response head.
- The seed's ordinary transport result is `Stopped(Cancelled)` in the body phase with cancellation observed, local resource closure confirmed and no seed original/digest or promoted page.
- Durable cancellation acknowledgement, terminal `Cancelled`, unchanged accepted observations, and exact equality of pre-publication observation and canonical receipt.
- Joined shutdown, released workspace ownership, identical canonical state on normal reopen, and the same receipts/referenced original after backup and restore.
- A terminal resume attempt under freshly held ordinary coordinator ownership is refused without changing revision or canonical bytes. Public controls stay unavailable.

Unknown resolver/transport completion, failed settlement, a backward clock or timeout remains a failed observation with its actual evidence. There is no automatic settlement retry, native rerun or scope fallback. Confirmed process exit from an outer kill is separate from native resource cleanup and cannot establish provider quiescence. Trusted OS calls are not forcibly preempted by the cooperative polling interval.

## Source binding and failure retention

`scripts/test_native_collection_cancellation.py` is a separate fixed CLI. It reuses the existing signed-clean-source/offline-build runner and timeout/error accounting. Only the cancellation profile can be selected by this CLI; there is no URL, method, test-name, deadline or budget argument. A compiled commit ID, runtime commit ID, unique canonical nonce, exact test name, binary hash, source tree and relevant file hashes bind the future campaign. The existing positive validator still accepts the unchanged actual positive receipt; its result is not rewritten into a cancellation receipt.

A fresh incomplete `report.json` is saved before source/build inspection under ignored `artifacts/native-collection-cancellation/<nonce>/`. Build logs, native output, workspace, backup and restored workspace are retained. Duplicate-key-free valid event prefixes survive malformed/truncated tails; those tails fail validation. The first outer timeout remains primary if signaling or reaping also fails. Signal attempt, confirmed exit and fixed error categories are separate. Source and binary identity are checked after failed as well as successful runs. Zero selected tests, missing cancellation intent, missing acknowledgement, synthetic mode, substituted scope, phantom body identity, unconfirmed cleanup and unsupported event order cannot pass. No source body needs public publication.

Checking in this command **does not authorize execution**. Root must first review the exact signed source and approve a single fixed campaign:

```sh
python3 scripts/test_native_collection_cancellation.py \
  --allow-fixed-cancellation \
  --allowed-signers /path/to/reviewed-public-allowed-signers
```

## Offline verification layers and remaining work

The canonical fixture drives real queue/reserve/cancel/settle/reopen/backup operations with explicitly synthetic transport observations. A separate local TLS/socket test pauses the real HTTP implementation at headers, cancels its actual token, checks its typed result, and observes owned connection closure. Both are ordinary tests with no public DNS/HTTPS. Missing receiver and expired handshake controls prove these failure paths do not manufacture cancellation. Python tests use clearly synthetic receipts, malformed/missing events and mocked subprocess failures; they do not constitute native evidence. They also revalidate the earlier retained real positive receipt without contacting its source.

```sh
cargo test -p workbench-core --lib fixed_cancellation --locked --offline
cargo test -p workbench-core --lib fixed_https --locked --offline
python3 -m unittest discover -s scripts/tests -p 'test_native_*receipts.py' -v
python3 -O -m unittest discover -s scripts/tests -p 'test_native_*receipts.py' -v
```

Even after a successful future Mac observation, activation still requires separately reviewed coverage of other native cancellation phases, Windows provider-uncertain outcomes, retained unpublished settlement/recovery and installed-platform behavior. Existing synthetic coordinator coverage remains distinct from native evidence. No production or release gate changes here.
