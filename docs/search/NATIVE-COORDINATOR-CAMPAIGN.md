# Fixed native coordinator Search campaign

This source adds one ignored macOS arm64 development experiment, `coordinator::search::native::native_coordinator_search_campaign`. **It has not been executed by the source implementation or its ordinary tests.** A signed, clean source revision and separately reviewed explicit runtime digest are prerequisites for an eventual invocation. There is no application, schema, Java protocol, result DTO, runtime selection or confinement change.

## Finite cases

The healthy workspace imports three fixed synthetic TXT originals through the canonical store. The normal `JobCoordinator::start` attaches the explicitly provided staged runtime; actual `Command::Search` dispatch follows the repaired workspace-owner and durable-intent path. No processing or collection jobs are queued.

| Step | Query/control | Required observation |
| --- | --- | --- |
| 1 | `cooperative` | One index recipe and one search recipe; exactly the alpha original is returned at the unchanged canonical revision. |
| 2 | `Mira AND depot` | One search recipe; exactly the gamma original; the same index files, device/inode identities, contents and revision marker remain. |
| 3 | `(` | One search recipe; the existing typed `Validation` error; confirmed supervisor return and complete assignment/input/intent cleanup; unchanged index and canonical data. |
| 4 | `riverside` | One search recipe; exactly the beta original, using the same index after the known failure. |
| 5 | Separate host-injected retained-intent workspace | Startup quarantines execution; Search returns `Blocked` before **any** recipe entry; inert intent and index sentinel are unchanged. No unknown child is created by this control. |

There are exactly **five recipe entries** in the healthy workspace: Index, Search, Search, Search, Search. A `cfg(test)` thread-local guard sits at the fixed Search recipe entry before assignment creation; it refuses a reordered or extra entry. The second workspace uses a zero-entry guard. These observations count entry into the recipe, **not process spawns**. Successful results come from the real coordinator/Java path in the eventual native invocation; this observer alone is not execution or confinement proof.

The malformed-query observation is the current public error category, not an invented query-parser diagnostic. Both successful neighboring queries and fixed recipe order are required. A different error, false success, missing event or changed identity fails the campaign; there is no replacement case or automatic retry.

## Data and ownership checks

The trusted Rust test reuses the graph fixture's fixed-schema logical digest with visibility only changed. It includes every typed row of all six tables (`records`, `history`, `events`, `meta`, `derivative_objects`, `sqlite_sequence`), the full ordered schema and `user_version`; an unexpected table is rejected. Revision and digest must match before/after queries and after joined shutdown. This is a **Rust assertion over retained SQLite**, not independent SQLite reopening by the Python receipt reader.

The exact three original filenames, no-follow/single-link bounded file hashes and canonical `InspectSource` text anchors are checked. After joining, the same original-reader method is used directly under the retained workspace mutex because normal coordinator dispatch refuses all commands after stopping. There are no new canonical writes after fixture import. Retained workspaces permit a later independent database/original audit; raw SQLite file bytes are not compared because normal journal/page handling can change representation without changing logical content.

The healthy index snapshot permits at most 128 ordinary files, 8 MiB per file and 24 MiB logical total. Its byte hashes, file identities and marker are retained for comparison. Cache completion permits only `coordinator.lock` and `index`; any intent, assignment or staged input is a failure. These are deliberately small synthetic-fixture acceptance limits, not expanded production limits or a claim of general corpus capacity.

While healthy coordination is active, a second processing-lock acquisition must fail. Healthy shutdown must join, release ownership and permit a new lock acquisition. The inert-intent control must instead return a quarantined shutdown and retain ownership inside the test process. It does **not** advertise successful quarantine recovery or normal ownership release. OS process exit later ends that retained descriptor lifetime; there is still no application recovery procedure for an uncertain intent.

## Failure preservation and limits

Each query passes a typed completion check before index, canonical, original or runtime postreads. `TerminationUnverified`, `Cleanup` and unexpected failures stop the sequence; the original error category is preserved over a later shutdown error. Coordinator threads are joined, but the test does not read or clean worker-owned paths after unverified termination. No subsequent case runs on that path.

