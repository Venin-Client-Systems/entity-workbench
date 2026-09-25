# Offline reviewed Python wheel archives

This EW-07/#11 increment verifies and retains **58 specific production wheel
archives**. It does not install them, import package code, launch the staged
interpreter, satisfy any runnable-engine component or change a release gate.
Its manifests expressly contain `installation_state: "archives-only"`,
`runtime_components_satisfied: []` and `complete_release: false`.

## Fixed selection and provenance

The [reviewed selection](plans/python-macos-arm64-wheels.v1.json) contains one exact
filename, URL, size and SHA-256 per package, totalling **94,887,529 compressed
bytes**. Its digest is
`983ac846a79e4985218b3f2582445abeac18de90fc3108c06fcb29b60928580e`.
The verifier also pins the existing `workers/python/uv.lock` digest
`5fe63f3b0597df585840b9e6757c87a76754b52aa1570cb401cb48113ac81eb4` and
`pyproject.toml` digest
`a6b11f5ca5d427d963be6f63dd62c4c40f602c14e3252447a04be109de0b5375`.
Any change requires a newly reviewed selection and corresponding source update;
there is no runtime resolver, floating version or alternate-source fallback.

Selection used existing **uv 0.11.32**, the frozen production lock, CPython 3.13.15
and `aarch64-apple-darwin`, with macOS **12.0 solely as wheel-selection context**.
Binary-only resolution found compatible candidates; an independent offline
`uv tree --locked --no-dev --python-version 3.13.15 --python-platform
aarch64-apple-darwin --format json` traversal confirmed 58 reachable production
packages. This reused uv's marker handling; no marker solver was implemented.
The raw preview-format tree contains local paths and is retained privately/ignored,
while the public evidence includes only sanitized package identities and digests.

Three packages had multiple compatible candidates. The reviewed plan explicitly
chooses `charset_normalizer-3.5.1-cp313-cp313-macosx_10_13_universal2.whl`,
`duckdb-1.5.0-cp313-cp313-macosx_11_0_arm64.whl`, and
`wrapt-2.4.1-cp313-cp313-macosx_11_0_arm64.whl`. The other packages had one
candidate in that target resolution. Every selected row was matched against
the existing lock's URL, size and hash. All URLs use official
`https://files.pythonhosted.org/` assets. These are development inputs selected
for one target, not an application minimum-OS or native compatibility declaration.

## Run without installing anything

Use an existing POSIX build host with Python 3.13 or later. The script uses only
standard-library parsing and the repository's descriptor/copy/hash helpers. It
invokes no network client, subprocess, uv, pip, package installer or package import.
Supply a previously obtained local directory containing exactly the 58 approved
files; downloading is a separate explicitly authorized build operation.

```sh
mkdir -p runtime artifacts/python-wheelhouse
python3 scripts/stage_python_wheelhouse.py \
  --input /path/to/verified-local-wheel-inputs \
  --destination runtime/python-wheelhouse \
  > artifacts/python-wheelhouse/staging-result.json
```

The destination parent must already exist. The destination must be fresh. Exit
0 means archive staging succeeded, not that Python dependencies work; failures
return 1. Existing directories/files/links are not replaced. Any missing,
additional, alternate, source-distribution or partial-download input fails. No
downloaded executable is invoked to identify its version.

## Validation and retained output

The [wheel format](https://packaging.python.org/en/latest/specifications/binary-distribution-format/)
defines ZIP archives, filename compatibility tags and primary `.dist-info`
metadata. This verifier checks the exact selected archive hash before ZIP
inspection, then validates the expected primary `METADATA`, `WHEEL` and `RECORD`
locations, distribution name/version, declared wheel tags and reviewed metadata
versions. Legitimate nested vendored metadata is preserved and is not mistaken
for a second primary distribution. The tag check is a closed check of the
already reviewed selection, not a general platform compatibility implementation.

All member paths, file types, sizes and compression methods are checked before
reading member content. Every ordinary member is streamed to EOF, verifying its
ZIP CRC and actual byte count, even when it will remain only inside the wheel.
Stored and deflated members are supported; encrypted entries, links, special
files, traversal, duplicates, case/Unicode collisions, file-as-parent conflicts,
NUL-truncated names, unexpected root distributions and unsupported formats fail.
Limits are 64 MiB per wheel, 256 MiB per member, 512 MiB expansion per wheel,
2 GiB combined expansion, 20,000 members per wheel, and 32 archive path segments.
Metadata and individual notice reads are bounded to 4 MiB. Streaming uses 64 KiB
blocks; selected output paths also satisfy the shared helper's 16-segment limit.

`wheels/` holds each original archive byte-for-byte with non-executable file
permissions. `review/<package>/` holds the original primary metadata and selected
licence/notice bytes, retaining the source paths. Declared `License-File` entries
must resolve uniquely to retained notices. The standard
[core metadata fields](https://packaging.python.org/en/latest/specifications/core-metadata/)
remain readable without importing the package. Complete wheel archives preserve
other material even when a filename is not recognized as a notice. No legal
sufficiency claim is made; `RECORD` is retained, not used as an installation plan
or as proof that installation is safe.

The source descriptor remains open for ZIP inspection, rehashing and copying;
identity/timestamps and copied digest are checked. No-follow descriptor operations
reject linked input ancestors/files. A final independent tree/hash check validates
every retained file. Deterministic provenance and manifest files record exact
selection/lock identities, archive observations and source anchors. Failed owned
partial trees are removed; if identity or cleanup cannot be confirmed, a failed
receipt preserves the preceding failure and retains the output for recovery.
An interrupted or failed build must not be treated as successful from directory
or manifest existence alone. Use an exclusively controlled build workspace;
this tooling is not a sandbox against other processes running as the build user.

## Outstanding work

This archive manifest is deliberately **not** a runnable runtime-inventory
component declaration. Do not map these ZIP files to `spacy`, `duckdb`, `pyarrow`
or other mandatory engine IDs to make the application inventory appear complete.
Installation layout, script entry points, `.pth` behavior, native loader closure,
relocation, offline imports, worker confinement and canonical integration need
separate implementation and evidence. The CPython runtime's referenced-but-absent
zlib-ng licence file remains an independent unresolved notice discrepancy.

```sh
python3 -m unittest discover -s scripts/tests -p test_python_wheelhouse_staging.py -v
python3 -O -m unittest discover -s scripts/tests -p test_python_wheelhouse_staging.py -v
```

The unit suite uses synthetic wheels with deliberately non-executable fixture
code, malformed archives and mocked fixed pins. It includes portable refusal/tag
checks; POSIX descriptor tests skip on Windows. No actual Windows execution or
installed-product compatibility is established by those tests.
