# OCR model and Leptonica licence sources

The build-time staging script retains installed dependency licences and notices in the app-local runtime. Two pinned licences are included here because local installations do not necessarily include them:

- `tessdata_fast-LICENSE`: [Tesseract tessdata_fast 4.1.0 LICENSE](https://github.com/tesseract-ocr/tessdata_fast/blob/4.1.0/LICENSE), Apache-2.0. English model SHA-256: `7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2`.
- `leptonica-LICENSE`: [Leptonica 1.87.0 licence](https://github.com/DanBloomberg/leptonica/blob/1.87.0/leptonica-license.txt), BSD-2-Clause.

These licence texts are third-party material; the project's Apache-2.0 licence applies to original code. Staged NOTICE.json records each native source version and source/bundle hashes, and the dependency-path/ad-hoc-signature modifications. No runtime binaries are committed here.
