# Typed-literal transaction CSV

This backend increment for EW-30 / issue #34 adds an explicit reversible CSV presentation of the same complete, revision-bound transaction selection as [raw JSON export](COMPLETE-EXPORT.md). It adds `export_transaction_csv` in Command v24 and request/result v1 schemas. It does not add a UI control, native download format, report attachment or release approval.

## Selection and consistency

```json
{
  "action": "export_transaction_csv",
  "expected_revision": 42,
  "request": {
    "selection": {
      "query": "",
      "filter": {
        "date_from": null,
        "date_to": null,
        "account": null,
        "currency": null,
        "review": null
      },
      "order": "date_ascending"
    },
    "non_accepted": "reject"
  }
}
```

The mandatory `non_accepted` policy is either `reject` or `allow_selected`. `reject` fails the entire export if **any selected transaction** is pending, deferred or rejected; it does not silently omit those rows. `allow_selected` explicitly includes all review states selected by the request. An explicit accepted-only filter works with `reject`. An empty valid scope produces the header and zero rows.

Both formats use one shared canonical selection helper. It retains the existing query/filter validation, Rust literal-lowercase matching profile, exact account/currency/review comparisons, inclusive dates, stable date order with canonical insertion-order ties, and source verification. No arithmetic, currency conversion, duplicate removal, transfer exclusion or review mutation occurs. The result includes the original selection and matching metadata. A stale revision, malformed canonical transaction, missing/altered original or source key/body/digest mismatch fails the whole result. Selected distinct originals are verified once each. All canonical transactions are validated while scanning, including malformed rows outside the selected query; the export is not a repair operation.

Selection, source metadata and formatting remain inside the same SQLite read snapshot. Each output row carries the captured workspace revision and exact canonical transaction ID/version/anchor. A concurrent correction cannot produce mixed versions. File integrity is verified at export time; the artifact is not proof of continued storage integrity after export or of an analyst accepting the source.

## Explicit presentation format

