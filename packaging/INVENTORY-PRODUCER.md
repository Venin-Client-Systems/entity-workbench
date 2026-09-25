# Offline inventory production

The offline producer closes the manual inventory-generation gap in EW-01/EW-06.
It creates the existing runtime-inventory v1 format from a reviewed
[ownership plan](runtime-ownership.v1.schema.json) and a frozen local staging
tree. It uses no network, account, executable discovery, subprocess, runtime
launch or additional Python package. It does not download missing dependencies
or identify a binary from its filename.

```sh
python3 scripts/generate_runtime_inventory.py \
  --bundle runtime/staged/engines \
  --plan packaging/plans/development-macos-engines.v1.json \
  --inventory artifacts/runtime-inventory/development-engines.json \
  --target macos-aarch64 > artifacts/runtime-inventory/development-result.json
```

Create the output parent directory first, and use a new inventory destination.
The plan and inventory must be outside the bundle. The report must also be
redirected outside it. Absolute paths are CLI inputs only; the inventory contains
portable relative names, declared versions, sizes and SHA-256 hashes.

## Reviewed ownership and completeness

An ownership plan contains `schema_version`, `target`, `ocr_languages`, `regions`
and `components`. Each component has an exact `id`, pinned `version`, and nonempty
`paths` list. Each path names one file or a directory whose complete ordinary-file
subtree belongs to that component. These are literal selectors: no wildcard,
implicit root, traversal, missing path or empty directory. Repeated or overlapping
selectors within one component fail. Shared file ownership between distinct
components is explicit and permitted by the existing inventory contract.

The producer checks every present file has an owner. It reuses the verifier's
bounded tree scan and descriptor hash checks, then runs the complete existing
verifier over the generated candidate before publication. The final file list,
component ownership lists, languages and regions have deterministic ordering.
The same bytes and reviewed declarations produce the same inventory bytes,
regardless of declaration order. No timestamp or local path is embedded.

The tool never fills missing components with placeholders. It may successfully
produce an exact inventory of a partial tree: `generated: true`,
`complete: false`, and a verifier list of missing mandatory components. This
returns exit **1**, as does invalid input. Exit **0** requires successful
generation and inventory-contract completeness. CLI usage errors return **2**.
`complete_release` is always **false**; even a complete synthetic fixture or a
complete component inventory is not installed-product or release acceptance.

Only `missing_components` may remain in a published partial inventory's verifier
errors. Corruption, incompatible versions, unknown components, unsafe paths,
unowned files, unreadable assets and all other validation failures prevent
publication. The JSON report binds the exact plan, requirements and generated
inventory digests. This is a producer declaration; it cannot independently prove
that the declared version or component label truthfully identifies the bytes.
Review pins, notices and build provenance separately.

## Bounds, publication and trust boundary

The existing policy bounds apply: 16 MiB plan/inventory JSON, 100,000 files,
8 GiB per file, 32 GiB total and 64 path segments. The plan additionally has at
most 100,000 literal selectors and 200,000 expanded component/file references.
Files are streamed in 1 MiB blocks. Inventory JSON is emitted incrementally to a
bounded temporary file; no unbounded encoded output is allocated. Literal
directory expansion uses a sorted file index instead of rescanning the tree for
each selector.

Symlinks, Windows junction/reparse entries, hardlinked files, special files and
case/Unicode name collisions are rejected by the shared scanner. Resolved output
and plan locations cannot be hidden inside the bundle through a parent alias.
Ordinary file changes are checked while hashing; the independent verifier reads
the completed candidate against the tree again. The plan and requirements must
retain their digests across generation.

Publication links a closed, flushed temporary file to a previously absent output
name, then removes the temporary name. Existing inventories are never overwritten,
including one created concurrently. Filesystems without this publication primitive
fail explicitly; there is no overwrite fallback. A failure after publication may
leave an output or temporary name. The report distinguishes `published` from
`generated`; cleanup failure keeps `generated` and `complete` false. Retain and
inspect these failed artifacts before using a new destination.

Run on a frozen, access-controlled build tree and output directory. This portable
build tool is not a sandbox against a hostile process concurrently swapping
ancestor directories, an installed binary verifier, an SBOM scanner or proof of
architecture compatibility, executable permissions, absence of networking,
licence approval or signing. Signing may change bytes, so re-inventory and verify
the extracted final artifact after signing.

## Development plan and verification

The checked-in [development Mac plan](plans/development-macos-engines.v1.json)
describes only the existing engines staging layout: Java semantic version
`21.0.12.1+1`, document/image/PDF adapter `0.1.0`, Lucene `10.5.1`, Tesseract
`5.5.2` and the `tessdata_fast` English pack `4.1.0`. The Java declaration owns the
shared development-only build marker; this does not turn it into a runtime asset
or attest to the marker's prose. The document-parser component includes the
separate parser, image decoder and PDF renderer. OCR native dependencies and
notices are included; model metadata and its licence are explicitly shared with
the English model component. Transitive JAR/native-library identities remain
individual file hashes and existing staging notices.

This plan advertises no regional data and declares no application/UI, Python,
Chromium, Spatial or signed helper components. Missing components remain visible.
The plan is not suitable for a different runtime layout, version or target until
its declarations are reviewed and changed. It cannot close EW-06's requirement
for all-target, isolated offline installed execution.

The [25 September development observation](evidence/development-inventory-2026-09-25.json)
generated and independently verified an inventory of **425 files / 250,551,479
bytes** under that engines-only layout. Its inventory SHA-256 is
`2d1cc4cf9620e6e46b56d6186d817e0188eb78196fc9bfa42a092bfd99301629`.
The tool returned **exit 1**, `generated: true` and `complete: false`, with
**18 missing components** and no invalid present components. It launched no
runtime. The record binds the exact executed producer/verifier source hashes,
plan and requirements, and retains the limitation that those declarations are
reviewed producer metadata. This does not replace the older negative observation
or establish installation or execution acceptance.

Synthetic producer tests cover complete policies for all three target names,
partial inventories, deterministic ordering, ownership and shared files,
version mismatches, unsafe/empty selectors, byte/reference limits, aliases,
links, changed content and declarations, no-clobber races, bounded JSON and
post-publication cleanup failure. These are inventory tests, not cross-platform
runtime probes. Windows junction execution remains native-platform coverage;
portable reparse metadata checks do not replace it.

```sh
python3 -m unittest discover -s scripts/tests -p 'test_inventory_producer.py' -v
python3 -m unittest discover -s scripts/tests -p 'test_runtime_bundle.py' -v
```
