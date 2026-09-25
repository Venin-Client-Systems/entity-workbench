# Typed CSV transaction export interface

The ledger now offers **JSON — exact canonical values** and **CSV — visible typed literals** for the complete applied scope. JSON remains the default. CSV starts with an explicit policy that refuses the whole export when any selected row is not accepted. The analyst can instead choose to include every selected review state. Neither choice changes review decisions or silently removes rows.

## Editable design and actual comparison

These native editable frames extend the existing industrial ledger frame in **Entity Workbench — development design**. They are development specimens, not final product approval:

| Frame | Native node | Size | Figma export |
| --- | --- | --- | --- |
| 28 / Instrument — Typed CSV export | [2023:2](https://www.figma.com/design/CphN4aTS8IXSVlKX7zDJtg/Entity-Workbench?node-id=2023-2) | 1120 × 940 | [Export controls](evidence/typed-csv-20260926/figma-csv.png) |
| 29 / Instrument — CSV export outcomes | [2023:41](https://www.figma.com/design/CphN4aTS8IXSVlKX7zDJtg/Entity-Workbench?node-id=2023-41) | 720 × 980 | [Outcome reference](evidence/typed-csv-20260926/figma-outcomes.png) |

Native Figma text/vector layers, frame dimensions and selection were observed through computer use. The PNGs were downloaded from Figma and visually inspected. Figma's available Roboto Mono replaced a missing IBM Plex Mono face in the specimen; the application retains its existing packaged fonts. The original file and existing frames remain intact. Browser control initially failed; the correct signed-in profile and native controls subsequently enabled authoring and export. A later reload action produced no observable reload, so post-reload export equality remains unverified.

[Actual wide controls](evidence/typed-csv-20260926/app-wide.png) and [compact controls](evidence/typed-csv-20260926/app-compact.png) retain the comparison. The application uses its existing tighter text scale, hard edges, graphite header, wrapped selectors and amber notice. Its layout follows the frame's order: complete scope/revision, pre-review denominator, format/policy, typed-value explanation, inclusion consequence, file semantics and export action. The outcome frame is a reference to mutually exclusive states, not a claim that the application displays four simultaneous cards. Exact runtime errors, locations and hashes differ from design placeholders. A complete compact editable control specification, 200% zoom campaign and all assistive-technology/native visual states remain future work.

## File and lifecycle contract

The visible examples distinguish exact tagged text from spreadsheet numeric/date cells: `text:000042`, `decimal:-0.10000001`, `date:2024-02-29` and `text:=SUM(A1)`. Missing values use unprefixed `null`; present empty text uses `text:`. The interface explains that removing prefixes can change interpretation. It does not promise universal spreadsheet safety. See the canonical [typed CSV contract](../transactions/TYPED-CSV.md).

Rust continues to own selection, review policy, ordering, exact values and source validation. The interface freezes the applied query/filter/order, revision, complete selected count, matching metadata and policy for each action. A draft change, new applied scope, revision change or unmount prevents late browser bytes or a late native preparation from being committed. No frontend financial calculation or Unicode matching implementation was added.

Browser development export checks the exact typed response, request, count, revision, matching metadata, fixed dictionary identity, UTF-8 byte count and SHA-256. A bounded structural pass rejects incomplete records, wrong columns, unquoted fields and invalid type prefixes. It does not parse money or normalize values. The canonical selection digest is validated as a digest and accompanied by exact request/matching/revision checks; the browser does not independently recompute Rust's Unicode-dependent selection digest. The browser reports **download requested**, because it cannot attest that the user saved a file.

Native CSV uses the closed `transaction_csv` preparation request and version-2 receipt. Rust generates and publishes the file. JavaScript supplies neither artifact bytes nor an arbitrary path. The interface validates the exact format, dictionary digest, request, revision, count, matching metadata, artifact digest and expected filename. JSON, HTML and DOCX keep their version-1 envelopes. The existing lifecycle allows one automatic commit retry using the same ticket after an uncertain acknowledgement, then reports **Native save completion is unconfirmed** if both acknowledgements fail. It does not report success merely because preparation succeeded or silently prepare a replacement during that action. Stale or rejected preparations are discarded through the existing cleanup path.

## Verification and limits

The [validation manifest](evidence/typed-csv-20260926/validation.json) binds the final source files, real Rust executables, commands, synthetic output, screenshots and sanitized logs. Browser scenarios use the actual Rust development CLI and, for native saves, the persistent real `JobCoordinator` export session through a test IPC bridge. This is native backend/file-lifecycle evidence, **not** observed packaged WebKit interaction.

The native export lifecycle suite passed **19 Rust tests**, strict core Clippy passed, and the final UI type/build check passed. A **38-case browser compatibility run** passed; the **nine CSV cases** were rerun after the final accessible-name and singular-count wording corrections. Three axe scans reported zero violations, and the compact 760 × 1000 viewport retained keyboard export and no horizontal overflow. Existing large-chunk build warnings remain.

The synthetic 113-row fixture includes leading-zero accounts, eight decimal places, a large exact amount, formula-like literals, quoted multiline text and all four review states. Python's standard CSV reader independently decodes every exported field and compares every row with the canonical JSON export. Tests cover accepted-only and empty scopes, explicit mixed-state refusal/inclusion, stale revision, corrupted original, altered metadata/dictionary/hash, recomputed-hash truncation, late scope changes, unmount, same-ticket lost acknowledgement, two lost acknowledgements, no false save notice and actual staging cleanup. Existing JSON, historical HTML, frozen DOCX and summary-ledger workflows are rerun.

An initial test fixture used an empty required description and was correctly refused by the importer; the fixture was corrected without relaxing production validation. A broader compatibility run found a pre-existing test-only hardcoded port: valid requests on the isolated test port were labelled external. The assertion now accepts only the configured local origin plus the existing `blob:`/`data:` schemes. Failed observations are retained by digest and summarized in the manifest. Initial screenshot crops clipped the header behind the sticky application bar; the capture helper now temporarily increases screenshot height while accessibility checks still run at the original viewport.

The complete CSV artifact still has a 256 MiB backend limit and browser processing makes additional in-memory copies. This increment makes no performance or memory claim, does not prove behaviour in Excel or other spreadsheet applications, and does not complete EW-30 or the cross-platform release gates. No core command, schema, dependency, native runtime or canonical storage behaviour changed.

## Actual Mac application observation — 26 September 2026

A separate unsigned development `.app`, built at clean source
`0f65c56732babeed3d6a58f523f9fca821a32704`, was exercised through its actual
WebKit controls on the available Apple Silicon Mac. Its exclusive synthetic
workspace contained three transactions at revision 2: one accepted and two
pending, leading-zero account identifiers, eight-place decimal amounts, two
currencies, a formula-like description and quoted multiline text. This app used
an isolated identifier and no bundled worker resources; it was not an installer
or full-runtime acceptance run.

The default CSV policy visibly refused the mixed-review scope and created no
file or staging residue. Explicit inclusion saved all three records. A second
export reused the same filename and hash, leaving exactly one CSV file. An
independent standard-library CSV reader decoded every one of the 15 columns and
compared all fields with the canonical transactions, preserving exact decimals,
leading zeros, missing values and embedded line breaks. The 1,396-byte BOM/CRLF
artifact has SHA-256
`c07c2a4a252a770c7afcc37fc5d845a8ed380d12466f77a9b4d1c5bb28c2fadc`.

Normal quit and reopen retained the three rows and revision. A subsequent JSON
save produced the unchanged historical array contract, exactly equal to the
canonical records: 1,813 bytes, SHA-256
`6629e680f6eeb21318e0078916ec0c0f17796fb4180ff599761da06537caf46b`.
The first independent JSON check incorrectly expected an envelope; correcting
that verifier to the existing array shape required no app change or repeat
export. The previously saved CSV remained byte-identical. All canonical tables
and original files matched the pre-action baseline, the staging directory was
empty, and the saved CSV was a single-link mode-0600 file. Final application
exit was confirmed.

The [native observation](../transactions/verification/native-csv-ui-first-2026-09-26.json)
binds source, binary, configuration, frontend assets and the scoped checks;
its SHA-256 is
`0ac07afe85922e2264a4396dbe1692f10125630361650ba3321e8b344f25b6cb`.
Private path-bearing screenshots and raw workspace evidence are not published.
This establishes one native development workflow, with no claim of spreadsheet
application execution, universal formula safety, other-platform behaviour,
clean installation, signing or release readiness.
