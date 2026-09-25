# Python hostile controls and relocation — source contract

EW-07 / issue #11. These are fixed, ignored development tests for the exact
macOS-arm64 prefix already covered by the
[compatibility observations](PYTHON-COMPATIBILITY-PROBE.md). Source validation and
trusted Rust hashing measurements do not execute the candidate interpreter.
Hostile and relocated candidate campaigns require a separate reviewed explicit
invocation. There is no production command or canonical Python protocol.

## Fixed hostile recipe

`python-hostile-v1` uses only the standard library. It starts the exact verified
`install/bin/python3.13` with `-I -S -B`, checks the same three standard-library
paths and interpreter prefix identity, and does not process `.pth` files or
load optional plugins. The launcher calls the **same profile generator and
process supervisor** used by the compatibility test. There are no new grants.
The environment, 30-second worker wall/CPU limits, descriptor ceiling, file/job
limits and post-reap diagnostic bounds are unchanged.

The parent writes three assigned files: fixed `code/bootstrap.py`, a synthetic
`input/sentinel.txt`, and `input/assignment.json`. Their relative names, sizes and
hashes are recorded before spawn. The worker first reads the assigned sentinel
and completes a scratch write/read round trip. These are actual positive file
controls under the same profile. Separate host-owned synthetic sibling and
original files lie outside every assigned data-read/write grant. The parent
checks their contents and its own non-truncating write-open capability before
and after the worker. It also proves its own write-open capability for the
verified interpreter file; it never truncates or writes that file.

The worker attempts sibling content read, original write-open and prefix
interpreter write-open. **Every write-open omits truncation and performs no
write.** A successful open is itself a failed boundary assertion. An expected
denial requires Darwin `EPERM` or `EACCES`; missing files, unrelated I/O errors,
partial reads or an arbitrary nonzero exit do not pass. Metadata remains globally
readable under the existing profile and is not claimed hidden. Originals in this
test are wholly synthetic; no case or user file is assigned.

Results use closed Rust types with a 16 KiB read bound and exact
schema/recipe/job/runtime/Python/flags/sentinel identities. Typed observations
are retained even when a boundary assertion fails; arbitrary error strings,
paths or extra fields cannot become diagnostic content. Parent receipt flags
begin unsuccessful before any preparation. Termination must be confirmed before
reading worker output or checking sentinels. Failure and cleanup precedence remain
explicit; unverified termination retains both assignment and sentinel trees.

## Parent-observed IPv4 loopback networking

The host owns one TCP and one UDP listener bound only to IPv4 loopback on assigned
ephemeral ports. There is no Internet request or DNS lookup. Before/after positive
controls are sent by the trusted host, while the confined worker receives a
separate random 32-byte marker. Each protocol requires exactly one acknowledged
before control, exactly one after control, zero confined deliveries, zero
unexpected traffic and zero observation errors.

The worker records attempted/socket-created/connected/send-accepted/echo-received
separately. TCP must fail connection with an explicit permission error. UDP may
report an accepted send followed by a receive timeout; this is eligible only
when the independent listener records no confined delivery and both host controls
succeed. A successful `sendto` call alone is neither delivery proof nor denial
proof. These are **IPv4-loopback observations only**. They do not establish
Internet, DNS, IPv6 or arbitrary destination behavior.

Client operations have 500 ms timeouts; accepted TCP connections have 250 ms
read/write timeouts. Each listener is nonblocking with at most eight handled
events, a 60-second absolute host observation bound and a fixed 250 ms drain after
stop. Both threads are joined independently on every owned completion path.
Listener errors keep the campaign failed; possible-live-worker termination errors
retain precedence. Unknown termination never triggers a blind retry or a
post-runtime inventory read. The receive window is finite and explicitly bounded.

## Exact relocation without installation or execution

`scripts/relocate_python_prefix.py` copies the reviewed prefix into a fresh
no-clobber directory. It reuses the no-follow directory/file helpers and bounded
64 KiB streams from offline staging. It preserves all ordinary bytes and
0644/0755 asset modes, including manifests, notices, original RECORD provenance,
installed RECORDs and `.pth` bytes. It creates no hard links, symlinks, scripts,
console wrappers or new manifests. No pip/uv/install hook or candidate interpreter
is executed by copying.

The existing independent verifier checks the exact source manifest and complete
file inventory before copy, then both source and destination after copy. The
copier also compares per-file sizes/hashes and open-file fingerprints, rejects
nested or linked destinations, and checks source/destination directory identities.
Existing bounds remain 20,000 files, 40,000 entries, depth 16, 80 MiB per file and
768 MiB total. Only the owned partial destination can be removed after failure;
failed cleanup is explicitly reported and leaves recovery state. Successful
copies remain identifiable local development artifacts.

