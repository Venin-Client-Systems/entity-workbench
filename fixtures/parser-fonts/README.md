# Synthetic extraction font fixtures

These generated PDFs contain fictional text only. They test the Windows extraction-only app-local font policy; they are not rendering reference images or an advertised language-coverage corpus.

- `corpus.pdf`: 21 pages; all Standard 14 faces, accented Latin/euro, Symbol and Zapf Dingbats encodings, an embedded/subset font, non-embedded unknown fonts with/without declared widths, custom Differences including a ligature, simple-font and CID ToUnicode maps containing CJK/supplementary/Arabic characters, and a CID font without a ToUnicode map.
- `embedded.pdf`: one page with embedded Latin/Greek/Cyrillic text; parsing must not claim font substitution.

Per-page BEGIN/END markers prevent one Unicode case from masking another. Expected Unicode, page counts, all twelve Latin-family face labels and fallback limitations are asserted independently in the Java tests and the native Rust control/confinement harness. CJK/Arabic characters with ToUnicode can be extracted despite absent fallback glyphs; this does not establish faithful rendering, shaping or layout. The no-ToUnicode CID case remains explicitly partial with unverified coverage.

The generator is `workers/java/src/test/java/workbench/FontFixtures.java`, compiled only in the test classpath. It is excluded from staged worker JARs. After the pinned Maven test build, regenerate on a POSIX development shell with:

```sh
java -cp 'workers/java/target/test-classes:workers/java/target/classes:workers/java/target/lib/*' workbench.FontFixtures fixtures/parser-fonts/corpus.pdf fixtures/parser-fonts/embedded.pdf
```

Two independent local generations produced identical bytes. SHA-256:

- corpus: `836c0cbd6b5e686b5ae761c6e7fdb794ef1f5f50f5308bc71292ec5e659395d4`
- embedded: `f4cf6d3bd3fed0a4c43ad679baf474df40b1f3fe278105d764773fac65fbc308`

The embedded font is a PDFBox-produced subset of PDFBox 3.0.8's Liberation Sans Regular 2.1.5 resource. Its original resource SHA-256 is `76d04c18ea243f426b7de1f3ad208e927008f961dc5945e5aad352d0dfde8ee8`. Preserve [the font copyright/licence](LICENSE.font.txt). The original font is SIL OFL 1.1, not Apache-2.0. The staging process preserves PDFBox's complete original JAR, including all fourteen AFM resources and their distribution notices in `META-INF/LICENSE`, plus `META-INF/NOTICE`.
