# Native coordinator Search results

One fixed native campaign passed on the available Apple Silicon Mac from clean
source `792b2176bd3a3c0a845f57fc1c189b57a4c0d623`. It used actual coordinator
dispatch, the existing confined Java/Lucene path and three synthetic text
originals. It exercised the [reviewed campaign contract](NATIVE-COORDINATOR-CAMPAIGN.md)
without a retry, replacement case, warm-up, dependency download or larger limit.

The [exact host report](verification/native-coordinator-792b217.json),
[independent root audit](verification/native-coordinator-792b217-root.json) and
[combined source checks](../verification/integration-native-search-792b217.json)
retain the source, executable, runtime and evidence identities. The raw build log,
private invocation paths, test executable and synthetic workspaces remain retained
locally. The selected Java/JAR files are pinned development artifacts; a rebuild
from the current Java source is not claimed.

## Results

| Step | Actual outcome |
| --- | --- |
| `cooperative` | Created the index and returned only `alpha.txt` at revision 3. |
| `Mira AND depot` | Returned only `gamma.txt` using the same index. |
| `(` | Returned the existing typed validation error and cleaned the assignment, staged input and intent. |
| `riverside` | Returned only `beta.txt` after the known failure, using the same index. |
| Separate retained-intent control | Returned Blocked before any recipe entry; retained the inert intent and index sentinel and reported quarantined shutdown. |

The healthy workspace recorded exactly five recipe entries: one Index followed
by four Search entries. These count calls to the guarded recipe boundary, not
independent OS process-spawn observations. The queries used the real worker path;
their replies were not injected. Every index file retained its device/inode,
size and content hash across all four queries. Healthy shutdown joined and
released workspace ownership. The separate retained-intent workspace deliberately
kept quarantine until test process exit; it does not demonstrate recovery from a
real unknown child.

## Independent retained evidence checks

Root matched all 387 source files in the campaign collector to their immutable Git
blobs, checked and preserved the actual executable, and revalidated all 14 ordered
events. The actual executable SHA-256 is
`85bb532c6498c181c9fd7731ef22f63635d828f5883a263d4b962504629f574b`.

A separate read-only SQLite reconstruction matched both retained canonical
digests, covering the full schema, user version, all six tables and typed values.
Both databases returned `integrity_check = ok`, remained at revision 3 and retained
exactly their three evidence records and originals. Root independently matched the
healthy index identities and the refusal workspace's intent/sentinel identities.
This later check adds independent database inspection to the runner's receipt
validation; it did not rerun Java.

The 269-file Java/Search inventory contained 168,201,487 bytes and had digest
`cb3f7cc163a851eec4f70ba11c96029458524e8465210720a57f318503aef9a7`.
The Rust and Python inventories matched before/after execution, and root's later
read-only inventory matched again. The native interval was 18,624 ms for this
single tiny fixture, under the fixed 180-second outer limit. This is not a latency
benchmark or general search capacity claim.

## Limits

This verifies the repaired coordinator Search lifecycle on a development Mac.
It does not qualify a Tauri UI session, installed application, Windows or Intel
runtime, signed helper, broad-web discovery coverage, ranking quality or general
exact-total semantics. Synchronous Search still holds the workspace mutex;
bounded corpus capture, ranked query receipts, intent recovery and responsiveness
remain separate work. The required large-corpus benchmark and complete offline
packages remain unfinished. All twelve complete-release gates remain unpassed.
