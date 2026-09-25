# Reviewed offline CPython staging

This EW-07/#11 development increment stages one exact upstream runtime. It does
not install analysis wheels, execute the interpreter, activate a worker, prove
relocation or confinement, select a supported macOS minimum, or pass a release
gate. The staged manifest and report always retain those unmet checks.

## Input and build prerequisites

Use **build-time Python 3.14 with its standard-library zstd support** on a POSIX
build host with descriptor-relative no-follow file operations. The script remains
importable on Python 3.13; synthetic uncompressed tests run there without zstd.
This prerequisite applies to the developer/build machine, not installed users.
No third-party decompression package, subprocess, network request, package
installer or staged program is invoked by the stager.

The only accepted input is
`cpython-3.13.15+20260924-aarch64-apple-darwin-pgo+lto-full.tar.zst`,
**59,200,647 bytes**, SHA-256
`aa76e997b7bb159002dd38938e57ce06fc07bf73621131d3d126d459df4e9ac7`.
The fixed size and digest are checked before decoding; CLI options cannot replace
them. The source is [upstream release 20260924](https://github.com/astral-sh/python-build-standalone/releases/tag/20260924),
whose metadata identifies commit
`6a729962cddc76630b59b1b895501b1539412524`. The inspected upstream `SHA256SUMS`
agrees with the downloaded archive and the release asset digest. This is retained
upstream checksum evidence, not an independent security approval or signature claim.

The pinned [archive documentation](https://github.com/astral-sh/python-build-standalone/blob/6a729962cddc76630b59b1b895501b1539412524/docs/distributions.rst)
describes the full archive's `install/`, `PYTHON.json`, build information and
licence metadata. The install-only variant omits provenance/notice material and
is deliberately not accepted by this tool. This is the standard-GIL `pgo+lto`
build; no free-threaded or source-build variant is selected.

```sh
mkdir -p runtime artifacts/python-staging
python3.14 scripts/stage_python_runtime.py \
  --archive /path/to/the/reviewed/full-archive.tar.zst \
  --destination runtime/python-cpython-3.13.15-macos-aarch64 \
  > artifacts/python-staging/stage.json

python3 scripts/generate_runtime_inventory.py \
  --bundle runtime/python-cpython-3.13.15-macos-aarch64 \
  --plan packaging/plans/development-macos-python.v1.json \
  --inventory artifacts/python-staging/inventory.json \
  --target macos-aarch64 > artifacts/python-staging/inventory-result.json
```

Create the destination's parent first and provide a fresh destination. Existing
files, directories and symlinks are never replaced. Reports/inventories belong
outside the staged tree. Stager exit **0** means file staging succeeded;
`complete_release` remains **false**. Its failure exit is **1**. The separate
inventory producer should return **1** for this partial tree, with
`generated: true`, `complete: false`, and missing mandatory application components.
The plan's English OCR declaration expresses an existing product requirement;
there is no OCR asset or claim in this Python-only tree.

## Retained data and deliberate transformations

Every original regular file under `python/install/` is retained beneath `install/`.
The actual **19** upstream licence files are retained beneath `licenses/`, and
`PYTHON.json` is retained byte-for-byte. Build objects outside the installation
are validated as archive members but not published. Fixed, timestamp-free
`provenance.json` records the source identity, observed member/byte counts,
materialized links and unmet checks. `manifest.json` binds every other output
file's byte count, SHA-256 and executable status. The inventory producer also
hashes the manifest itself and the complete tree.

The only accepted symlinks are the **ten literal source/target pairs** in
`stage_python_runtime.py::LINKS`. These include the nine command/configuration/man
aliases in install-only and the full archive's additional
`lib/libpython3.13.a -> python3.13/config-3.13-darwin/libpython3.13.a`.
Each target must be an ordinary retained file. Every alias becomes a separate,
single-link regular file with identical bytes and its own inventory hash. The
70,101,872-byte static library therefore occupies two copies, both counted against
the output budget. No pruning is part of this increment. Executable files use
0755 and other files 0644, irrespective of archive ownership or the build machine's
umask. Directory creation requests 0700, subject to the existing umask; an
unusually owner-restrictive umask can prevent staging and is not loosened. The
tool never changes the process-wide umask. No upstream file contents are rewritten.

`PYTHON.json` refers to `licenses/LICENSE.zlib-ng.txt`, which this full archive
does not contain; its zlib entry identifies system `z` linkage. The tool preserves
the original metadata and actual notices and explicitly records this discrepancy.
It does not invent the missing file or claim the notices are complete.

Upstream documents [embedded build-time paths and installation fixups](https://github.com/astral-sh/python-build-standalone/blob/6a729962cddc76630b59b1b895501b1539412524/docs/quirks.rst#references-to-build-time-paths).
Preserving those upstream bytes is intentional here. Loading all native extensions,
analysis wheels, moving the tree and running it in the intended worker boundary
remain future tests. Upstream deployment metadata is not application OS support.

## Bounds, failure and trust boundary

Limits are 5,000 visible archive members, 16 path segments, 80 MiB per member,
280 MiB declared payload, **300 MiB actual decompressed bytes including trailing
data**, and 320 MiB retained output including materialized copies and metadata.
Copies and hashes use 64 KiB blocks. These limits fit only this reviewed source;
they do not claim arbitrary CPython archive support. The original input descriptor
stays open; its identity/timestamps and complete digest are rechecked after decode
and before manifest publication.

Traversal, absolute/unsafe/reserved names, case/Unicode collisions, duplicate
members, file-as-parent conflicts, unlisted/changed links, archive hardlinks,
special files, sparse entries, privileged modes and unsupported PAX metadata fail.
All source/destination ancestors and output opens use no-follow directory/file
descriptors. Source files must be regular and single-link. The private fresh
output contains only newly created ordinary files; final verification rejects
unexpected files/links and changed sizes, hashes or executable status.

The build workspace must be exclusively controlled during staging. This is a
build-input validator, not a sandbox against another process running as the same
build user. There is no overwrite or unsanitized fallback. On an ordinary failure,
the owned partial tree is removed through its pinned parent descriptor. If cleanup
cannot be confirmed, the result remains failed, retains the preceding failure and
reports the destination for recovery without emitting local paths. A failed
command or interrupted build must never be treated as a ready runtime merely
because a directory exists. A completed manifest is published only after checks;
an OS/process interruption may still leave partial files requiring review.

Run the portable synthetic regressions with:

```sh
python3 -m unittest discover -s scripts/tests -p test_python_runtime_staging.py -v
python3 -O -m unittest discover -s scripts/tests -p test_python_runtime_staging.py -v
```

Fixtures replace the production checksum only within unit-test mocks. The public
CLI has no arbitrary archive hash, link allowlist or runtime-version override.
