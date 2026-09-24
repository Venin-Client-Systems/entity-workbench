# Entity Workbench v1 — three-month delivery programme

Start: **23 September 2026**. Target: **23 December 2026**. Six two-week sprints finish on 15 December, followed by an eight-day release/repair buffer. The date is a target; the agreed feature, security and offline-installation requirements are release conditions. A calendar expiry does not pass a gate.

The programme starts from source revision `73b34438c220efd89253bdbc44538b9af5688b9c`. The existing industrial-design, statement-mapping and assessment work is in PRs #1, #2 and #3. Their source checks pass, but the chain is not merged and PR #1 requires review. The implementation baseline includes 49 local Rust tests, six browser workflows, a native Apple Silicon development app, direct selected-host collection and local Lucene search. No complete-release gate is passed. See [current implementation](../STATUS.md) and [verification](../VERIFICATION.md).

## What finished means

The first supported release must let an analyst complete an investigation: questions and hypotheses; lead discovery and bounded collection; document/OCR review; identity and temporal relationship analysis; reconciled transactions; uncertain merchant/location assessment; linked graph/map/timeline/table views; and cited, identifiable DOCX/HTML assessments and data exports. All required local runtimes and advertised assets ship with Windows 11 x64, macOS Apple Silicon and macOS Intel distributions.

Rust owns canonical writes and network policy. Java parser and Lucene processes remain separate. Python supplies narrowly scoped conventional analyses. DuckDB/Spatial and Arrow/Parquet snapshots are rebuildable and revision-labelled. No generative model, hosted search provider, account/key requirement, separately operated service, arbitrary downloaded plugin or remote map dependency is introduced.

Online discovery uses explicitly selected public-web information and local indexing. Client addresses, account information and statements stay local. Direct public publishers are data sources; upstream build downloads and OS signing/notarization are development/distribution activities, not runtime search providers. There is no claim of an exhaustive global web index. The real usefulness of broad, multi-source discovery must pass EW-05; a narrow mock corpus cannot substitute for that gate.

English is the initial advertised OCR language. Additional languages require bundled and tested assets. At least one real, redistribution-compatible regional map/address pack must be selected during Sprint 1 and delivered for the matching regional distribution. Synthetic map data is a demonstration fixture, not production address coverage. Minimum Mac versions, initial regional coverage and signing/test access are explicit readiness decisions; do not quietly advertise unsupported combinations.

## Delivery calendar

| Sprint | Dates | Committed outcome to aim for | Exit evidence | Work items |
|---|---|---|---|---|
| S1 — risk and contracts | 23 Sep–6 Oct | Runtime/acceptance contracts; Windows and Mac confinement feasibility; measured direct-web discovery; release access and reviewed baseline | Hostile/benign probes per platform, coverage benchmark or concrete blocker, inventory failure report, access/region decisions | EW-01–05, EW-08, EW-41; start design and regional-data selection |
| S2 — packaged foundations | 7–20 Oct | Bundled engines, bounded durable jobs, workspace recovery UI, document/OCR integration and installed vertical slice | Parse, OCR, search, transaction analysis and local map from installed app on all three targets; no first-run downloads | EW-06–07, EW-09–13 |
| S3 — evidence and finance | 21 Oct–3 Nov | Difficult-document review, scalable local search, PDF/XLSX statements, typed analysis, transaction patterns and durable collection | Source-linked OCR/table corrections; independently reconciled totals; restart-safe collection; revision-labelled analytical results | EW-14–18, EW-22 |
| S4 — investigation and geography | 4–17 Nov | Calibrated candidate matching, temporal graph, lead/tasks, direct-source adapters, browser capture, typed recipes and regional packs | Explainable candidates/paths, real bounded lead expansion, confined capture, local address lookup and visible coverage limits | EW-19–21, EW-23–26 |
| S5 — integrated analyst acceptance | 18 Nov–1 Dec | Historical merchant/location analysis; linked views; reports/data exports; complete design; privacy/recovery/investigation scenarios | One fictional investigation works end to end; saved reports survive corrections; manual design/accessibility and recovery results | EW-27–33, EW-35 |
| S6 — release candidate | 2–15 Dec | Hostile/resource campaign, measured performance, security/licence review, signed installers and downloaded-artifact offline tests | Exact candidate artifacts pass required gates on clean Windows 11 and both Mac architectures | EW-34, EW-36–39 |
| Release buffer | 16–23 Dec | Repair/retest only, bundled help and tutorial acceptance, final gate reconciliation and publication | Identical signed downloadable artifacts, checksums/SBOM/notices, acceptance evidence and release sign-off | EW-40 |

