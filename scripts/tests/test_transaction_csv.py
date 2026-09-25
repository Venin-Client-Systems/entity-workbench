"""Decoder/failure-retention tests; real Rust interoperability is the separate runner."""
import csv
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import verify_transaction_csv as check


def specimen():
    dictionary = {"null_literal": "null", "columns": [
        {"name": name, "logical_type": kind, "prefix": prefix, "nullable": nullable}
        for name, kind, prefix, nullable in check.COLUMNS]}
    buffer = io.StringIO(newline="")
    writer = csv.writer(buffer, lineterminator="\r\n", quoting=csv.QUOTE_ALL)
    writer.writerow([column[0] for column in check.COLUMNS])
    writer.writerow(["uint:7", "text:row1", "uint:1", "text:000042", "date:2024-02-29", "null",
                     'text:\t＝1\r\n"quoted", 🚲', "decimal:79228162514264337593543950335", "text:AUD",
                     "null", 'json:{"kind":"csv_row","evidence_id":"0001","row":2}',
                     "text:pending", "json:[]", "text:null", "text:"])
    text = "\ufeff" + buffer.getvalue()
    return {"schema_version": 1, "format": "typed_literal_v1", "workspace_revision": 7,
            "dictionary": dictionary, "format_sha256": check.digest(check.compact(["typed_literal_v1", dictionary])),
            "row_count": 1, "bytes": len(text.encode()), "sha256": check.digest(text.encode()), "csv": text}


class CsvVerificationTests(unittest.TestCase):
    def test_independent_decoder_preserves_exact_text_nulls_and_extreme_decimal(self):
        row = check.decode(specimen())[0]
        self.assertEqual(row["account"], "000042")
        self.assertEqual(row["amount"], "79228162514264337593543950335")
        self.assertEqual(row["description"], '\t＝1\r\n"quoted", 🚲')
        self.assertIsNone(row["posting_date"])
        self.assertEqual(row["transfer_peer"], "null")
        self.assertEqual(row["merchant"], "")
        self.assertEqual(row["anchor"]["evidence_id"], "0001")

    def test_format_identity_is_independent_of_dto_object_order(self):
        value = specimen()
        expected = check.decode(value)
        dictionary = dict(reversed(list(value["dictionary"].items())))
        dictionary["columns"] = [dict(reversed(list(column.items())))
                                 for column in dictionary["columns"]]
        value["dictionary"] = dictionary
        self.assertEqual(check.decode(value), expected)
        dictionary["columns"][0]["meaning"] = "changed dictionary content"
        with self.assertRaisesRegex(ValueError, "Format/dictionary identity mismatch"):
            check.decode(value)

    def test_truncated_substituted_or_inconsistent_response_is_not_success(self):
        for mutation in (
            lambda x: x.update(row_count=2),
            lambda x: x.update(workspace_revision=8),
            lambda x: x.update(sha256="0" * 64),
            lambda x: x.update(bytes=x["bytes"] - 1),
            lambda x: x.update(csv=x["csv"].replace("text:000042", "=000042")),
            lambda x: x["dictionary"]["columns"].append(x["dictionary"]["columns"][0]),
            lambda x: x.update(format_sha256="f" * 64),
        ):
            with self.subTest(mutation=mutation):
                value = specimen()
                mutation(value)
                with self.assertRaises(ValueError):
                    check.decode(value)

    def test_correct_digest_cannot_hide_missing_prefix_or_invalid_integer(self):
        for before, after in (("text:000042", "=000042"), ("uint:7", "uint:07"),
                              ("uint:1", "uint:18446744073709551616"), ("text:row1", "null")):
            with self.subTest(after=after):
                value = specimen()
                value["csv"] = value["csv"].replace(before, after)
                value.update(bytes=len(value["csv"].encode()), sha256=check.digest(value["csv"].encode()))
                with self.assertRaises(ValueError):
                    check.decode(value)

    def test_failed_metadata_is_retained_without_assumed_command_result(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "observation"
            with mock.patch.object(check.subprocess, "run", side_effect=OSError("private/path")):
                report = check.run(Path(temp) / "absent", output)
            self.assertEqual(report["outcome"], "failed")
            self.assertEqual(report["command_count"], 0)
            self.assertEqual(report["checks"], [])
            saved = (output / "report.json").read_text()
            self.assertNotIn("private/path", saved)
            self.assertEqual(json.loads(saved), report)

    def test_checks_are_not_removed_under_optimized_python(self):
        with self.assertRaisesRegex(ValueError, "explicit check"):
            check.require(False, "explicit check")


if __name__ == "__main__":
    unittest.main()
