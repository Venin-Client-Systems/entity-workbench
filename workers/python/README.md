# Development analytical adapters

These are engine experiments, not an enabled desktop worker runtime. The native
coordinator does not yet create their analytical snapshots or publish their
results. The current development request envelope is not the canonical worker
protocol. Packaging, protocol integration, operating-system confinement and
Rust-side result validation remain required before application use.

Run the pinned development environment's suite with:

```sh
uv sync --frozen --project workers/python
uv run --frozen --project workers/python pytest workers/python
```

## Exact transaction totals

`transaction_totals` reads one Parquet file with exactly five columns: textual
`id`, `amount`, `currency`, `review` and nullable textual `transfer_peer`.
Only accepted rows contribute. Pending, rejected and deferred rows stay outside the
aggregate. IDs must be unique; identical purchases with different IDs remain
separate source rows. Results retain sorted contributing IDs and separate
currency totals.

Amounts use plain decimal strings, up to 28 fractional places. Each currency is
converted to an Arrow `Decimal128(38, scale)` column without an intermediate
float or a rounding cast. The scale is at least eight to preserve the prototype
result format. Additional fractional digits are preserved. If values of very
different magnitudes cannot share a lossless 38-digit type, or the aggregate
exceeds that precision, the entire operation fails. No partial aggregate is
published. The adapter admits at most 100,000 rows and 128 MiB of declared
uncompressed Parquet row-group data; native memory and time enforcement is still
required because file metadata is untrusted. That declared byte count is not an
Arrow/Python memory bound: dictionary repetition can expand much further. The
reader retains dictionary encoding while checking string lengths, then converts
at most 4,096 bounded rows at a time into Python objects. A regression rejects
oversized repeated dictionary strings before that conversion. These checks do
not replace the missing native memory boundary.

An accepted row with a transfer pointer fails explicitly. A pointer alone cannot
establish that two accepted, same-currency, equal/opposite rows are a reviewed
internal pair. The Rust domain's transfer treatment remains authoritative; a
future snapshot contract must supply its validated inclusion decision. This
adapter does not independently choose which transfers to remove.

The regression suite covers precision beyond eight decimal places, separate
currency scales, incompatible magnitudes, aggregate precision overflow, malformed
amounts, review treatment, duplicate IDs, unresolved transfers, schema drift and
declared expansion bounds. These are local development checks, not packaged
runtime or financial-workflow acceptance.
