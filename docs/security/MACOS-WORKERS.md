# macOS development Java worker supervision

This implements a bounded part of **EW-04 / issue #8**. The Rust application now launches its Java/Lucene index and search workers through a narrower development profile and supervisor. It does **not** pass the signed-helper release gate. `sandbox-exec` is deprecated in the installed Apple manual; signed App Sandbox helper design, entitlements and compatibility remain required before release.

## Assigned access

Each request receives a new private job directory, removed after success or failure. Rust retains the canonical workspace and originals. A coordinator-owned lock serializes access to the local index. A failed rebuild removes the revision marker so the next attempt cannot mistake partial output for a current index.

| Resource | Index job | Search job |
| --- | --- | --- |
| Assigned input and bounded stdin request | Read | Read |
| Result file and private scratch directory | Read/write as needed; result accepted by Rust only after validation | Same |
| Assigned Lucene index | Read/write | Read only |
| Other job directories, workspace data, originals | No content access | No content access |
| Packaged Java and search adapter/library directories | Read/execute mapping | Read/execute mapping |
| `/System/Library`, dyld bootstrap paths, `/usr/lib` and entropy devices | Required runtime reads | Same |
| Direct networking | Denied | Denied |
| Fork/child execution | Fork explicitly denied | Fork explicitly denied |

The profile permits filesystem metadata queries and system-control reads for JVM compatibility. It does not promise to hide the existence of all host paths. The job directory itself is readable because JVM initialization uses the working directory; this does not grant reads of arbitrary child files. Rust supplies the assigned index through a JVM property; a missing or denied path fails the job. There is no unsandboxed fallback. Windows/Linux retain the existing blocked worker result.

The launcher clears the caller environment, retaining only the OS HOME value because the observed development host's `sandbox-exec` requires it. That value grants no filesystem permission. Java `user.home` and temporary files point to private scratch. Caller descriptors above stderr receive `FD_CLOEXEC` before execution; stdin is the assigned request, stdout/stderr are discarded. The native test deliberately introduces a non-CLOEXEC descriptor and verifies it does not reach a child. The fork hook uses only syscalls and OS errors; its invariants are documented next to the unsafe code. [Rust CommandExt](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html), [Apple fcntl manual](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fcntl.2.html).

## Limits and cleanup

| Limit | Enforcement |
| --- | --- |
| Wall time | 30 seconds per production request; supervisor terminates dedicated process group |
| CPU time | `RLIMIT_CPU` 30 seconds |
| Individual file size | `RLIMIT_FSIZE` 64 MiB; native overrun probe fails |
| Open descriptors | `RLIMIT_NOFILE` 256 |
| Core dump size | Zero |
| Result bytes | 1 MiB maximum; no-follow/nonblocking open, regular file, one link, bounded read |
| Job plus index tree | 128 MiB / 512 entries / bounded nesting checked during polling and at exit |
| JVM heap | 256 MiB maximum Java heap |

The tree limit is a monitored budget, **not a hard aggregate filesystem quota**. A writer can exceed it between 20 ms checks. The heap cap is **not a resident-memory limit**; native allocation and mappings remain unbounded by a proven OS memory ceiling. CPU/descriptor limits are configured but have not been stress-tested individually. A malicious worker's symlinks, hard links, special files and oversized final results are rejected; the index is checked before it is assigned again. Index contents remain untrusted rebuildable derivatives.

The leader stays unreaped while the supervisor detects exit using `waitid(..., WNOWAIT)`, terminates its process group and then reaps it. This avoids signalling a recycled PID after `try_wait`. Cleanup also runs on timeout, tree violation or supervisor error, signalling the tracked leader directly as well as the group in case it changed groups. A separate disposable shell test creates a descendant and verifies it is gone or awaiting reaping after timeout. The actual Java profile denies spawning that child in the first place. The process-group mechanism is not a general containment mechanism for arbitrary workers that can fork and escape their group. [Apple resource-limit manual](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/setrlimit.2.html).

These limits are temporary development limits for small corpora. No 10,000-page throughput or large-corpus performance claim follows from them. A supervisor process crash can leave scratch files; full crash-recovery and scratch lifecycle tests remain open. Host administrator/same-user tampering with the runtime or workspace is outside this probe's threat boundary.

## Repeat the native probe without downloads

A staged Java 21 runtime must exist at `runtime/staged/engines`, with the current `SearchWorker`, `Protocol` and `HostileProbe` compiled into `search/workers-0.1.0.jar` and matching Lucene/Jackson JARs in `search/lib`. Use an already installed JDK to compile the three sources with `javac --release 21`; package their compiled classes with `jar --create`. Alternatively rebuild the existing Maven worker project and restage with `scripts/stage_macos_engines.py`. These are developer operations; end users do not compile or download dependencies.

```sh
python3 scripts/test_macos_confinement.py
# Or use an existing staged directory without changing it:
python3 scripts/test_macos_confinement.py --runtime /path/to/staged/engines
cargo test --locked -p workbench-core
cargo clippy -p workbench-core --all-targets -- -D warnings
```

The Python entry point exercises the actual Rust production launcher, replacing the old separately maintained Python profile. It runs six tests sequentially, including the two explicitly opted-in Java tests. Serial execution avoids unrelated test child launches briefly inheriting the index lock before exec closes their descriptors. A concurrent production index request can return a retryable blocked result while its coordinator lock is held. The hostile test uses only temporary synthetic sentinels and an ephemeral loopback listener. An unconstrained baseline first proves each forbidden attempt works with those same disposable inputs. The constrained run then checks outside/sibling reads (including the `/System/Volumes/Data` alias), original/input writes, networking, child spawning, environment leakage and profile reads fail, while input/scratch and operation-specific index access work. Further probes exceed file size and wall time. The real application adapter indexes a synthetic notice and checks phrase, Boolean, proximity, fuzzy and fielded searches, failure cleanup and revision invalidation.

Each run retains a timestamped report in `artifacts/confinement/`, including failed attempts. `artifacts/confinement-result.json` is the latest report and records source file hashes, source revision/dirty state, runtime and library hashes, actual OS/architecture and named outcomes. `artifacts/confinement-test-output.txt` is local diagnostic output and may contain development paths; do not publish it without redaction. A passing report always has `complete_release: false`. The runner does not upgrade a clean-install, signed-helper or platform gate. The two Java tests stay explicitly ignored in ordinary source CI when their staged runtime is unavailable.

## Observed development result

On 24 September 2026, the six supervisor tests passed on an **arm64 development Mac running macOS 26.6.2**, using **Java 21.0.12.1**, **Lucene 10.5.1** and the locally rebuilt current adapter. No runtime download was required. The evidence report is retained locally with the exact source/runtime hashes. Full Rust regression tests and strict Clippy also passed; see the PR validation for counts at the reviewed revision.

Still unverified: signed/notarized helper packages and entitlement inheritance; Intel Mac execution; minimum supported OS; clean installation; parser/Tika/PDFBox/POI, OCR, Python and Chromium compatibility; hard total disk/RSS limits; archive and native-code attack coverage; supervisor-crash cleanup; and the actual downloadable artifacts. Existing source/engine tests do not establish these claims.
