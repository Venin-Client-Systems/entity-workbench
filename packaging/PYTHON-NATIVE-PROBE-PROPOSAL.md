# Proposed first confined Python compatibility probe

Status: **proposal for root review; not implemented or executed by the offline
installer**. The assembled prefix has only file-integrity evidence. The existing
`workers/python/worker.py` envelope is experimental and is not a canonical
protocol or permission boundary. No application adapter activation follows from
this proposal.

## Fixed launch and file assignment

Verify the exact complete prefix inventory and all installed RECORDs before
launch. Use the existing macOS supervisor's process-group, descriptor-closing,
timeout, cancellation, stop/reap and owned-cleanup mechanisms with a dedicated
Python profile. Do not launch an ordinary unconfined compatibility process first.
The proposed command is exactly:

```text
/usr/bin/sandbox-exec -f <job>/worker.sb \
  <verified-prefix>/install/bin/python3.13 -I -S -B <job>/code/bootstrap.py
```

The host writes reviewed, hash-bound bootstrap and analytical probe files plus
the copied `transaction_totals.py` adapter under `<job>/code/`. No caller-supplied
Python, shell, flags, paths or recipe is accepted. Start with an empty environment
and preserve only the actual `HOME` value unchanged as the existing macOS
supervisor requires for `sandbox-exec`; do not repurpose it or grant reads of
its contents. Assign `TMPDIR=<job>/scratch` and fixed single-thread
numerical-library settings reviewed with the harness. No `PYTHONHOME`,
`PYTHONPATH`, `DYLD_*`, credentials or other user environment is inherited.
Set the working directory to `<job>/scratch`; stdin is closed. All inherited
descriptors above standard streams remain subject to the existing close-on-exec
guard.

Before importing third-party packages, the bootstrap verifies the expected
`isolated`, `no_site` and `dont_write_bytecode` flags and checks every initial
search-path location against the verified prefix's expected standard-library
locations. It adds only the exact verified
`<prefix>/install/lib/python3.13/site-packages` and reviewed `<job>/code` paths.
Do not invoke `site.main()`, `site.addsitedir()`, wheel entry-point wrappers or
any `.pth` processing. An unexpected path fails instead of being silently
discarded. The public receipt contains relative path classifications, not raw
local path strings.

## Proposed sandbox profile

Generate escaped literal/subpath arguments using the existing reviewed path
quoting helper. The intended profile is:

```scheme
(version 1)
(deny default)
(import "dyld-support.sb")
(deny process-fork)
(allow sysctl-read)
(allow file-read-metadata)
(allow process-exec (literal "<prefix>/install/bin/python3.13"))
(allow file-read* file-map-executable
  (subpath "<verified-prefix>")
  (subpath "/usr/lib")
  (subpath "/System/Library"))
(allow file-read*
  (literal "<job>")
  (subpath "<job>/code")
  (subpath "<job>/input")
  (subpath "<job>/scratch")
  (literal "/dev/null")
  (literal "/dev/random")
  (literal "/dev/urandom"))
(allow file-write* (subpath "<job>/scratch"))
```

There is no network grant and no prefix write grant. Global metadata reads match
the existing supervisor policy; this does not imply that metadata is hidden.
Only the assigned runtime, reviewed code, synthetic input and scratch have data
read grants beyond normal system loader facilities. The launcher must review
the expanded `dyld-support.sb` assumptions just as for existing workers; a source
profile alone is not confinement proof. Any missing import permission produces
a retained failure for review, not automatic grant expansion.

## Fixed budgets and receipts

Use the existing enforced native supervisor limits initially: **30 seconds wall
time, 30 CPU seconds, 256 descriptors, core dumps disabled, 64 MiB individual
file limit, 128 MiB monitored job-tree bytes and 512 job-tree entries**. The
601,821,300-byte read-only prefix is a separately inventoried assigned input, not
writable scratch. This is not a hard resident-memory cap; the current supervisor
does not establish one. Record peak memory if a reliable platform measurement is
available, without presenting it as enforced. Do not increase limits to obtain a
pass after a failure.

Further constrain fixed probe inputs to at most 64 KiB bootstrap, 64 KiB probe
code, 32 KiB adapter, 64 KiB JSON fixtures and 1 MiB synthetic Parquet. Accept at
most 1 MiB structured result and bounded 128 KiB diagnostic streams. Include
all assigned files in the job-tree monitor. The source/profile/fixture/binary/
runtime identities, fresh job nonce, outcome, exit/termination status and every
required assertion belong in an initially unsuccessful receipt. Timeout,
malformed output, missing phases, cancelled jobs or unconfirmed termination
cannot preserve a previous successful outcome. Only post-reap bounded result
reads are eligible for acceptance; cleanup failure stays a failure.

## Deliberate compatibility assertions

1. Compare all 58 selected distribution versions through metadata at the explicit
   site-packages path, then import the six top-level engine packages and required
   compiled dependencies. This does not mean every optional module was imported.
2. Build `spacy.blank("en")` and a `PhraseMatcher` with fixed synthetic terms.
   Assert exact UTF-8-text character offsets, case behavior, empty results and
   pending candidate status; no model or language asset download.
3. Build a NetworkX graph from a fixed accepted/rejected relationship fixture.
   Assert the accepted shortest path, assertion IDs and unreachable case. Use
   the standard backend explicitly; the advertised `nx_loopback` entry point
   points into a package test module and is not a required analytical backend.
4. Write/read bounded synthetic Parquet with PyArrow and invoke the reviewed
   `transaction_totals` function directly. Assert exact decimal/currency results,
   review-state exclusion and rejection of accepted transfer pointers. DuckDB
   external access remains disabled as in the reviewed adapter.
5. Import Splink and verify its pinned version only. Do not build an uncalibrated
   matching model or claim statistical linkage quality.
6. Enumerate relevant entry-point groups case-sensitively and compare their
   pinned metadata declarations. Deliberately resolve required spaCy/Thinc
   registry paths under `-S`: the English tokenizer used by the fixture, a
   reviewed `spacy-legacy.Tok2Vec.v1` architecture factory, and
   `spacy-legacy.StaticVectors.v1` layer factory without creating a model.
   Exercise `srsly.read_json.v1` against the assigned synthetic JSON through
   spaCy's reader registry. Optional telemetry/training loggers, GPU backends,
   pytest hooks and console wrappers are not required or invoked; their presence
   in metadata is not a functionality claim. Freeze these exact registry API
   calls after reading the pinned sources before implementation.

Separate campaigns must subsequently prove hostile file/network denial with
matched positive controls, immutable prefix enforcement, a freshly relocated
copy, and canonical job/protocol integration. The first compatibility receipt
must not claim those scenarios passed merely because imports completed inside
the proposed profile. Windows, Intel Mac, clean installation, signing and
minimum-OS support remain outside this first probe.
