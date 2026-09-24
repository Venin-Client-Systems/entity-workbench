# Industrial document processing — design handoff

The Evidence area now queues retained originals, reviews durable document jobs, cancels an exact reserved attempt and requests a manual retry with a reason. Extraction review opens an immutable, unreviewed derivative with its original digest, parser status, limitations and metadata. The interface uses the actual Rust command v5 contract; it does not publish observations, accept extraction quality or invent source anchors.

## Editable design

Three frames were authored and inspected in the existing Figma Design file as native editable text and vector layers:

| Frame | Purpose | Dimensions |
|---|---|---|
| [08 / Instrument — document jobs, 28:636](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=28-636) | Queue control, compact job ledger, attempts and derivative counts | 1120×780 |
| [09 / Instrument — extraction review, 28:690](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=28-690) | Unreviewed text, limitations and original/derivative provenance | 920×1100 |
| [10 / Instrument — document attempt controls, 28:730](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=28-730) | Manual reservation, retained derivatives and cancellation-pending semantics | 920×840 |

The frames are independent root-level specimens, exported through Figma. They contain editable primitives, not flattened application screenshots. Their example hashes, byte counts and statuses are illustrative synthetic design content. Application screenshots use records returned by the Rust development fixtures and therefore have different identifiers and timestamps.

The design retains the existing graphite/amber system: square controls, thin rules, bundled Inter and JetBrains Mono, joined measurement cells, restrained status fills and an amber review edge. The attempt-controls specimen places related states together for comparison; the application presents retry in a separate modal with an explicit reason and confirmation action. No interactive Figma prototype or owner design acceptance is claimed.

## Controls and state

| Element | Behaviour |
|---|---|
| Queue | Select a retained original and submit a canonical UUID request key. A known active job disables a fresh duplicate submission. An uncertain acknowledgement retains its key in the open application's workspace context, including navigation away from Evidence and back. Recovery of that acknowledgement returns the same job even if it has since become terminal. Keys are not persisted across application restart or browser reload. |
| Job ledger | Serial local reads update the newest 200 jobs. The interface shows displayed, loaded and total counts and explains when filtering applies only to the loaded window. There is no fabricated pagination for older jobs. |
| State | Queued, running, cancellation requested, partial, blocked, quota exhausted, failed, cancelled and completed remain distinct. Completion is neutral and does not imply analyst acceptance. |
| Attempt | The displayed attempt is reserved by Rust, beginning at one. Claiming it does not increment it. Retry reserves the next attempt immediately; allocated cancelled attempts count toward the three-attempt limit. |
| Cancellation | Commands carry the displayed expected attempt. A running request remains pending until a worker outcome is recorded. A racing cancellation may return an already-terminal job; feedback names the returned state and failure, without claiming a failed or unverified worker was stopped. |
| Manual retry | The dialog captures the expected attempt when opened and preserves the reason after a stale reservation or command failure. A changed attempt requires closing the review and inspecting current state. No automatic retry is initiated. |
| Unverified exit | `worker_exit_unverified` remains Failed even after a cancellation request. `recovery_required` explains suspended processing. These states have no retry control; Rust also refuses further queue/retry operations in that workspace. The UI provides no override. |
| Cleanup | `cleanup_failed` stays explicit, including on a cancelled record. Ordinary cleanup failure is not treated as proof of successful extraction. |
| Original binding | Job review displays retained original SHA-256, byte count, evidence ID and job timestamps. A legacy `unsupported_in_development_build` import label is shown as legacy/not-yet-processed; actual format support comes from the attempted parser result. |
| Derivatives | Earlier immutable results remain inspectable after retry. Their recorded attempt is read from the extraction itself, never inferred from array position. The canonical document job ID and the parser's worker request ID have separate labels. |
| Text and metadata | React text nodes and a read-only textarea render inert data. Raw document markup, scripts, images and links are not executed or fetched. No text is not evidence of no relevant information. |
| Copy | A deliberate user action copies only the unreviewed text. Clipboard refusal selects the text and offers the system copy shortcut. It does not alter the original or derivative. |
| Polling and focus | Reads are serial, stop on unmount and invalidate obsolete replies after selection changes or mutation. Errors mark retained state as potentially stale and disable dependent mutations. Native dialogs contain focus and support Escape. Closing extraction/retry returns to the available opener; closing a job returns to its current ledger entry or the original selector. |
| Local execution | Production commands use the native host. The development browser bridge explicitly says that it queues and inspects records without starting an ephemeral worker. Missing runtime, worker failure and incomplete results are visible. |

The job and extraction dialogs use a 920 px maximum width and the existing 90vh scroll boundary. Below 760 px, facts and measurement cells stack and long identifiers wrap. Retry uses the existing smaller decision dialog. The implementation adds exact job IDs, timestamps, current failure details, read errors and retry limits beyond the condensed design specimens.

## Rendered review evidence

