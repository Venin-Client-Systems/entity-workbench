# DOCX foundation dependencies

The production writer uses the existing locked quick-xml version directly and a
narrow pinned ZIP dependency. No ZIP compression, crypto, time or default feature
is enabled. Existing resolved dependency versions were not updated.

| Package | Version | Locked registry checksum | Licence |
|---|---|---|---|
| quick-xml | 0.42.0 | `41b1177fdf999d2321d3fb46ff47159d9c1fb9ad66a4879f8c50a0b504615e9b` | MIT; `quick-xml-0.42.0-LICENSE-MIT.md` |
| zip | 8.6.0 | `2d04a6b5381502aa6087c94c669499eb1602eb9c5e8198e534de571f7154809b` | MIT; `zip-8.6.0-LICENSE` |
| typed-path | 0.12.3 | `8e28f89b80c87b8fb0cf04ab448d5dd0dd0ade2f8891bae878de66a75a28600e` | MIT OR Apache-2.0; distributed here under MIT, `typed-path-0.12.3-LICENSE-MIT` |

Licence texts are unmodified copies from the exact Cargo registry source packages.
The existing crc32fast, indexmap and memchr dependencies are reused; complete
release SBOM/licence assembly remains governed by the release process.

Upstream: [zip 8.6.0 manifest](https://github.com/zip-rs/zip2/blob/v8.6.0/Cargo.toml),
[zip licence](https://github.com/zip-rs/zip2/blob/v8.6.0/LICENSE),
[typed-path 0.12.3 package](https://crates.io/crates/typed-path/0.12.3),
[quick-xml 0.42.0 package](https://crates.io/crates/quick-xml/0.42.0).
The ZIP package declares Rust 1.88 minimum; the project build used Rust 1.90.0.

The bundled documents-skill LibreOfficeDev/Python/Poppler used for developer QA are
not copied into the application or this directory. Arial is requested by name in
the document; no Arial font bytes are distributed.
