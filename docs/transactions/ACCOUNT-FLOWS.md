# Revision-bound reviewed account flows

This backend calculates internal account relationships from the existing canonical transaction ledger. It is a read-only API; this increment adds no graph UI, automatic matching, new observations, merchant attribution or external counterparty inference. The complete EW-18 workflow and release gates remain unfinished.

`analyze_account_flows` takes an explicitly supplied `expected_revision` and a request containing nullable `date_from`, `date_to`, `account` and `currency`. Dates are canonical `YYYY-MM-DD`, inclusive and based on transaction dates. Account and currency are exact labels. There is no review-state selector: all selected review states remain visible in the denominators. An absent limit means the corresponding scope is unrestricted, not an automatically chosen period.

The command is additive in `command.v25.schema.json`. `account-flow-request.v1.schema.json` and `account-flows.v1.schema.json` describe the new DTOs. Prior command and result schema files remain byte-identical. No storage migration or saved record rewrite occurs.

Example request for a synthetic workspace at revision 42:

```json
{
  "action": "analyze_account_flows",
  "request": {
    "date_from": "2024-01-01",
    "date_to": "2024-01-31",
    "account": "0001",
    "currency": "AUD"
  },
  "expected_revision": 42
}
```

## What forms a flow

The existing `analytics::verified_transfer_peer` rule is the sole matching authority. Both source transactions must be accepted, refer reciprocally to one another, have distinct transaction IDs and exact account labels, share one currency, and have nonzero exactly equal-and-opposite amounts. Similar dates, merchant descriptions, refunds, equal amounts alone, one-sided markers and duplicate candidates cannot create an edge.

A verified pair is included when **either** endpoint is in the selected scope. It is counted once, directed from its negative debit account to its positive credit account. Edges aggregate pairs with the same currency and directed account labels. Each pair retains both transaction IDs and versions, both dates, exact positive amount and separate `debit_in_scope` / `credit_in_scope` flags. Repeated legitimate pairs remain separate contributions; nothing is silently deduplicated.

For example, a reviewed debit of `-10.00` on January 31 in account `0001` and a reciprocal credit of `10.00` on February 1 in account `0002` form one `10.00` edge. Selecting only account `0001` in January still includes that pair, but the credit is explicitly supporting evidence outside the scope. Selecting only account `0002` in February reverses which endpoint is selected, without reversing the debit-to-credit direction.

Edge amounts are positive once-per-pair totals. They are not a net balance, a sum of selected ledger rows, total spending, or proof that funds ultimately reached an external person. Summing edges with asymmetric scopes includes the counterpart needed to explain the selected endpoint; it does not turn the counterpart into selected activity. Currencies never mix and no FX conversion occurs.

## Nodes, denominators and sources

Each selected row belongs to exactly one node identified by its exact account and currency. A node contains:

- `accepted_ledger`: exact credits, absolute debits, signed net and contributing IDs for all accepted selected rows, including mapped transfers.
- `accepted_unmapped`: the same calculations for accepted selected rows without a verified transfer counterpart. Refunds and unverified markers remain here.
- Full accepted/pending/rejected/deferred counts, with IDs for each unaccepted state. These form a disjoint partition of `scope_count`.
- `unverified_transfer_ids` and `duplicate_candidate_ids`, with explicit counts, for selected rows in any review state. These are overlapping annotations, not additional partitions or automatic exclusions.
- `support_ids` for out-of-scope counterparts. A `support_only` node has zero scope count, zero selected review counts and zero selected totals.

The complete `sources` catalogue contains each selected row once and each necessary out-of-scope verified peer once. It retains ID/version, account/currency, transaction date, original decimal string, review state, scope flag, canonical anchor and transfer/duplicate annotations. Full descriptions and other canonical fields are available through the existing bounded `read_transaction_sources` API using the result revision and exact expected row version. No blank or fabricated row substitutes are returned.

The workspace reader verifies original bytes for every distinct selected source and recorded counterpart source, including out-of-scope counterparts. This is conservative: even an ultimately unverified marker can require its referenced original to be intact. Canonical Evidence key/body/digest identity rules remain in force. Anchors are bounded retained provenance references; their location and quote still require source inspection. They do not become accepted coordinates, new findings, or a new immutable extraction derivative.

## Exactness, ordering and limits

One SQLite read transaction pins revision, input-size checks, canonical rows and the returned result. An expected-revision conflict fails before loading the ledger. Every retained transaction key must equal its body ID, and versions must be positive. The shared analytical lookup validates money, dates, currency and unique IDs; extraction of this helper preserves the previous analysis/comparison validation order and behavior without running recurrence calculations for account flows.

All monetary arithmetic uses the existing exact decimal parser and checked exact addition/subtraction. A representation overflow or precision loss fails the whole request. Source amount spelling is retained separately from calculated totals. No rounding is silently accepted.

Ordering is deterministic: nodes by `(account, currency)`, edges by `(currency, debit account, credit account)`, sources and per-node ID lists by transaction ID, and pairs by `(debit date, credit date, debit ID, credit ID)`. IDs are SHA-256 hashes of domain-tagged compact JSON tuples: node account/currency, directed edge currency/accounts, or pair debit/credit transaction IDs. Versions remain explicit payload data rather than replacing identity after every review.

The reader refuses the complete request on any exceeded limit:

| Limit | Bound |
| --- | ---: |
| Whole retained ledger | 100,000 rows |
| Each retained transaction body | 2 MiB |
| Whole retained transaction bodies | 64 MiB |
| Shared aggregate description bytes | 32 MiB |
| Transaction/peer/evidence identifier | 1–256 bytes, no controls |
| Positive source version | `u32` |
| Account label | 4,000 bytes |
| Each serialized retained anchor | 8 KiB |
| Account/currency nodes | 1,000 |
| Directed account/currency edges | 10,000 |
| Verified pairs | 50,000 |
| Complete compact JSON result | 16 MiB |

The output limit uses a capped counting writer that accounts for actual JSON escaping without first allocating a giant encoded string. No partial graph or silently truncated row list is returned. Input body sizes and identities are projected before Rust copies those bodies; SQLite itself still scans/parses the ledger. The calculation holds canonical rows and result structures in memory. These bounds are not a hard RSS guarantee or an indexed performance claim. Narrowing the output scope does not conceal malformed/oversized records elsewhere in the complete analytical input.

## Verification and remaining integration

Synthetic tests independently calculate repeated transfers, ordinary repeated purchases, refunds, near matches, zero amounts, multiple currencies, unaccepted rows and asymmetric scopes. They check exact source drillthrough, deterministic order under input permutation, leap-day boundaries, overflow refusal, original tampering on either side, valid-row substitution under the wrong canonical key, Evidence retargeting and all-or-error size limits. A normal canonical correction unlinks the old pair, makes the corrected row pending and invalidates old-revision reads while retaining previous report JSON and HTML bytes.

The command is usable through existing canonical dispatch, but no UI or native graph interaction has been implemented or verified in this increment. Existing Figma account-flow states, accessibility, in-app drillthrough and complete release validation remain future integration work.