The `relocated` campaign subsequently invokes the **unchanged full compatibility
recipe** against that moved prefix, under the same sandbox with its new assigned
paths. It requires exact bootstrap and module-prefix checks plus all existing
analytical assertions. The original prefix is outside that job's assigned runtime
grant. Both prefixes are independently checked after confirmed execution, even
if the worker failed. A copy alone is not a relocation-compatibility pass.

## Build identity, measurement and invocation

The separate `scripts/test_python_isolation.py` runner uses `cargo test --release`
with the existing Cargo release profile: optimization level three, disabled debug
assertions, thin LTO and one codegen unit. It validates the compiler artifact's
profile, records the source commit/tree/parents/file hashes and compiled test
binary SHA-256, and rechecks source/binary identity after acceptance. The build
limit remains **300 seconds** and each native test has the same **120-second
outer timeout**; neither extends the worker's 30-second limit. The compatibility
recipe/fixtures and the first two historical receipts remain unchanged.

The `measure` case selects only the ignored trusted Rust prefix-hashing test. It
asserts release compilation, writes an initially unsuccessful measurement receipt,
verifies all 11,320 files using the Rust reader and records monotonic elapsed
milliseconds. It has no subprocess launch and sets `candidate_executed: false`.
This measurement distinguishes host verification cost from worker execution;
it does not relax limits or establish a package performance claim.

From a clean reviewed checkout, with an existing owned artifact parent and a
fresh child directory, the explicit forms are:

```text
python3 scripts/test_python_isolation.py --case measure --prefix <reviewed-prefix> --artifacts <fresh-private-artifacts>
python3 scripts/test_python_isolation.py --case hostile --prefix <reviewed-prefix> --artifacts <fresh-private-artifacts> --execute-reviewed-probe
python3 scripts/test_python_isolation.py --case relocated --prefix <reviewed-prefix> --artifacts <fresh-private-artifacts> --execute-reviewed-probe
```

The latter two flags are execution guards, not authority by themselves. The runner
records a fresh campaign nonce and unsuccessful observation before work, validates
native recipe/job/campaign identity, and never reuses an earlier result. It may
retain a bounded parent-owned initial receipt after timeout, but does not read
worker scratch, reverify a possibly live worker's prefix, clean its assignment or
retry. The signed source handoff authorizes no candidate execution on its own.

## Source checks and limits of the claim

Synthetic contracts cover permission-error classification; immutable write-open;
missing host controls or nonzero delivery; send-versus-delivery distinction;
malformed/duplicate/oversized results; cleanup/termination precedence; stale nonce;
wrong build profile; failed relocation/build; confirmed-failure post-verification;
timeout recovery; no-follow copying, source mutation, collisions and partial-output
cleanup. Worker networking is mocked in Python source tests; Rust listener count
tests are pure observations. Ordinary tests do not launch the hostile candidate.

Canonical protocol, hard resident-memory limits, supervisor-crash recovery,
minimum-OS support, signed clean offline installations, Intel Mac/Windows runtime
execution and complete notices remain separate unmet checks. No complete-release
flag or shared release gate is changed by this source increment.

## Trusted release hashing observation — 2026-09-26

At signed source `b502d987c5caa30e5c6cb16f75040cee69639437`, exactly the trusted
Rust `native_python_prefix_hash_measurement` test ran. It passed full verification
of the pinned 11,320-file prefix in **7,432 ms**; the complete test took 7.47 seconds.
The release build completed in 4m 56s inside the unchanged 300-second build limit.
No candidate interpreter, listener, hostile or relocation case executed. This is
an observed host-verification cost, not a general performance claim or an
explanation for the earlier compatibility failure.

The source-bound [outer measurement receipt](../../packaging/evidence/python-prefix-hash-release-2026-09-26.json)
and [native Rust receipt](../../packaging/evidence/python-prefix-hash-release-native-2026-09-26.json)
are retained verbatim. Campaign identity was
`83358d82-6ac2-4af1-a6df-893e37129e6d`; measurement identity was
`d8f02a43-cd77-4e27-9004-46e4c4445139`. The release test binary SHA-256 was
`e2b40682fe1891163bca734080483dfc52fd46261ec6393db2e032029fecb28f`.
Compiler artifact metadata required optimization level three and disabled debug
assertions; the measured Rust test separately asserted release compilation.
Source/binary identities were rechecked after acceptance. The outer native-test
limit remains 120 seconds and all worker limits remain unchanged. Hostile and
relocation candidate tests still require final source review and separate native
observations; neither has passed merely because this measurement did.
