# Delivery issue index

[Programme and acceptance plan](THREE-MONTH-PLAN.md). Source catalogue: [backlog.json](backlog.json).

Coarse sizing envelope: **130–260 focused engineering days** across the full remaining scope, before external waiting. This is an uncalibrated planning range, not a delivery promise; concurrent work and first-sprint re-estimation are essential to the three-month target.

| Key | Sprint | Priority | Size | Outcome | Hard dependencies |
|---|---|---|---|---|---|
| [EW-01 · #5](https://github.com/Venin-Client-Systems/entity-workbench/issues/5) | S1 | P0 | M | Verify app-local runtime inventories before packaging | None |
| [EW-02 · #6](https://github.com/Venin-Client-Systems/entity-workbench/issues/6) | S1 | P0 | M | Bind release gates to reproducible acceptance evidence | [EW-01 · #5](https://github.com/Venin-Client-Systems/entity-workbench/issues/5) |
| [EW-03 · #7](https://github.com/Venin-Client-Systems/entity-workbench/issues/7) | S1 | P0 | L | Prove Windows AppContainer isolation for specialist workers | [EW-01 · #5](https://github.com/Venin-Client-Systems/entity-workbench/issues/5) |
| [EW-04 · #8](https://github.com/Venin-Client-Systems/entity-workbench/issues/8) | S1 | P0 | L | Prove macOS helper confinement on both architectures | [EW-01 · #5](https://github.com/Venin-Client-Systems/entity-workbench/issues/5) |
| [EW-05 · #9](https://github.com/Venin-Client-Systems/entity-workbench/issues/9) | S1 | P0 | L | Demonstrate useful direct-web discovery without a search provider | None |
| [EW-06 · #10](https://github.com/Venin-Client-Systems/entity-workbench/issues/10) | S2 | P0 | L | Package Java parsing, Lucene and OCR on every target | EW-01, EW-03, EW-04 |
| [EW-07 · #11](https://github.com/Venin-Client-Systems/entity-workbench/issues/11) | S2 | P0 | L | Package Python, analytical assets and browser capture runtime | EW-01, EW-03, EW-04 |
| [EW-08 · #12](https://github.com/Venin-Client-Systems/entity-workbench/issues/12) | S1 | P0 | S | Resolve signing, clean-machine and release-review access | None |
| [EW-09 · #13](https://github.com/Venin-Client-Systems/entity-workbench/issues/13) | S2 | P0 | M | Run the complete packaged foundation slice on three targets | EW-02, EW-03, EW-04, EW-06, EW-07 |
| [EW-10 · #14](https://github.com/Venin-Client-Systems/entity-workbench/issues/14) | S2 | P0 | L | Persist bounded jobs, cancellation and restart recovery | [EW-02 · #6](https://github.com/Venin-Client-Systems/entity-workbench/issues/6) |
| [EW-11 · #15](https://github.com/Venin-Client-Systems/entity-workbench/issues/15) | S2 | P0 | M | Add workspace management and usable backup/restore | EW-02, EW-03 |
| [EW-12 · #16](https://github.com/Venin-Client-Systems/entity-workbench/issues/16) | S2 | P0 | L | Integrate bounded PDF, Office, image and email extraction | EW-03, EW-04, EW-06, EW-10 |
| [EW-13 · #17](https://github.com/Venin-Client-Systems/entity-workbench/issues/17) | S2 | P0 | L | Integrate offline OCR with reviewable page regions | EW-06, EW-10, EW-12 |
| [EW-14 · #18](https://github.com/Venin-Client-Systems/entity-workbench/issues/18) | S3 | P1 | L | Finish evidence review, derivative history and extraction acceptance | EW-12, EW-13 |
| [EW-15 · #19](https://github.com/Venin-Client-Systems/entity-workbench/issues/19) | S3 | P0 | M | Make local corpus search incremental and scalable | EW-06, EW-10, EW-12 |
| [EW-16 · #20](https://github.com/Venin-Client-Systems/entity-workbench/issues/20) | S3 | P0 | L | Extend statement mapping to XLSX and searchable/scanned PDFs | EW-13, EW-14 |
| [EW-17 · #21](https://github.com/Venin-Client-Systems/entity-workbench/issues/21) | S3 | P0 | M | Publish typed analytical snapshots with revision validation | EW-07, EW-10 |
| [EW-18 · #22](https://github.com/Venin-Client-Systems/entity-workbench/issues/22) | S3 | P1 | L | Complete transaction patterns, comparisons and account-flow analysis | EW-16, EW-17 |
| [EW-19 · #23](https://github.com/Venin-Client-Systems/entity-workbench/issues/23) | S4 | P1 | L | Calibrate identity candidates and preserve reversible decisions | EW-14, EW-17 |
| [EW-20 · #24](https://github.com/Venin-Client-Systems/entity-workbench/issues/24) | S4 | P1 | M | Add temporal relationships and explainable graph paths | EW-17, EW-19 |
| [EW-21 · #26](https://github.com/Venin-Client-Systems/entity-workbench/issues/26) | S4 | P1 | M | Complete lead inbox, case tasks and hypothesis-driven pivots | EW-14, EW-22 |
| [EW-22 · #25](https://github.com/Venin-Client-Systems/entity-workbench/issues/25) | S3 | P0 | L | Make direct collection durable, cancellable and policy-bounded | EW-05, EW-10 |
| [EW-23 · #27](https://github.com/Venin-Client-Systems/entity-workbench/issues/27) | S4 | P1 | L | Add reviewed direct-source discovery adapters | [EW-22 · #25](https://github.com/Venin-Client-Systems/entity-workbench/issues/25) |
| [EW-24 · #28](https://github.com/Venin-Client-Systems/entity-workbench/issues/28) | S4 | P0 | L | Capture dynamic pages in a broker-only sandboxed browser | EW-03, EW-04, EW-07, EW-22 |
| [EW-25 · #29](https://github.com/Venin-Client-Systems/entity-workbench/issues/29) | S4 | P1 | M | Publish typed discovery and analysis recipes | EW-17, EW-21, EW-23 |
| [EW-26 · #30](https://github.com/Venin-Client-Systems/entity-workbench/issues/30) | S4 | P0 | L | Deliver versioned offline map and address data packs | EW-01, EW-07, EW-17 |
| [EW-27 · #31](https://github.com/Venin-Client-Systems/entity-workbench/issues/31) | S5 | P1 | L | Complete merchant, channel and historical proximity analysis | EW-18, EW-23, EW-26 |
| [EW-28 · #32](https://github.com/Venin-Client-Systems/entity-workbench/issues/32) | S5 | P1 | M | Connect timelines, maps, graphs and tables to reviewed evidence | EW-18, EW-20, EW-27 |
| [EW-29 · #33](https://github.com/Venin-Client-Systems/entity-workbench/issues/33) | S5 | P1 | L | Finish DOCX and HTML report assembly with cited exhibits | EW-14, EW-18, EW-20, EW-27 |
| [EW-30 · #34](https://github.com/Venin-Client-Systems/entity-workbench/issues/34) | S5 | P1 | M | Export CSV, JSON, Parquet and graph interchange | EW-17, EW-20 |
| [EW-31 · #35](https://github.com/Venin-Client-Systems/entity-workbench/issues/35) | S5 | P1 | L | Complete industrial design and accessibility acceptance | None |
| [EW-32 · #36](https://github.com/Venin-Client-Systems/entity-workbench/issues/36) | S5 | P0 | M | Verify network disclosure and destination enforcement end to end | EW-22, EW-23, EW-24, EW-27 |
| [EW-33 · #37](https://github.com/Venin-Client-Systems/entity-workbench/issues/37) | S5 | P0 | L | Pass the complete synthetic investigation acceptance suite | EW-09, EW-11, EW-15, EW-18, EW-19, EW-21, EW-24, EW-25, EW-27, EW-28, EW-29, EW-30 |
| [EW-34 · #38](https://github.com/Venin-Client-Systems/entity-workbench/issues/38) | S6 | P0 | L | Run hostile-input, IPC and resource-exhaustion campaigns | EW-03, EW-04, EW-12, EW-13, EW-24, EW-32 |
| [EW-35 · #39](https://github.com/Venin-Client-Systems/entity-workbench/issues/39) | S5 | P0 | L | Prove crash recovery, migrations and correction consistency | EW-10, EW-11, EW-17, EW-28 |
| [EW-36 · #40](https://github.com/Venin-Client-Systems/entity-workbench/issues/40) | S6 | P0 | L | Meet measured 16 GB performance and responsiveness targets | EW-15, EW-17, EW-28, EW-33 |
| [EW-37 · #41](https://github.com/Venin-Client-Systems/entity-workbench/issues/41) | S6 | P0 | M | Publish security, licence and supply-chain review materials | EW-06, EW-07, EW-26, EW-32, EW-34 |
| [EW-38 · #43](https://github.com/Venin-Client-Systems/entity-workbench/issues/43) | S6 | P0 | L | Produce signed installers and notarized Mac release candidates | EW-08, EW-09, EW-33, EW-34, EW-35, EW-37, EW-41 |
| [EW-39 · #44](https://github.com/Venin-Client-Systems/entity-workbench/issues/44) | S6 | P0 | L | Verify actual downloaded artifacts on clean offline machines | EW-36, EW-38 |
| [EW-40 · #45](https://github.com/Venin-Client-Systems/entity-workbench/issues/45) | S7 | P0 | M | Finish bundled help, acceptance sign-off and v1 publication | EW-05, EW-31, EW-33, EW-34, EW-35, EW-36, EW-37, EW-39 |
| [EW-41 · #42](https://github.com/Venin-Client-Systems/entity-workbench/issues/42) | S1 | P0 | S | Review and integrate the existing pull-request chain | None |

GitHub programme: https://github.com/Venin-Client-Systems/entity-workbench/issues/4. All 41 work items are native sub-issues with explicit blocked-by relationships.
