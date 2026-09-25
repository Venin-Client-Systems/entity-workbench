# Inert current-source Java Search candidate

`scripts/prepare_java_search_candidate.py` prepares a separate ignored development runtime. It never invokes Java, Maven, a compiler, Search, a worker or a native test. Existing staging is read-only. No downloads, runtime activation or release claim are part of preparation.

The selected input is the Java 21/Search runtime pinned by the previous native campaign: digest `cb3f7cc163a851eec4f70ba11c96029458524e8465210720a57f318503aef9a7`, 269 files, 168,201,487 bytes. The replacement is the whole 74,334-byte current-source workers JAR, digest `a555216f95cd7ea44c9e16718e18bcd5b88f6089eac36680909192bfc9ca1877`. It contains the current Maven production classes; preparation does not filter or rewrite the archive. Only `search/workers-0.1.0.jar` changes. All nine Search dependencies and every JRE file retain their exact bytes and executable bits.

## Bound producer and source

Preparation requires clean trusted-signed source. The committed `offline-java-producer-6c467bd.json` evidence selects the exact successful producer receipt by size and SHA-256. Its producer commit signature is independently checked using the supplied trusted local signer file; both retained build outputs must match their recorded bytes and each other. The current tracked Java/POM set and bytes must still equal the producer inputs. Duplicate JSON keys, mismatched receipts, incomplete production, altered JARs and source drift fail.

The producer compiler was **JDK 25.0.2 targeting Java 21**, while the copied runtime is the existing **Java 21.0.12.1 JRE**. Only the new worker has this producer binding. The JRE and dependency JARs remain pinned existing artifacts; the combined runtime is not claimed to have been reproduced from source. The older staged worker is not retroactively relabelled.

## Copy and receipt

One invocation creates one fresh `artifacts/java-search-candidate/<UUID>` directory. It records `initial.json` with `prepared: false` before source validation or copying. The candidate lives under its `runtime` child, alongside the final receipt; no destination name comes from producer text or a URL.

The existing bounded runtime inventory checks ordinary canonical ancestors, no-follow directory/file handles, single-link regular files, exact file lengths/digests and executable bits. Its bounds are 1,024 entries, depth 8, 128 MiB per file and 256 MiB aggregate. Producer receipt reads are limited to 2 MiB; replacement JAR reads to 16 MiB. Verified source reads are reused. The destination must be new. Its files are created with descriptor-relative no-follow/exclusive opens; directories are `0700`, executable files `0700`, others `0600`. Previously unseen directories cannot be adopted, and known directory identities are checked. File data is synced, but there is no power-loss or durable-directory-publication claim.

Final verification requires the exact expected files, no extra root entries or empty directories, unchanged source runtime and producer inputs, unchanged signed source, and precisely one changed inventory row. The receipt includes the old and new complete inventories and their canonical native Search digests, producer receipt/source/JAR identities, and explicit execution/release limits. Existing inventory's conservative `rebuilt_from_current_java_source: false` remains unchanged for the combined runtime.

On failure, `prepared` remains false. Partial candidate files and diagnostics remain in the unique ignored directory; there is no cleanup, reuse, implicit retry or overwrite of another candidate. A caller must validate the final complete receipt and inventories before considering later use. Preparation does not authorize or execute a native campaign. A subsequent reviewed campaign must pin the new combined digest and establish its own actual runtime behavior.

Tests are synthetic. Pure receipt binding tests are portable; actual descriptor/private-mode tests are explicitly POSIX-scoped. There is no Windows preparation evidence, dependency-origin guarantee, hard RSS/latency claim, or full release acceptance here.