Design is continuous: each user-facing issue requires its relevant editable Figma frame and rendered comparison before closure. EW-31 is final design/accessibility acceptance, not permission to postpone design until Sprint 5. Similarly, regional licensing and signing/test-machine readiness begin in S1 despite later implementation milestones. Build/install smoke checks run from S2 onwards, not only in the last fortnight.

## Capacity, uncertainty and scheduling rules

Sizing is a deliberately coarse engineering envelope: S = 1–2 focused days, M = 2–4, L = 4–8. It includes implementation and issue-level verification, not waiting for accounts/hardware/review. These estimates are uncalibrated. The issue index reports the total and must be re-estimated from actual first-sprint throughput.

The calendar assumes concurrent platform/distribution, evidence/analysis, and analyst-workflow/design effort, plus timely maintainer reviews and native test access. A single serial implementation stream is unlikely to deliver the entire agreed scope in this window. The owner authorized parallel agents on 24 September 2026 for independent GitHub issues. The integration owner assigns isolated worktrees and file ownership, reviews each result and validates interfaces before integration. Use concurrency for work whose implementation dependencies are satisfied; retain outstanding review/release dependencies explicitly. No unattended three-month scheduler is installed by this plan. Execution proceeds while the agent is active, and each continuation resumes from the issue board and recorded evidence.

**Confirmed capacity — 24 September:** the project has one human owner and only the current development Mac. Windows and Intel Mac release-test environments and an independent human reviewer are unavailable. The current Apple Silicon Mac can support local development and controlled synthetic testing; it does not establish a clean installation environment, reserved performance capacity or signing access. Parallel coding agents expand implementation capacity, while integration and owner decisions remain serial constraints.

Prioritize locally executable work on application workflows, bundled engines, Mac confinement and direct collection. Keep Windows and Intel source builds running in CI and prepare their probes/install tests without claiming those builds satisfy native release acceptance. Track access and review gaps in EW-08. All three distributions remain required, and 23 December remains a conditional target: the current resources alone do not establish a feasible complete-release date. Reforecast by the S1 exit using measured throughput and actual access; any change to release scope requires a separate owner decision.

Reserve roughly 20% of implementation capacity for defects, integration and review; keep the final buffer free of planned feature work. By 6 October, replace coarse sizes with measured throughput and reassess the critical path. If capacity, confinement compatibility or coverage cannot support the target, record the earliest achievable forecast and concrete alternatives. Changing a mandatory requirement needs an explicit owner decision. Do not lower acceptance criteria, label an unsupported feature complete, or release an unsigned partial build to meet a date.

Large issues must be split into independently verifiable implementation PRs when picked up, while the parent remains open until every acceptance criterion passes. Keep at most one active issue per implementation agent and one integration owner. Independent tasks may run concurrently only when authorized; dependencies cannot be bypassed by labeling them parallel.

## Critical paths and decision deadlines

| Path | Sequence | Early failure response |
|---|---|---|
| Native distribution | EW-01 → EW-03/04 → EW-06/07 → EW-09 → EW-38 → EW-39 → EW-40 | Missing confinement/runtime compatibility is a release blocker; keep worker activation fail-closed and continue independent UI/domain work |
| Document/financial workflow | EW-10/12 → EW-13 → EW-14/16 → EW-17/18 → EW-27/28/29 → EW-33 | Preserve partial/unsupported status and reviewable originals; do not invent extraction or totals |
| Provider-free discovery | EW-05 → EW-22 → EW-23/24 → EW-21/25/32 → EW-33/40 | By S1 exit publish coverage measurements and a concrete blocker if useful direct discovery cannot be demonstrated |
| Review and release access | EW-08/41 → EW-04/38/39/40 | Track secure-access and maintainer-review dependency with needed-by dates; no credential publication or review bypass |
| Performance/recovery | EW-02/10/15/17 → EW-28/33/35 → EW-36/39/40 | Profile on the 16 GB workload before RC, retain failed results and repair without changing previous report snapshots |

Source builds on hosted Windows Server are not Windows 11 runtime acceptance. Ad-hoc Mac signing and the existing experimental Seatbelt probe are not signed-helper release acceptance. Early probes must establish what the real native constraints permit. [Microsoft AppContainer isolation](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation) describes the platform boundary; the application still needs its own benign and hostile validation.

