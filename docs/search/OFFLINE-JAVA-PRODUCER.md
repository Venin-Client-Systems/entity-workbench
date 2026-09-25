# Offline Java Search development producer

`scripts/build_java_search.py` produces the current Maven workers JAR in isolated ignored artifacts. It establishes a new local producer observation; it does not retroactively establish the source of an existing staged JAR. No runtime staging directory is written, no software or dependency is fetched, and neither worker entry points nor Maven tests run.

The existing Mac staging script copies the complete `workers-0.1.0.jar` and selects Lucene/Jackson runtime libraries from `target/lib`. This producer preserves that artifact meaning. It does not create a new class-filtered Search JAR, modify the POM, activate a runtime, or run Search. The current POM pins its declared dependencies but is not a complete transitive Maven lock. Existing bundle inventory tools verify file identity; they do not supply missing build provenance.

## Exact inputs and closed build

The command requires an explicit installed JDK, Maven distribution, existing local Maven repository, trusted signing-key file, and staged Search directory for read-only comparison. It never bootstraps these prerequisites or selects them from PATH. The actual JDK/compiler is identified separately from the POM's Java 21 classfile target. In the current environment the selected compiler is **Temurin JDK 25 with `--release 21`**, not a Java 21 compiler.

Before tooling executes, the runner retains an initial failed/incomplete receipt. It requires a clean signed source commit and captures exact tracked Java/POM and Python tooling bytes. All Java build-directory inputs must be tracked. JDK and Maven distribution inventories include relative names, bytes, hashes and executable bits. JDK/compiler and Maven version commands have separate 30-second limits and private logs; they do not invoke application workers.

Each fresh build receives a separately verified source copy and local Maven cache copy. The existing cache is a bounded private input snapshot, **not a claimed minimal resolver closure**. It is limited to 4,096 files, 8,192 total entries, depth 20, 256 MiB per file and 512 MiB aggregate. No-follow ordinary single-link reads check identities before/after; changed, linked, special, oversized or excess inputs fail. Copy destinations must be new. Private manifests retain cache names; the public receipt exposes only its aggregate identity/count/bytes plus the selected build plugin and actual output dependency identities. Unrelated cache coordinates and private paths are not public fields.

The subprocess environment excludes inherited classpaths, Java agents, Maven arguments and user startup scripts. Explicit empty user/global settings and toolchains, an explicit copied local repository, and this closed goal/options set are used:

```text
package
--offline --batch-mode --no-transfer-progress --strict-checksums
-Dmaven.test.skip=true
-Dmaven.compiler.proc=none
-Dmaven.compiler.fork=false
-Dcyclonedx.skip=true
-Dproject.build.outputTimestamp=2020-01-01T00:00:00Z
```

`--offline` disables Maven repository resolution over the network. It is **a build setting, not a network sandbox claim**. Only the reviewed local project/plugins are executed. The fixed timestamp is supported by the installed Maven JAR plugin's primary plugin descriptor; the CycloneDX skip setting is likewise declared by its cached descriptor. Actual plugin/version/goal headers are checked against the fixed selected plugin list. Compiler annotation processing, Maven test compilation/execution and BOM generation are skipped explicitly. No arbitrary extra goals, arguments or caller-supplied POM are accepted.

## Outputs and equality claims

At most two Maven package builds run, with separate 600-second bounds. A failed first build stops the sequence; there is no automatic resolution retry, second attempt or online fallback. The outputs remain beneath the fresh artifact directory. On success the runner requires:

- Exact whole-JAR byte identity and runtime dependency identities across both builds.
- Every tracked production Java source has its top-level class in the produced JAR; each class targets version 65 (Java 21).
- Bounded, unique, safe ZIP members; member identity is recorded separately from whole-JAR identity. Equal class bytes with different ZIP metadata do not count as equal JARs.
- Every copied runtime dependency JAR is exactly matched to one initial cached input. The existing Mac Lucene/Jackson selection is reported separately.
- Source, selected tools, original Maven cache, and staged Search inputs are unchanged after both builds.

The produced JAR is compared to the exact existing staged JAR, and the selected dependency set is compared independently. An unequal JAR retains `staged_worker_byte_equal: false`. Two reproducible current builds alone never change that field to true or establish historical producer provenance. A later native test of a new artifact would be a separate reviewed action; the existing native Search receipt remains bound to its original staged bytes.

## Retention and limits

The receipt, build/version logs, exact private invocations, private input manifests, both copied repositories/projects and produced artifacts are retained. Missing cached plugins/dependencies, tool failure, timeout, output inequality or identity drift remains an explicit unsuccessful receipt. A timeout makes one bounded process-group stop and direct-process reap attempt; it never implies arbitrary descendant quiescence or permits a second build. Primary failure categories survive subsequent evidence errors.

The current producer is a POSIX/macOS development tool. Pure command/receipt tests are portable; tests exercising real no-follow descriptors and mode `0600` are explicitly POSIX-scoped. Synthetic tests do not invoke Maven or Java. There is no Windows producer, dependency origin/signature guarantee, general hermetic-build claim, hard RSS/latency guarantee, release bundle claim or staged runtime replacement in this slice.
