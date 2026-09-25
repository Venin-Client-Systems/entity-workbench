# Bounded corpus capture through the native Search worker

One campaign passed on the available Apple Silicon development Mac at clean signed
source `439b772798e34e2d7f2b5352e7b167e9563b6813`. It exercised the new
[atomic bounded corpus capture](BOUNDED-CORPUS-CAPTURE.md) through the existing
coordinator and actual confined Lucene worker. The fixed campaign recipes,
acceptance rules and runtime were unchanged.

The [exact native report](verification/native-coordinator-439b772.json),
[root audit](verification/native-coordinator-439b772-root.json) and
[worker provenance](verification/native-coordinator-439b772-provenance.json)
record this observation separately from the earlier native campaigns. There
was one invocation and no retry. The native interval was 17,688 ms; the unchanged
limits were 30 seconds per worker, 180 seconds for the outer native test and
600 seconds for its offline build. These small-fixture timings are not a
performance benchmark.

| Observed operation | Result |
| --- | --- |
| Initial `cooperative` query | Created the index; returned only `alpha.txt` at revision 3. |
| `Mira AND depot` | Returned only `gamma.txt`, reusing the index. |
| Malformed `(` query | Returned a typed validation error and completed assignment, input and intent cleanup. |
| Later `riverside` query | Returned only `beta.txt`, reusing the same index after the failed query. |
| Separate retained-intent workspace | Refused Search before any recipe entry and preserved the intent and sentinel. |

Five guarded recipe entries—one Index and four Search—are recipe observations,
not an independent count of operating-system process launches. Healthy shutdown
joined and released ownership. The retained-intent control finished quarantined;
it did not create an unknown child and does not prove recovery from one.

After the accepted termination receipt, root checked all 413 collected source
identities, the preserved test executable, 14 ordered events and the complete
269-file runtime inventory. The binary SHA-256 is
`032d664e8876b15797e11619fb7662eb54c3728314714ca571bdf4058ad0990a`.
Read-only reconstruction of both complete SQLite databases matched the trusted
typed logical digests, including their schemas and all six tables. Each remained
at revision 3 with three evidence records and three unchanged originals. Healthy
index files and the separate intent/sentinel retained their recorded identities.
A second agent independently verified those source, binary, receipt, database,
original and runtime identities, including the producer chain. It also reacquired
and released healthy workspace ownership without changing canonical data. No
concrete mismatch remained. This is agent review, not independent human approval.

The runtime inventory remains
`a34ea61a64a823ea77180079126c9eba9b2da801ab3c48fc4962acad23c35570`.
Its worker is the previously produced 74,334-byte JAR with SHA-256
`a555216f95cd7ea44c9e16718e18bcd5b88f6089eac36680909192bfc9ca1877`.
All 20 tracked Java/POM inputs still match the two reproducible offline producer
builds. The Java runtime and third-party dependencies were not rebuilt by this
campaign; the conservative original report fields remain unchanged.

The [combined source checks](../verification/integration-access-search.json)
passed 741 ordinary Rust tests, with 34 native/opt-in cases ignored, plus strict
host Clippy and formatting. They cover the atomic snapshot race and oversized
input refusal independently of this tiny native fixture. The
[source handoff](verification/bounded-corpus-capture-source.json) additionally
retains strict Intel/release checks and 11 normal plus 11 optimized Python
receipt tests. No UI source changed or new browser workflow run is claimed.

This verifies the new capture's compatibility with actual native execution on
this Mac. It does not establish acquisition-scoped Search, off-lock dispatch,
intent recovery, large-corpus latency, ranked discovery quality, a complete
installed runtime or another platform. All twelve release gates remain false.
