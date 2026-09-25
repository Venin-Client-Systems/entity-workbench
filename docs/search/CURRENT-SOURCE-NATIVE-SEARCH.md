# Current-source Java Search through the native coordinator

One native campaign passed on the available Apple Silicon Mac at clean signed
source `39e669f1b296da472d2694e447511fa4e8953f55`. It executed the new worker from
the [two reproducible offline Java builds](OFFLINE-JAVA-PRODUCER.md), using the
isolated [prepared runtime candidate](CURRENT-JAVA-CANDIDATE.md). The existing
Search campaign runner, cases, limits and acceptance rules were unchanged.

The [exact native report](verification/native-coordinator-39e669f.json),
[independent root audit](verification/native-coordinator-39e669f-root.json) and
[producer-to-execution binding](verification/native-coordinator-39e669f-provenance.json)
retain separate evidence for this observation. The earlier campaign with the
older staged JAR remains unchanged and is not used as execution evidence for
the replacement worker.

## Observed behaviour

| Step | Result |
| --- | --- |
| `cooperative` | Created the index and returned only `alpha.txt` at revision 3. |
| `Mira AND depot` | Returned only `gamma.txt`, reusing that index. |
| `(` | Returned the typed validation error; assignment, input and intent cleanup completed. |
| `riverside` | Returned only `beta.txt` after the failed query, with the same index. |
| Separate inert retained intent | Refused Search before any recipe entry; retained the intent and sentinel, with quarantined shutdown. |

There were five guarded recipe entries: one Index and four Search. These are
recipe-call observations, not an independent count of OS process launches. The
native interval was 18,887 ms, below the unchanged 180-second outer bound; each
worker retained its 30-second limit. There was one campaign invocation, no
warm-up, replacement case or retry. This tiny fixture is not a performance
benchmark. Healthy shutdown joined and released ownership. The inert-intent
control did not create an unknown child and does not establish recovery from one.

## Source and retained-data checks

The executed worker is 74,334 bytes with SHA-256
`a555216f95cd7ea44c9e16718e18bcd5b88f6089eac36680909192bfc9ca1877`.
It equals both retained producer outputs. All 20 tracked Java source/POM inputs
match between producer source `6c467bd` and native source `39e669f`.
The producer used JDK 25 with `release=21`; the selected runtime uses the
previously pinned Java 21 JRE. Only the worker JAR changed in the candidate.

The 269-file runtime contains 168,265,891 bytes and has inventory digest
`a34ea61a64a823ea77180079126c9eba9b2da801ab3c48fc4962acad23c35570`.
The JRE and nine Search dependencies retain their existing byte and executable
pins. They were not rebuilt by the producer. The unchanged native report's
conservative `rebuilt_from_current_java_source: false` does not describe this
separate worker provenance chain and is preserved verbatim.

Root independently checked all 397 collected source identities, the actual
executable, 14 ordered events and the complete runtime inventory. The preserved
test executable has SHA-256
`43582368fcecd5f391f54df830fe47968d03f6a139ff70fcdf9552e5751ec0b2`.
Read-only SQLite reconstruction matched both canonical digests, including the
schema, user version and typed contents of all six tables. Both databases
passed integrity checks, remained at revision 3 and retained exactly their
three evidence records and original files. The healthy index file identities
and the separate intent/sentinel identities matched the receipt. A second
agent independently checked this new campaign and its producer chain and found
no remaining mismatch. This is agent review, not independent human approval.

Preparation guards passed eight synthetic tests normally and under Python
optimization. The combined producer/preparation suite passed 19 tests. An
initial root invocation named a nonexistent producer test module after the
eight candidate cases passed; correcting that command passed on unchanged
source. No Java or native campaign was repeated for that command correction.

## Remaining scope

This establishes current-source worker compatibility through the native
coordinator on this development Mac. It does not establish an installed Tauri
application, Windows or Intel runtime, signed helpers, large-corpus performance,
ranking quality or broad public-web discovery coverage. Intent recovery,
bounded corpus capture and ranked query receipts remain open. All twelve
complete-release gates remain unpassed.
