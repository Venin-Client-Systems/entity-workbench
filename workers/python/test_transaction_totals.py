from decimal import localcontext

import pyarrow as pa
import pyarrow.parquet as pq
import pytest

from transaction_totals import transaction_totals


def write_rows(tmp_path, amounts, *, currencies=None, reviews=None, peers=None, ids=None):
    size = len(amounts)
    table = pa.table({
        "id": pa.array(ids if ids is not None else [f"row-{i}" for i in range(size)], pa.string()),
        "amount": pa.array(amounts, pa.string()),
        "currency": pa.array(currencies if currencies is not None else ["AUD"] * size, pa.string()),
        "review": pa.array(reviews if reviews is not None else ["accepted"] * size, pa.string()),
        "transfer_peer": pa.array(peers if peers is not None else [None] * size, pa.string()),
    })
    path = tmp_path / "transactions.parquet"
    pq.write_table(table, path)
    return path


def test_fractional_digits_survive_aggregation_without_decimal_context_rounding(tmp_path):
    source = write_rows(tmp_path, ["1.123456789", "0.000000001", "-0.000000002"])
    with localcontext() as context:
        context.prec = 2
        result = transaction_totals(source)
    assert result == {"totals": [{
        "currency": "AUD", "net": "1.123456788", "transaction_ids": ["row-0", "row-1", "row-2"],
    }]}


def test_maximum_supported_scale_and_separate_currency_types(tmp_path):
    source = write_rows(tmp_path, ["0.0000000000000000000000000001", "999999999999999999999999999999"], currencies=["AUD", "USD"])
    assert [row["net"] for row in transaction_totals(source)["totals"]] == [
        "0.0000000000000000000000000001", "999999999999999999999999999999.00000000",
    ]


def test_incompatible_magnitudes_fail_without_rounding(tmp_path):
    source = write_rows(tmp_path, ["0.0000000000000000000000000001", "999999999999999999999999999999"])
    with pytest.raises(ValueError, match="exact Decimal128 scale"):
        transaction_totals(source)


def test_sum_precision_is_checked_even_before_native_integer_overflow(tmp_path):
    source = write_rows(tmp_path, ["999999999999999999999999999999.99999999", "0.00000001"])
    with pytest.raises(ValueError, match="total exceeds exact Decimal128 precision"):
        transaction_totals(source)


@pytest.mark.parametrize("amount", ["NaN", "Infinity", "1e-9", "1.0 ", "+1.0", "0.00000000000000000000000000001", None])
def test_unsupported_amounts_fail_instead_of_coercing(tmp_path, amount):
    with pytest.raises(ValueError):
        transaction_totals(write_rows(tmp_path, [amount]))


def test_all_review_states_have_explicit_treatment_and_empty_is_empty(tmp_path):
    source = write_rows(tmp_path, ["1", "100", "1000", "1", "10000"], reviews=["accepted", "pending", "rejected", "accepted", "deferred"])
    assert transaction_totals(source)["totals"][0] == {
        "currency": "AUD", "net": "2.00000000", "transaction_ids": ["row-0", "row-3"],
    }
    assert transaction_totals(write_rows(tmp_path, [])) == {"totals": []}


def test_transfer_pointer_alone_cannot_silently_remove_money(tmp_path):
    with pytest.raises(ValueError, match="canonical pair validation"):
        transaction_totals(write_rows(tmp_path, ["-1"], peers=["unverified-other-row"]))


@pytest.mark.parametrize("kwargs", [{"ids": ["same", "same"]}, {"reviews": ["accepted", "unknown"]}, {"currencies": ["AUD", "aud"]}])
def test_invalid_rows_fail_before_aggregation(tmp_path, kwargs):
    with pytest.raises(ValueError):
        transaction_totals(write_rows(tmp_path, ["1", "2"], **kwargs))


def test_schema_drift_and_numeric_amount_columns_are_not_coerced(tmp_path):
    source = write_rows(tmp_path, ["1"])
    table = pq.read_table(source)
    pq.write_table(table.append_column("unexpected", pa.array(["x"])), source)
    with pytest.raises(ValueError, match="Unexpected transaction columns"):
        transaction_totals(source)
    table = table.set_column(1, "amount", pa.array([1.0]))
    pq.write_table(table, source)
    with pytest.raises(ValueError, match="textual values"):
        transaction_totals(source)


def test_parquet_expansion_bounds_are_checked_before_read(tmp_path, monkeypatch):
    import transaction_totals as adapter
    source = write_rows(tmp_path, ["1", "2"])
    monkeypatch.setattr(adapter, "MAX_ROWS", 1)
    with pytest.raises(ValueError, match="adapter bounds"):
        transaction_totals(source)
    monkeypatch.setattr(adapter, "MAX_ROWS", 2)
    monkeypatch.setattr(adapter, "MAX_EXPANDED_BYTES", 1)
    with pytest.raises(ValueError, match="adapter bounds"):
        transaction_totals(source)


def test_dictionary_repetition_cannot_expand_oversized_strings_into_python_rows(tmp_path):
    source = write_rows(tmp_path, ["1"] * 5000, peers=["x" * 1024] * 5000)
    metadata = pq.ParquetFile(source).metadata
    assert metadata.row_group(0).total_byte_size < 128 * 1024
    with pytest.raises(ValueError, match="field length bound"):
        transaction_totals(source)