| Surface | Editable Figma export | Working application |
|---|---|---|
| Document ledger | [Jobs design](review/document-jobs/figma-document-jobs.png) | [Desktop active filter](review/document-jobs/jobs-desktop.png), [complete ledger](review/document-jobs/jobs-full-surface.png), [compact active filter](review/document-jobs/jobs-compact.png) |
| Job / attempt | [Attempt design](review/document-jobs/figma-attempt-controls.png) | [Desktop job](review/document-jobs/job-desktop.png), [compact job](review/document-jobs/job-compact.png), [manual retry](review/document-jobs/retry-desktop.png) |
| Extraction | [Extraction design](review/document-jobs/figma-extraction-review.png) | [Desktop viewport](review/document-jobs/extraction-desktop.png), [complete review](review/document-jobs/extraction-full-surface.png), [compact viewport](review/document-jobs/extraction-compact.png) |
| Exceptional failure | Uses the attempt frame's explicit-state treatment | [Unverified exit](review/document-jobs/job-unverified-exit.txt.png), [recovery required](review/document-jobs/job-recovery-required.txt.png) |

Captures are region/dialog crops: ordinary desktop 1440×1000, compact 720×900, complete extraction 1440×2200 and complete ledger 1440×3200. Taller comparison viewports expose the full surface without altering application styles; ordinary windows scroll. A visual pass corrected dialog width and close-button placement and removed sticky-header overlap from the tall ledger capture by using an appropriate comparison viewport. These are manual comparisons, not pixel-equality assertions or native-platform acceptance.

The [checksum manifest](review/document-jobs/checksums.json) binds retained PNGs and accessibility results. Five axe runs under WCAG 2 A/AA and 2.1 AA record zero violations: [job ledger](review/document-jobs/accessibility-jobs-desktop.json), [desktop extraction](review/document-jobs/accessibility-extraction-desktop.json), [retry](review/document-jobs/accessibility-retry-desktop.json), [compact job](review/document-jobs/accessibility-job-compact.json) and [compact extraction](review/document-jobs/accessibility-extraction-compact.json). Automated checks do not establish screen-reader or native WebKit/WebView2 behaviour.

## Verification and synthetic fixture boundary

`ui/tests/document-jobs.spec.ts` contains ten workflows against the actual development Rust executable. The tests exercise queue/cancel/manual retry, immutable earlier derivatives, attempt exhaustion, polling, stale retry rejection with a retained draft, late reads after selection replacement, keyboard focus, exact provenance, inert hostile-looking content with zero external browser requests, clipboard success/refusal, exceptional process-recovery states and responsive accessibility. The lost-acknowledgement test executes a real queue request, drops only its transport acknowledgement, navigates away, cancels the job through Rust, then recovers the same terminal job after remount. Transport delay/drop tests never synthesize an API response or bypass canonical publication.

Two debug-only helpers accept only a fresh workspace path:

- `ew-dev seed-processing-review <workspace>` builds eleven fixed synthetic job states and four derivatives through canonical import, queue, claim, finish and cancel methods. The PDF-looking and unsupported specimens contain fixed synthetic text; their results are fixture values, not a claim that a parser processed those bytes. Running/cancellation specimens are state-only. No worker or network operation runs.
- `ew-dev seed-processing-recovery-review <workspace>` creates a separate fatal-state specimen through canonical `TerminationUnverified` publication after a cancellation request, plus a suspended pending job. It cannot contaminate the ordinary queue/retry workflow fixture. No OS worker is launched or left running.

Both helpers refuse nonempty workspaces, retain source digests, create no observations and preserve the existing demo behaviour. Their public Rust methods and CLI branches compile only with `debug_assertions`. There is no new desktop command, arbitrary JSON result, SQL writer or release dispatch path. Two core tests cover canonical results and no-overwrite guards; a release-profile check covers compile-time exclusion.

Validation on macOS Apple Silicon: UI production build; all 23 browser workflows (13 existing plus ten new); 93 ordinary core Rust tests; strict Clippy; release-profile core/dev-harness compile check. Five native runtime tests remain intentionally ignored in this ordinary core run and belong to the separate native verification programme. Targeted UI reruns cover subsequent peer-review fixes. The existing frontend chunk-size warning remains; no performance benchmark follows from these small fixtures.

## Remaining boundaries

This increment completes the bounded document job/extraction review surface, not the whole document-exploitation or release programme. OCR execution, verified page/region/cell anchors, extraction-to-observation acceptance, full historical job pagination, workflow recipes, signed packaging, clean offline installation and cross-platform runtime verification retain their own gates. The current derivative inspector does not add worker extraction text to the corpus index.

Pending request identity currently survives section navigation within one open application; restart/reload recovery still requires inspecting the durable job ledger before submitting a new request. Recovery from unverified OS worker exit has no UI override. Figma reusable components/auto-layout conversion, prototype wiring, owner acceptance, screen-reader review, 200% zoom and native Windows/macOS Intel interaction remain further design and release checks. This UI work does not strengthen the existing confinement claims or establish a complete release.
