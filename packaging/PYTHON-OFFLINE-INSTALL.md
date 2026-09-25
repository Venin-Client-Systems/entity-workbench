# Finite offline Python prefix assembly

This EW-07/#11 implementation assembles the reviewed CPython 3.13.15 macOS arm64
tree and 58 exact wheels into a fresh development prefix. It uses no pip, uv,
package scripts, subprocesses, downloaded interpreter or network access. No
worker or canonical protocol is enabled. The output is explicitly
`installation_state: "assembled-unexecuted"`, `runtime_components_satisfied: []`
and `complete_release: false`.

The actual source-bound assembly produced **11,320 ordinary files totaling
601,821,300 bytes**. Its manifest SHA-256 is
`4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822`.
A separate read-only verifier process confirmed every file and all **58 installed
RECORD files**. The [sanitized receipt](evidence/python-offline-install-2026-09-25.json)
binds the executed source, inputs, reports and retained provenance. This is
source-file-bound development evidence, not clean-commit release acceptance.

## Fixed contract and unchanged inputs

The [installation plan](plans/python-install-macos-arm64.v1.json), SHA-256
`849ad7b775879f7e8f213af11f1421ad75d30019ed6aab3c35e047a8a50b9b59`,
pins the previously reviewed runtime manifest
`6b4edce6755094b4f8393d5553da9a6968ac18b761035616430aaed7dbb0be95`
and [58-wheel selection](plans/python-macos-arm64-wheels.v1.json)
`983ac846a79e4985218b3f2582445abeac18de90fc3108c06fcb29b60928580e`.
The existing lock/project hashes are checked through the shared wheel selector.
No new package selection, marker resolution, version, URL or platform inference
occurs here. Input directories and their ancestors must be real directories;
input files must be ordinary single-link files.

All 3,332 files declared by the staged CPython manifest are copied to identical
relative paths with unchanged bytes. The original manifest itself is retained
at `review/cpython/manifest.json`; the new complete-file manifest occupies
`manifest.json`. CPython's original `provenance.json`, `PYTHON.json`, 19 actual
licence files, standard library, bundled pip metadata and ten materialized
aliases remain unchanged. Existing upstream scripts are retained; this step
generates no additional console wrappers.

All ordinary wheel members are installed below
`install/lib/python3.13/site-packages/`, except the reviewed igraph header:
`igraph-1.0.0.data/headers/igraphmodule_api.h` becomes
`install/include/python3.13/igraph/igraphmodule_api.h`. Any other spread path
fails. Primary and nested vendored metadata, package data, notices and the
151-byte setuptools `.pth` file are preserved. Only the 58 primary installed
`RECORD` files have newly generated content.

The reused wheel validator retains the original `METADATA`, `WHEEL`, `RECORD`
and recognized notices separately under `review/<package>/`. This provides
174 original primary metadata files and 105 notice copies. Original wheel
archives remain immutable external build inputs, identified by exact hashes;
they are not duplicated inside the assembled prefix. All their ordinary member
bytes remain available in the installed tree or retained review material.

`installation-provenance.json` records each wheel's source-member-to-destination
mapping, original archive identity, original and installed RECORD paths, CRC
validation and explicit omissions. All originals are rehashed after copying,
including earlier wheel inputs, and both plans are reread before publication.
The installer rejects output nested inside either input by comparing pinned
directory ancestry identities, including case aliases on insensitive hosts.

## Installed RECORDs and independent verification

Before accepting a wheel, the installer verifies every source RECORD row against
the actual member SHA-256 and byte count. Rows must cover exactly the ordinary
archive members, with no duplicates or omissions; only RECORD's own row has a
blank hash and size. The current reviewed set has 7,705 source rows and only
SHA-256 hashes. Unknown source-record forms are rejected instead of silently
normalized.

