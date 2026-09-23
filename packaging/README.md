# Offline runtime inventory contract

[EW-01 / issue #5](https://github.com/Venin-Client-Systems/entity-workbench/issues/5) establishes a packaging input contract. It does not enable an incomplete runtime or pass a complete-release gate.

`runtime-requirements.v1.json` lists mandatory components for Windows x64, Mac Apple Silicon and Mac Intel. Every target includes the app/UI, sandbox helper, Java/parser/Lucene, Python and analysis libraries, DuckDB/Spatial, OCR, Playwright/driver/Chromium and local presentation/help/demo assets. Windows also requires Fixed Version WebView2. English OCR is mandatory; every additional advertised OCR language or region requires its own component.

The producer writes an inventory matching [the published v1 schema](../schemas/runtime-inventory.v1.json). Keep the inventory **outside** the bundle root, avoiding a self-referential checksum. Each component declares a pinned version and a non-empty list of owned relative files. The global file table records exact byte sizes and SHA-256 hashes. Include dependencies, licences and runtime support files in the appropriate component. Every file must be listed and owned; multiple components may share an explicitly declared file. All directory entries must be portable and free of links, including Windows reparse points/junctions and hardlinks. Materialize reviewed upstream links into ordinary files during staging.

```sh
python3 scripts/verify_runtime_bundle.py \
  --bundle artifacts/candidate-bundle \
  --inventory artifacts/candidate-inventory.json \
  --target macos-aarch64 > artifacts/candidate-inventory-result.json
```

Exit `0` means the inventory contract passed. Exit `1` means incomplete/invalid content; stdout remains a JSON result with missing/invalid component IDs, error codes, verified file/byte counts and inventory/requirements checksums. Invalid CLI usage exits `2`. The verifier uses only Python's standard library, reads local files and launches no executables or network requests. Python is a developer/build prerequisite for this check, not an end-user prerequisite.

The structural schema is only part of validation. The verifier additionally checks the target component requirements, duplicates, case/Unicode path collisions, unsafe paths/links, undeclared files, exact lengths/hashes and the Java 21 series, matching DuckDB/Spatial and Playwright/driver versions. Limits are 16 MiB JSON, 100,000 declared files, 8 GiB per file, 32 GiB total declared content, 64 path segments and 200 reported errors. Extra errors keep the result failed and set `errors_truncated`.

`ocr_languages` uses bundled Tesseract language IDs (initially `eng`). `regions` lists versioned pack IDs actually advertised by this distribution. An empty region list explicitly advertises no regional address/basemap coverage; it does not meet the regional-distribution acceptance gate. Use component IDs `ocr-language:<id>` and `region:<id>` and exact asset-pack versions. Do not claim synthetic packs provide production coverage.

## Interpretation and trust boundary

The inventory is a reviewed producer declaration, not an SBOM scanner or an independent way to identify a binary. A malicious producer could mislabel bytes and versions. Signing and the release-evidence ledger must bind the reviewed inventory, requirements and exact candidate artifact; installed functional tests must prove the binaries work together, including Chromium/Playwright compatibility. This check alone proves neither confinement, absence of networking, licence approval, architecture compatibility, executable permissions, native signing nor a usable offline installation.

Run against a frozen, access-controlled staging tree. Size and identity checks detect ordinary concurrent file changes; this portable build tool is not a sandbox against a hostile process concurrently swapping ancestor directories. Do not run it on an actively modified or attacker-controlled filesystem. Keep the report outside the bundle too. Verify final artifact extraction again after signing because signing can change file bytes.

## Current development evidence

The 23 September 2026 check inventories only the existing `runtime/staged/engines` development directory: Java `21.0.12.1+1` and Lucene `10.5.1` with adapter `0.1.0`. It hashes **269 files / 168,205,077 bytes** and correctly returns incomplete: **21 missing components** plus the unlisted `development-only.json` marker. Application/UI assets are absent from this engines-only directory; their absence here is not a statement that the development app has no interface.

The sanitized [negative result](../docs/delivery/evidence/runtime-inventory-2026-09-23.json) is retained; the local inventory is in ignored `artifacts/runtime-inventory/current-development.json`. It is not a signed product inventory. Complete per-target inventory generation belongs to the packaging work in EW-06/EW-07/EW-38. All release gates remain unpassed.

Run the synthetic suite on every native source CI target:

```sh
python3 -m unittest discover -s scripts/tests -p 'test_*.py' -v
```

The suite checks all target policies, advertised assets, byte corruption, paths, symlinks/hardlinks, Windows junctions, missing/unknown/duplicate declarations, malformed/oversized JSON, bounded diagnostics and failure under Python optimization. The Windows-specific junction test is skipped on Mac; symlink creation may be unavailable on a host without the corresponding OS privilege and is reported as a skip.
