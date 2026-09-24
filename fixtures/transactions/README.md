# Transaction-pattern fixture

All entries are fictional. `patterns.csv` has 20 data rows. Row numbers below include the header; IDs are `<original SHA-256>:<row>` and the amount-cell anchor stays attached to that row.

For the review specimen, accept all rows except source row 12 (pending), row 13 (rejected) and row 14 (deferred). Explicitly match source rows 15 and 16 as a reviewed transfer. The two café purchases on row 5 and row 6 are legitimate repeated synthetic purchases and remain counted even when duplicate detection flags them.

Independently specified expected results:

| Scope / rule | Expected |
|---|---|
| All source rows | 20: AUD 17, USD 3 |
| AUD reviewed, transfers included | 14 rows; credits 125.00; debits 127.00; net -2.00 |
| AUD reviewed, reviewed transfer pair explicitly excluded | 12 included rows; credits 105.00; debits 107.00; net -2.00; two excluded rows still in denominator |
| USD reviewed | Three rows; debits 36.00; net -36.00; never summed into AUD |
| AUD ACME CLUB description group | Four rows across accounts 0001 and 0002; debits 40.00; merchant identity unconfirmed |
| AUD café description group | Three rows; debits 12.00; two same-day purchases remain counted; no recurring candidate |
| Cash candidate | Row 8 only; debit 50.00; exact ATM token heuristic |
| Refund/reversal candidates | Rows 9 and 10; credits 5.00; row 18's debit with REFUND is not a credit candidate |
| Default recurring candidates | Account 0001: AUD rows 2–4 (30.00) and USD rows 19–21 (36.00), both weekly; separate currencies and accounts |
| Accepted debit recurrence denominator | 13 rows with transfers included; six recurring candidate rows, seven unclassified rows |

These rules exercise classification and exact calculation. They do not establish merchant identity, subscription intent, cash use, a refund/purchase match, native installation or benchmark performance.

`comparison.csv` is a separate twelve-row synthetic period-comparison fixture. The UI tests accept every row except row index 3 (pending), 6 (rejected), and 7 (deferred), then pair indexes 8 and 9 as a reviewed transfer. January 2025 has six source rows, February has four, and March has two outside both periods. Account 0001 / AUD has January credits 100.00, debits 20.00, net 80.00 (three included and one pending) and February credits 120.00, debits 30.00, net 90.00 (two included, one rejected, one deferred). Duplicate-looking January purchases both remain counted. USD and accounts 0002/0003 remain separate. The literal HTML-looking descriptions are inert test strings and must remain escaped in every source surface.
