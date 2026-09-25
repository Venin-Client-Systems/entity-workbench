# Canonical workspace and report payload diagnostic

An actual `Command::View` response grew from **83,986,629 bytes** to
**198,449,390 bytes** after one HTML report had been saved in the retained
100,000-transaction workspace. The increase is **114,462,761 bytes** (about 2.36
times the original response). `WorkspaceView.reports` includes full historical
HTML, so a saved snapshot is serialized again on an ordinary whole-workspace
refresh. This is a concrete transport-volume finding; perceived UI latency and
memory amplification in Tauri/JavaScript remain unmeasured.

This read-only diagnostic is separate from the
[original timing campaign](OBSERVATION-2026-09-24.md). It did not import or review
records, create a new export, load the HTML in a browser, or change the timing
observation. The original report still has SHA-256
`3c16f2e415b55a5e83df3ee2c849069b7ef65ecd7b75df1b0f6e80562a81e3ab`.

The [full diagnostic evidence](observations/2026-09-24-macos-arm64-payload.json) has
SHA-256 `2c65faa775d73808f332aaa5a8e6463bcf491ca13b1aeeb2349b54d7716d6141`.
It binds source base `da532e5`, the working diagnostic source hashes, rebuilt
release executable, scripts, original fixture and exact saved HTML. It retains
both canonical responses as per-field size observations rather than giant raw
JSON files. `timing_claim` is false: build/child resource logs are diagnostic
provenance and are not new performance samples. The original measured example is
preserved in signed commit `da532e5` before this additive diagnostic mode.

## Exact serialized field sizes

The counter uses `serde_json::to_writer` with a byte-counting writer, matching the
actual serializer's UTF-8 and escape rules. It avoids allocating another complete
200 MB response buffer. Values below include each field value's delimiters and
escaping, but not that field's name. Exact object key/punctuation overhead is
reported separately, so the partition accounts for every byte.

| Field value | Baseline, zero reports | One saved report |
| --- | ---: | ---: |
| `workspace.transactions` — 100,000 rows | 47,110,803 | 47,110,803 |
| `workspace.decisions` — 95,100 decisions | 25,568,901 | 25,568,901 |
| `workspace.evidence` — one original/text | 5,110,461 | 5,110,461 |
| `workspace.reports` | 2 | 114,462,763 |
| Other workspace values plus key/punctuation overhead | 301 | 301 |
| **Complete `workspace` object** | **77,790,468** | **192,253,229** |
| Top-level `analysis` | 6,196,135 | 6,196,135 |
| Top-level key/punctuation overhead | 26 | 26 |
| **Complete View response** | **83,986,629** | **198,449,390** |

Evidence text itself contains 5,010,041 raw UTF-8 bytes and consumes 5,110,044
bytes as an escaped JSON string. The one report's HTML contains 113,592,530 raw
bytes and consumes 114,462,557 bytes as a JSON string. Report metadata and array
punctuation add the remaining 206 bytes of the reports-array value. The baseline
reports array is the two bytes `[]`; the workspace revision moves from 95,101 to
95,102 after snapshot publication, retaining the same digit count. These details
explain the exact response delta without relying on an estimated compression ratio.

## Exact HTML section sizes

The saved artifact's SHA-256 is
`3be44e386ec0dea70c7ccf715f48ce23daf93b7fd8df06977507ccf5bc158f0d`.
Its bytes match the canonical snapshot. The diagnostic partitions the generated
HTML at its literal `<h2>` boundaries; imported text is escaped and cannot create
a heading delimiter. It never executes or renders collected content.

| Generated section | Raw UTF-8 bytes |
| --- | ---: |
| Transaction exhibit | 60,071,871 |
| Reviewed calculations and review-coverage source links | 29,216,300 |
| Review history | 19,292,323 |
| Evidence register | 5,010,478 |
| All remaining markup/sections | 1,558 |
| **Complete HTML** | **113,592,530** |

Every source transaction and review decision remains retained; this diagnostic
does not propose deleting audit history or dropping drillthrough. A narrower
workspace summary with explicit detail retrieval is a follow-up design question.
Any paging implementation must preserve complete totals, review denominators,
source versions, immutable report bytes and stale-revision checks.

## Reproduce against the retained campaign

```sh
python3 scripts/transaction_payload_diagnostic.py \
  --run 20260924T142828-b16b36cb71404e8aba02a732e4225443
```

The diagnostic builds the release example, then opens only the retained baseline
and first-export workspaces. Each child has a 60-second whole-process timeout;
the serializer refuses values above 256 MiB. It requires exactly 100,000 rows,
the frozen original identity and zero/one reports respectively. The HTML file has
a 256 MiB read limit and must match the canonical digest. These input/output
bounds are not a hard RSS limit. Generated diagnostic JSON and logs go into a new
UUID-named subdirectory of the original ignored run directory. Failed observations
are retained. The old timing report is checked unchanged at the end.

The retained workspaces are local artifacts and are deliberately not committed.
On another machine, run the documented baseline first, then substitute its new
run identifier. The logical contents and fixture are repeatable; snapshot UUIDs,
timestamps, output hashes and JSON byte counts containing timestamp precision can
differ between independently generated workspaces.