The runner writes `passed: false` / `not_started` before platform/source/runtime/build work. It retains ordered, nonce/source/runtime-bound events and a valid prefix when a later event is malformed. Only a complete strict native success permits the runner's runtime post-inventory. Later source/binary/receipt failure clears a prior success. Unknown outcome, incomplete logs or missing native completion are failures.

The existing worker wall/CPU budget remains 30 seconds per recipe. The fixed outer test bound is 180 seconds, with a 600-second offline source-build bound. These are different scopes; Search includes index preparation, multiple disposable workers and verification, and is not promised to finish in 30 seconds total. An outer timeout makes one bounded kill/join attempt for the owned Rust process group. The Java child has its own group: reaping Rust **does not prove Java termination**. The runner marks Java termination unverified, retains partial evidence, performs no dependent runtime/index/workspace postreads or cleanup and makes no further invocation. Existing private raw logs are retained; public receipts expose closed categories rather than worker exception text or filesystem paths.

## Explicit selected development runtime

The runner requires both `--runtime` and `--runtime-sha256`. It does not search PATH, download assets, infer an installation or call `java -version`. The inventory helper can compute a proposed digest without executing Java:

```sh
python3 scripts/native_search_runtime.py --runtime /absolute/reviewed/engines
```

Only fixed `java/` and `search/` trees participate. Both Python and Rust inventory readers reject path aliases, symlink ancestors, symlink/special/hardlinked files, unsafe names, incomplete required assets, changed entries, more than 1,024 entries, depth over eight, files over 128 MiB or aggregate over 256 MiB. Files are hashed through no-follow retained descriptors in 64 KiB chunks; a sorted compact JSON list of `[relative_path, bytes, sha256, executable]` defines the digest. Required assets include executable `java/bin/java`, `java/release`, `search/workers-0.1.0.jar` and dependency JARs. Independent Rust pre/post rows must equal the runner's pinned inventory.

The selected JAR is labelled a **pinned selected development artifact**. No retained producer record currently establishes that these staged bytes were rebuilt from the Java source at the campaign commit. Recording current Java source hashes does not establish that relationship. The inventory proves selected file identity, not complete bundle, dependency/loader closure, notices, signature/notarization, minimum OS support or release readiness.

An eventual reviewed invocation has this fixed form:

```sh
python3 scripts/test_native_coordinator_search.py \
  --execute-reviewed-search \
  --allowed-signers /private/trusted-signers \
  --runtime /absolute/reviewed/engines \
  --runtime-sha256 REVIEWED_SHA256
```

It creates one fresh ignored `artifacts/native-coordinator-search/<nonce>` directory. The private `invocation.json` retains the exact binary/runtime paths; `report.json`, `build.log`, `native.log` and the two synthetic workspaces are retained. Source identity includes clean signed commit/tree, all tracked Rust/Python/Java/build/profile source hashes, compiler identity and actual compiled test-binary hash. Receipt consistency, fixed fixture outcomes and runtime equality are checked independently by the Python reader. Echoed metadata alone is never accepted as execution proof.

## Remaining scope

This experiment does not change the frozen discovery benchmark or establish relevance, ranking quality, general exact-total precision, cold-corpus performance, search cancellation responsiveness, a bounded full-corpus capture, Windows Search activation, Intel runtime behavior or a packaged application runtime. Search still holds the workspace mutex through synchronous execution. Existing worker confinement evidence, broader hostile-resource tests and future ranked revision-bound query receipts remain separate.

Ordinary source tests use synthetic bytes, fake process outcomes and inert intent markers. They cover complete/partial/duplicate/mistyped receipts, substitution and revision drift, no retry/post-inventory after timeout, source/binary failure precedence, runtime links/aliases/count/depth/size/mutation, index replacement/residue, fixed recipe-entry order and typed unknown/cleanup refusal. They never run the selected Java executable.
