"""Exact DuckDB aggregation for the development Parquet adapter.

Rust remains responsible for selecting the canonical snapshot and validating
returned rows. This adapter cannot infer whether a transfer is an internal pair.
It rejects those rows until the coordinator supplies a reviewed selection.
"""

from collections import defaultdict
from decimal import Decimal
import re

MAX_ROWS = 100_000
MAX_EXPANDED_BYTES = 128 * 1024 * 1024
MAX_SCALE = 28
AMOUNT = re.compile(r"-?[0-9]+(?:\.[0-9]+)?\Z", re.ASCII)
CURRENCY = re.compile(r"[A-Z]{3}\Z", re.ASCII)
COLUMNS = {"id", "amount", "currency", "review", "transfer_peer"}
FIELD_LIMITS = {"id": 128, "amount": 64, "currency": 3, "review": 8, "transfer_peer": 128}


def checked_rows(source):
    """Check declared bounds, then bound strings before materializing each batch.

    Encoded Parquet metadata is not an RSS limit. Keep dictionary strings
    encoded until their lengths are checked, so repetition cannot bypass the
    field limits by expanding oversized values into many Python strings.
    """
    import pyarrow as pa
    import pyarrow.compute as pc
    import pyarrow.parquet as pq

    with pq.ParquetFile(source, read_dictionary=sorted(COLUMNS)) as parquet:
        schema = parquet.schema_arrow
        if len(schema) != len(COLUMNS) or set(schema.names) != COLUMNS:
            raise ValueError("Unexpected transaction columns")
        for field in schema:
            value_type = field.type.value_type if pa.types.is_dictionary(field.type) else field.type
            if not pa.types.is_string(value_type) and not (
                field.name == "transfer_peer" and pa.types.is_null(field.type)
            ):
                raise ValueError("Transaction fields must preserve textual values")
        metadata = parquet.metadata
        expanded = sum(
            metadata.row_group(index).total_byte_size
            for index in range(metadata.num_row_groups)
        )
        if metadata.num_rows > MAX_ROWS or expanded > MAX_EXPANDED_BYTES:
            raise ValueError("Transaction table exceeds adapter bounds")
        rows = []
        for batch in parquet.iter_batches(batch_size=4096, use_threads=False):
            if len(rows) + batch.num_rows > MAX_ROWS:
                raise ValueError("Transaction row count exceeds adapter bounds")
            for field, column in zip(batch.schema, batch.columns):
                values = column.dictionary if pa.types.is_dictionary(column.type) else column
                if pa.types.is_null(values.type):
                    continue
                longest = pc.max(pc.utf8_length(values)).as_py()
                if longest is not None and longest > FIELD_LIMITS[field.name]:
                    raise ValueError("Transaction field length bound exceeded")
            rows.extend(batch.to_pylist())

    seen = set()
    for row in rows:
        key = row["id"]
        if not isinstance(key, str) or not key or len(key) > 128 or key in seen:
            raise ValueError("Transaction IDs must be unique nonempty strings")
        seen.add(key)
        amount = row["amount"]
        if not isinstance(amount, str) or len(amount) > 64 or not AMOUNT.fullmatch(amount):
            raise ValueError("Amount must be an exact plain decimal string")
        scale = len(amount.split(".", 1)[1]) if "." in amount else 0
        if scale > MAX_SCALE:
            raise ValueError("Amount exceeds the supported exact decimal scale")
        currency = row["currency"]
        if not isinstance(currency, str) or not CURRENCY.fullmatch(currency):
            raise ValueError("Invalid transaction currency")
        if row["review"] not in {"accepted", "pending", "rejected", "deferred"}:
            raise ValueError("Unknown transaction review state")
        peer = row["transfer_peer"]
        if peer is not None and (not isinstance(peer, str) or not peer or len(peer) > 128):
            raise ValueError("Invalid transfer counterpart")
        if row["review"] == "accepted" and peer is not None:
            raise ValueError("Transfer selection requires canonical pair validation")
    return rows


def transaction_totals(source):
    """Choose a lossless Decimal128 scale per currency, then aggregate in DuckDB.

    The eight-place minimum preserves the prototype result format. Additional
    fractional digits remain exact, up to scale 28. Values or totals that cannot
    fit Decimal128 fail; neither binary floats nor rounding are permitted.
    """
    import duckdb
    import pyarrow as pa

    groups = defaultdict(list)
    for row in checked_rows(source):
        if row["review"] == "accepted":
            groups[row["currency"]].append(row)
    totals = []
    with duckdb.connect(":memory:") as connection:
        connection.execute("SET enable_external_access=false")
        for currency, rows in sorted(groups.items()):
            amounts = [Decimal(row["amount"]) for row in rows]
            scale = max(8, max(-amount.as_tuple().exponent for amount in amounts))
            integral_digits = max(max(1, amount.adjusted() + 1) for amount in amounts)
            if integral_digits + scale > 38:
                raise ValueError("Currency values cannot share an exact Decimal128 scale")
            table = pa.table({
                "id": pa.array([row["id"] for row in rows], type=pa.string()),
                "amount": pa.array(amounts, type=pa.decimal128(38, scale)),
            })
            connection.register("transactions", table)
            net, identifiers = connection.execute(
                "SELECT sum(amount)::VARCHAR, list(id ORDER BY id) FROM transactions"
            ).fetchone()
            connection.unregister("transactions")
            # DuckDB's SUM accumulator may return 39 decimal digits before its
            # underlying signed 128-bit overflow check fires. Enforce the
            # declared precision as well as relying on the engine's hard bound.
            if max(1, Decimal(net).adjusted() + 1) + scale > 38:
                raise ValueError("Currency total exceeds exact Decimal128 precision")
            totals.append({"currency": currency, "net": net, "transaction_ids": identifiers})
    return {"totals": totals}
