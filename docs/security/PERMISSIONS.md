# Process and filesystem permissions

| Process | Read | Write | Network / IPC |
|---|---|---|---|
| Tauri/Rust coordinator | Application assets, selected imports, active workspace | Canonical DB, originals, indexes, job areas, backups, exports | Explicit direct HTTPS via broker; local Tauri IPC |
| Bundled webview | Bundled assets and validated command responses | UI state only; canonical changes through Rust commands | Tauri IPC; no remote documents, tiles or scripts |
| Java Lucene worker (macOS prototype) | Bundled Java/JARs, OS bootstrap libraries, derived corpus/cache | Assigned rebuildable index/job directory | Direct network denied by Seatbelt; OS bootstrap policy applies |
| Java parser (Mac development) | Assigned original, its separate classpath and runtime/OS libraries | Assigned bounded scratch/results | Direct network and canonical DB access denied by experimental Seatbelt profile |
| Java image/PDF workers (Mac development) | Assigned original/page, method-specific classpath and runtime/OS libraries | Assigned bounded raster/result/scratch | Separate from parser/search; direct network and canonical DB access denied |
| Native English OCR (Mac development) | Assigned canonical PGM, inventoried Tesseract/model/libraries and OS libraries | Fixed text output; separate region recipe additionally permits TSV; assigned scratch | Direct network, fork and canonical DB access denied; inherited descriptors/environment restricted |
| Python analysis (not enabled in app) | Intended: specified Arrow/Parquet/job inputs and packaged runtime | Intended: bounded analytical derivatives | Must deny network and canonical workspace access |
| Browser capture (not implemented) | Intended: fresh dedicated profile and broker-delivered page resources | Intended: disposable capture outputs | Must deny direct networking and use explicit Chromium sandboxing |

macOS workspace directories are created with mode 0700; originals 0400 and databases/exports/backups 0600. The Windows native ACL implementation is still a release gate. Originals' read-only mode protects against routine writes, not same-user hostile code by itself.

No downloaded plugin execution, arbitrary SQL, shell command field, custom executable path or user credential profile is exposed by the desktop UI.

For exact recipes, allowed bootstrap access, hostile probes and remaining limitations, see [Mac worker supervision](MACOS-WORKERS.md), [image decoding](IMAGE-WORKERS.md), [PDF rendering](PDF-RENDER-WORKERS.md), [OCR](OCR-WORKERS.md) and [word regions](OCR-WORD-REGIONS.md). macOS file metadata access is broader than file-content access. Per-file/CPU/time bounds and monitored tree limits do not establish hard aggregate disk or resident-memory ceilings. Unknown process termination blocks result acceptance and suspends canonical processing; supervisor-crash recovery remains open.