Installed RECORD rows describe paths relative to the site-packages directory,
URL-safe unpadded SHA-256 values and exact sizes. The relocated igraph header is
recorded as `../../../include/python3.13/igraph/igraphmodule_api.h`. Each RECORD
lists itself with blank hash and size. Original RECORD bytes remain unchanged
under `review/`; no new signatures, bytecode, wrappers, INSTALLER claim or
bootstrap are fabricated. Nested vendored RECORDs and CPython's bundled pip
metadata are retained unchanged rather than presented as newly installed
distributions.

The [standalone reader](../scripts/verify_python_install.py) takes an **expected
manifest SHA-256** supplied by the assembler or review record. It does not trust
manifest existence alone. It independently checks the manifest's closed schema
and truthful state, exact file set, bounds, no links, normalized permissions,
every file hash and all 58 RECORD ownership sets and rows. The new manifest
describes every ordinary output file except itself; its external SHA-256 and
verified size cover that unavoidable self-reference exception. This inventory
does not declare a runnable `spacy`, `duckdb` or other release component.

## Bounded writes and recovery

| Bound | Fixed value |
| --- | ---: |
| Ordinary output files | Fewer than 20,000 |
| Traversed output entries | 40,000 |
| Relative path depth | 16 segments |
| Individual installed file | 80 MiB |
| Combined prefix | 768 MiB |
| Metadata document | 16 MiB |
| Combined wheel expansion | 512 MiB |
| Streaming copy/hash block | 64 KiB |

Existing wheel limits remain enforced before member reads, including compression,
CRC, size, member count, source metadata, safe paths and duplicate checks. The
smaller remaining installation budget also limits retained review material.
Cross-source case/Unicode collisions and file-as-parent conflicts fail. Sources
and destination ancestors are traversed through no-follow directory descriptors;
the output must be fresh and no existing file is replaced. Ordinary file modes
are normalized to `0644` or `0755` from the source executable flag. Newly created
directories request `0700`, subject to the build process's existing umask; no
process-global umask change is made.

Use an exclusively controlled development workspace. This build tool is not a
sandbox against another process running as the same user. On failure it removes
only the newly owned output whose directory identity is still known. If that
identity or cleanup cannot be confirmed, the receipt remains unsuccessful and
retains the partial output for recovery, preserving the preceding failure.
Sanitized failures do not expose raw local paths. Interrupted/failed output must
not be treated as successful from directory or manifest existence alone.

## Run and test

Use existing POSIX build Python 3.13 or later. The candidate interpreter is never
invoked, including for scheme or version discovery.

```sh
mkdir -p runtime artifacts/python-install
python3 scripts/install_python_offline.py \
  --runtime /path/to/verified-cpython-tree \
  --wheelhouse /path/to/reviewed-wheel-inputs \
  --destination runtime/python-installed \
  > artifacts/python-install/assembly-result.json
python3 scripts/verify_python_install.py \
  --prefix runtime/python-installed \
  --expected-manifest-sha256 <digest-from-successful-assembly>
python3 -m unittest discover -s scripts/tests -p test_python_offline_install.py -v
python3 -O -m unittest discover -s scripts/tests -p test_python_offline_install.py -v
```

The 21 synthetic tests passed on development Python 3.13.11 and 3.14.2, normally
and with `-O`. POSIX descriptor tests skip on Windows; actual Windows assembly or
target compatibility is not claimed. The suite covers RECORD correctness and
relocation, immutable inputs, corruption, unsafe paths/links, collisions,
no-clobber output, independent-reader failure, bounds and cleanup precedence.

Actual imports, loader behavior, plugin functionality, relocated execution,
signing, confinement, OS support and canonical integration remain unverified.
No minimum macOS version is asserted. The missing zlib-ng licence reference in
upstream CPython metadata is [explained by its annotation mechanism](PYTHON-ZLIB-NOTICE-REVIEW.md);
final notice review remains open and the original discrepancy flags are
preserved. Retaining all available notices does not establish legal completeness. The separate
[native-probe proposal](PYTHON-NATIVE-PROBE-PROPOSAL.md) is for review before any
candidate execution, not authorization to enable the application worker.