`typed_literal_v1` is UTF-8 with a three-byte BOM. It uses comma separators, all fields double quoted, doubled internal quotes and CRLF record separators. Embedded CR/LF and commas remain in quoted cells. These quoting conventions follow the documented CSV convention in [RFC 4180](https://www.rfc-editor.org/info/rfc4180/). CSV itself supplies no type system.

Every non-null **data** cell starts with its visible alphabetic type prefix. The following are decoded field values after CSV quoting is removed:

| Cell | Meaning |
|---|---|
| `text:000042` | Exact account text `000042`, including leading zeros |
| `text:=SUM(A1)` | Literal text `=SUM(A1)` behind the visible prefix |
| `text:  ＝１` | Original whitespace and fullwidth characters retained after `text:` |
| `decimal:-0.10000001` | Exact decimal string; not a floating-point or formula cell |
| `date:2024-02-29` | Calendar date string, no inferred time zone |
| `uint:42` | Canonical unsigned integer encoded as tagged text |
| `json:{"kind":"csv_row","evidence_id":"…","row":2}` | Complete structured source anchor |
| `json:[]` | Empty duplicate-candidate array |
| `null` | Missing nullable value only |
| `text:null` | Present string `null` |
| `text:` | Present empty string |

The prefix is presentation data, not a formula, apostrophe escape or invisible character. A decoder removes exactly the declared prefix once, or interprets the exact unprefixed `null` token only for nullable columns. Amounts, dates and IDs remain text in this format. Do not strip the prefixes and then open untrusted values as spreadsheet formulas.

The response includes the full versioned `dictionary`, with ordered column names, logical types, prefixes, nullability, meanings and limitations. Columns are fixed in this order:

| Column | Prefix | Nullable | Preserved value |
|---|---|---:|---|
| workspace_revision | uint: | no | Snapshot revision |
| id | text: | no | Canonical transaction ID |
| version | uint: | no | Canonical row version |
| account | text: | no | Exact account string |
| date | date: | no | Transaction date |
| posting_date | date: | yes | Posting date when present |
| description | text: | no | Original description |
| amount | decimal: | no | Exact signed canonical decimal |
| currency | text: | no | Canonical currency |
| balance | decimal: | yes | Exact available balance |
| anchor | json: | no | Full canonical SourceAnchor |
| review | text: | no | pending / accepted / rejected / deferred |
| duplicate_candidates | json: | no | All candidate IDs, unchanged |
| transfer_peer | text: | yes | Existing peer ID |
| merchant | text: | yes | Existing merchant value |

Literal controls other than TAB, CR and LF are refused. Structured JSON keeps its JSON escapes (for example a NUL within an anchor string is `\u0000`); any unsupported control that remains a literal character in that JSON cell is also refused. This refusal does not rewrite the canonical transaction. Raw JSON remains available for unsupported literal values.

This design reduces formula interpretation by making all data cells start with fixed alphabetic tags, including values beginning with whitespace, `=`, `+`, `-`, `@` or fullwidth variants. It is **not a universal spreadsheet-safety guarantee**. No Excel or LibreOffice execution, save/reopen or importer behavior was tested. OWASP describes how spreadsheet parsing and later save/reopen can defeat common CSV escapes, and cautions that no single sanitization works for all consumers. [OWASP CSV Injection](https://community.owasp.org/attacks/CSV_Injection)

Keeping the visible tags also avoids relying on automatic numeric/date conversion. Microsoft documents that Excel can remove leading zeros and lose precision beyond 15 significant digits. Consumers needing ordinary numeric cells must deliberately parse the typed raw JSON or the declared CSV literals using exact decimal handling. [Microsoft: Keeping leading zeros and large numbers](https://support.microsoft.com/en-us/excel/keeping-leading-zeros-and-large-numbers)

## Three different identities

- `selection_sha256` is **exactly** the existing complete JSON export's `query_sha256`: revision, normalized literal query, filter, order and matching profile. It is independent of output format and of the explicit CSV non-accepted admission policy. That policy is echoed in `request`; it does not change the selected rows.
- `format_sha256` hashes UTF-8 compact sorted-object-key JSON for the two-element array `["typed_literal_v1", dictionary]`. Arrays keep their declared order. Dictionary strings are fixed ASCII in v1. This is a format/dictionary identity, not an artifact or selection hash.
- `sha256` hashes the exact UTF-8 CSV artifact, including BOM, all quoting and CRLF bytes. `bytes` counts those exact bytes and `row_count` excludes the header. `format` and `schema_version` are explicit.

Re-encoding line endings, dropping the BOM, changing quotes or removing tags changes the artifact hash. Response JSON escaping is transport encoding, not the exported CSV bytes. Historical raw JSON serialization and hashes remain unchanged; old schemas are not rewritten.

## Limits and verification

CSV artifact size is capped at 256 MiB. An exact preflight counts BOM, headers, prefixes, quotes, doubled quotes, commas, JSON cells and CRLF before allocating the final artifact. A second bounded writer independently enforces the cap during serialization; final length must equal the preflight. It visits one owned cell at a time rather than constructing a second complete table. A failed preflight/serialization returns no partial CSV artifact.

This cap is **not** a total memory, query duration or serialized IPC-body bound. The shared selector still scans canonical records and retains/sorts the complete selected vector; original verification also consumes resources. CSV text, response JSON and frontend/native delivery can introduce additional copies. No 100,000-row CSV performance claim or GUI responsiveness claim is made by this increment.

Rust tests cover complete 305-row selections, exact positive/negative 29-digit extremes and eight decimal places, Unicode/formula/quote/newline cases, explicit mixed-review policy, null/empty/literal-null, every anchor variant, capped preflight, control refusal, stale/malformed/substituted sources, unchanged reports/originals and an actual concurrent correction after snapshot capture.

The independent reader uses Python's standard `csv.reader`, reads actual Rust command responses, decodes the dictionary and compares every value/order with canonical raw JSON. An optional prior signed-source binary proves old/new raw export equality on the exact same synthetic workspace. It retains failures and source/binary/fixture hashes, and uses explicit checks that remain enabled under optimized Python. It does not run a spreadsheet application.

```sh
cargo test -p workbench-core --lib transaction_csv --locked --offline
cargo build -p workbench-core --bin ew-dev --locked --offline
python3 -m unittest discover -s scripts/tests -p test_transaction_csv.py -v
python3 -O -m unittest discover -s scripts/tests -p test_transaction_csv.py -v
python3 scripts/verify_transaction_csv.py --output artifacts/typed-csv/new-observation
```

Use a fresh output directory. The runner's fixed small responses are captured before a post-capture size sanity check; that harness check is not an OS pipe-memory quota. The production CSV writer has its own byte cap. Actual observations and compatibility boundaries are recorded in [typed-csv.json](verification/typed-csv.json).