Tika parsing belongs behind that boundary because Tika does not establish one by itself; extracted content remains untrusted. [Apache Tika security model](https://tika.apache.org/security-model.html). Browser capture must explicitly enable Chromium sandboxing, because Playwright's launch default is disabled. [Playwright launch options](https://playwright.dev/python/docs/api/class-browsertype#browser-type-launch-option-chromium-sandbox).

Fixed WebView2 is bundled on Windows and its patch lifecycle belongs to releases. [Tauri Windows packaging](https://v2.tauri.app/distribute/windows-installer/#fixed-version). Signing/notarization must be tested with actual candidate packages and kept separate from development builds. [Tauri macOS signing](https://v2.tauri.app/distribute/sign/macos/).

## Acceptance scenarios and gate traceability

| Required end-to-end scenario | Responsible issues | Required result |
|---|---|---|
| Clean offline installation | EW-01, 06–09, 38–39 | Every advertised local feature works without internet, developer tools, dependency/model downloads or application-initiated traffic |
| Discovery from incomplete input | EW-05, 21–25 | A direct public source yields a new identifier and further relevant lead within exact declared limits |
| Difficult document import | EW-12–16 | Wrapped tables, OCR errors and duplicates are reviewable and source-linked |
| Identity ambiguity | EW-19–20 | Namesakes/conflicting dates stay distinguishable; mistaken merge reverses without erased history |
| Statement reconciliation | EW-16–18 | OCR errors, overlaps, refunds, repeated purchases, transfers and currencies produce explainable totals |
| Proximity analysis | EW-26–28 | Ambiguous branches, online channels, relocated merchants and changed addresses produce resolved or uncertain findings appropriately |
| Correction propagation | EW-17–20, 27–30, 35 | Current tables/maps/graphs/findings update; earlier report snapshots remain byte-identical |
| Hostile input/networking | EW-03–04, 12–13, 24, 32, 34 | Scripts, archive escapes, malformed IPC, injection and direct worker networking fail safely |
| Recovery/upgrades | EW-10–11, 35 | Interrupted jobs resume without duplicate observations; failed migrations retain a usable evidence-inclusive recovery point |
| Broad direct-web/local search | EW-05, 15, 22–24 | No-payment/no-account/no-key/no-hosted-provider path demonstrates actual useful results and reports coverage limits honestly |

All twelve machine-readable gates in `docs/release-gates.json` are mapped in `gate-coverage.json`; issue closure alone does not pass a gate. EW-02 now supplies evidence-backed declarations in PR #49. Cross-platform gates require real evidence on every claimed target. The release is complete only after EW-40 reconciles the gate evidence with the exact downloadable signed binaries.

The benchmark is fixed at a documented 16 GB machine, 100,000 transactions, 10,000 document pages including 1,000 scans and concurrent map/graph use. Target ordinary indexed search and transaction filters at p95 under two seconds. Record real import/OCR throughput, peak memory and query mix before performance claims.

## Issue execution and review protocol

1. Select the highest-priority ready issue whose hard dependencies have accepted implementation evidence. Record the issue, branch/base and intended verification; update its status to in progress.
2. Read the relevant source, invariants and tests. For UI work, create/refine the editable Figma design and compare the working native/browser state. Use only synthetic evidence in repository/test/artifact material.
3. Implement the smallest reviewable slice. Rust owns business rules, validation, workspace writes and networking; workers adapt their specific engines.
4. Run meaningful affected tests, strict checks and platform/engine checks. Add regression coverage for the actual failure mode, not implementation-mirroring tests. Keep existing release gates accurate.
5. Audit exact staged public files against the private exclusion list. Publish a signed commit and linked PR with outcome, validation and limitations. External issue/PR updates identify Codex as the author.
6. Mark the issue in review only after acceptance evidence is recorded. PR creation is not delivery. Close after the implementation is integrated and the issue's acceptance is verified; release-gate issues also require their platform/artifact evidence.
7. Respect required reviews; never self-approve or force a protected merge. If one issue is blocked, record the concrete dependency and continue the next independent ready issue.

Use P0 for release-blocking correctness/security/foundation/dependency work and P1 for required product capabilities. P1 does not mean optional. Status labels are ready, in-progress, blocked and in-review; closed means accepted. All 41 issues remain part of the agreed programme. Weekly review checks accepted outcomes, blockers, test failures, actual versus forecast effort and the critical-path forecast. Sprint exit checks evidence, not the count of closed tickets.

## Starting execution

EW-01 is first: implement and test an offline bundle-inventory validator with target-specific runtime requirements. It is useful immediately, can fail honestly on the existing partial Mac bundle, and provides an acceptance contract for the packaging issues. EW-02 follows for evidence-backed release gates. Platform confinement, direct-web feasibility and release-access issues begin early as their environments become available. The existing PR chain is tracked separately so useful isolated implementation can proceed without bypassing review.
