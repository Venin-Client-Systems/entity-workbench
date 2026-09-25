# Statement mapping and import

CSV and TSV statement import runs entirely in Rust on the local device. Choose **Import evidence**, select the file, then map columns and preview before saving any transactions. No statement bytes, account identifiers or mapping information leave the application.

1. Choose comma, semicolon or tab separation. The source sample shows the first five logical records; cells are shortened to 240 characters in this sample only.
2. Choose an explicit date format (`YYYY-MM-DD`, `DD/MM/YYYY` or `MM/DD/YYYY`), dot/comma decimal interpretation, and oldest/newest-first row order. Dates are not inferred from locale or guessed from ambiguous examples.
3. Map transaction date and original description, plus optional posting date and running balance. Use a signed amount with explicit debit polarity or separate nonnegative debit/credit columns. Account and currency can come from columns or fixed values. Account identifiers remain text, including leading zeros.
4. Preview every row. The interface displays up to 50 interpreted rows and the first 100 errors, with counts for the whole file. Any invalid row blocks the entire import. Balance mismatches are review warnings, not automatic corrections; the first available balance has no preceding balance to compare.
5. Optionally save a uniquely named mapping. Import retains the original bytes, mapping snapshot and pending transactions in one canonical database transaction. A saved mapping is local to the workspace. Reuse it by name on a subsequent file, then preview again.

`fixtures/statement-mapped.csv` demonstrates semicolon separation, day-first dates, comma decimals, separate debit/credit values, newest-first order and a quoted multiline description. Map `Booked`, `Narrative`, `Paid out`, `Paid in`, `Running balance`, `Account ref` and `Unit` to their corresponding fields. The result is three pending transactions with amounts `12.30`, `-12.30` and `1000.00` AUD and account `000017`; available balances reconcile.

## Interpretation and evidence

Amounts use exact decimal strings with at most eight fractional digits. Optional grouping must use groups of three; currency symbols and exponent notation are rejected. A leading sign or enclosing parentheses can express a negative signed value. Separate debit/credit columns cannot contain negatives or two nonzero values. Currency is normalized to a three-letter uppercase code; no currency conversion occurs.

Descriptions and original evidence bytes are preserved. Normalized dates and amounts are separate interpretations. The import record retains the entire mapping, its optional profile identifier and transaction identifiers. Source anchors use logical CSV record numbers (header is row 1), so a quoted newline does not move a later transaction to the wrong record. Source inspection uses the retained separator and displays the original amount cell. Physical text-line numbering in the full source viewer is explicitly separate.

Newest-first input is reversed for canonical insertion and balance calculations, while preview and source anchors keep the original record order. Transactions enter pending review and do not contribute to reviewed totals until accepted. Import never silently discards repeated purchases. The identical original cannot add transactions a second time; differing overlapping files are retained and probable duplicate candidates are flagged for review.

A preview is bound to the original digest, display filename, exact mapping and workspace revision. Changing any of those requires a new preview. The binding guards stale results; it is not an authorization token or a claim that an analyst has checked every row. The UI also ignores responses from an outdated preview request.

## Current limits

Inputs must be UTF-8, optionally with a leading BOM, contain one header row with 1–128 unique nonempty columns, and fit within 16 MiB and 100,000 data rows. This is an input limit, not a verified performance claim. A malformed initial source sample may prevent mapping until the analyst supplies a structurally valid export. Empty headers, duplicate headers, invalid row widths, unsupported dates and invalid amounts fail explicitly.

This increment does not add XLSX/PDF/OCR extraction, multiple tables per file, profile editing/deletion, remapping an already-retained original, automatic source-format detection or a workspace restore UI. Existing CSV imports without mapping metadata use the original comma dialect. Version 2 workspaces prevent older applications from opening mapped sources with incompatible assumptions; see [recovery guidance](OPERATIONS.md).
