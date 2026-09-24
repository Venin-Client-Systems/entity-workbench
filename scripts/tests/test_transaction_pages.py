import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
spec = importlib.util.spec_from_file_location("page_benchmark", SCRIPTS / "transaction_page_benchmark.py")
benchmark = importlib.util.module_from_spec(spec)
spec.loader.exec_module(benchmark)


class PageBenchmarkTests(unittest.TestCase):
    def test_summary_keeps_first_call_and_uses_twenty_warm_samples(self):
        events = [{"event": "sample", "operation": "all_first", "sample": index,
                   "elapsed_ms": float(index + 1), "oracle_passed": True} for index in range(21)]
        events.append({"event": "complete", "operation": "all_first", "samples": 21})
        result = benchmark.summarize({"events": events, "returncode": 0}, "all_first")
        self.assertEqual(result["outcome"], "measured")
        self.assertEqual(result["first_call_ms"], 1)
        self.assertEqual(result["warm_p50_nearest_rank_ms"], 11)
        self.assertEqual(result["warm_p95_nearest_rank_ms"], 20)
        self.assertFalse(result["p95_qualified"])
        self.assertFalse(result["under_two_seconds_gate_passed"])
        result = benchmark.summarize({"events": events[:-2], "returncode": -9}, "all_first")
        self.assertEqual(result["outcome"], "failed")
        self.assertEqual(len(result["samples"]), 20)
        events[1]["elapsed_ms"] = "malformed"
        result = benchmark.summarize({"events": events, "returncode": 0}, "all_first")
        self.assertEqual(result["outcome"], "failed")
        self.assertIsNone(result["warm_p50_nearest_rank_ms"])

    def test_copy_never_overwrites_source_or_existing_destination(self):
        with tempfile.TemporaryDirectory() as temporary:
            source, target = Path(temporary) / "source", Path(temporary) / "copy"
            (source / "originals").mkdir(parents=True)
            (source / "workspace.db").write_bytes(b"synthetic database")
            original = source / "originals" / benchmark.FIXTURE
            original.write_bytes(b"synthetic original")
            benchmark.copy_workspace(source, target)
            self.assertEqual((target / "workspace.db").read_bytes(), b"synthetic database")
            self.assertEqual(original.read_bytes(), b"synthetic original")
            with self.assertRaises(FileExistsError):
                benchmark.copy_workspace(source, target)

    def test_source_with_live_sidecar_is_rejected_before_copy(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            (source / "workspace.db-wal").write_bytes(b"synthetic outstanding journal")
            with self.assertRaisesRegex(ValueError, "closed and checkpointed"):
                benchmark.source_identity(source)

    def test_metadata_tool_failure_retains_report_before_source_preflight(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with patch.object(benchmark, "ROOT", root), \
                 patch.object(benchmark.baseline, "host", return_value={}), \
                 patch.object(benchmark.baseline, "capture", side_effect=OSError("Synthetic metadata failure")):
                self.assertEqual(benchmark.run(root / "missing"), 1)
            reports = list(root.glob("artifacts/transaction-pages/*/report.json"))
            self.assertEqual(len(reports), 1)
            value = json.loads(reports[0].read_text())
            self.assertEqual(value["outcome"], "failed")
            self.assertEqual(value["phase"], "metadata")
            self.assertEqual(value["failure"], "Synthetic metadata failure")
            self.assertTrue(all(v["outcome"] == "not_run" for v in value["operations"].values()))

    def test_failed_preflight_retains_distinct_observations(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with patch.object(benchmark, "ROOT", root), \
                 patch.object(benchmark.baseline, "host", return_value={}), \
                 patch.object(benchmark.baseline, "capture", return_value="synthetic"), \
                 patch.object(benchmark.baseline, "sources", return_value={}):
                self.assertEqual(benchmark.run(root / "missing"), 1)
                self.assertEqual(benchmark.run(root / "missing"), 1)
            reports = list(root.glob("artifacts/transaction-pages/*/report.json"))
            self.assertEqual(len(reports), 2)
            for report in reports:
                value = json.loads(report.read_text())
                self.assertEqual(value["outcome"], "failed")
                self.assertEqual(value["phase"], "preflight")
                self.assertTrue(all(v["outcome"] == "not_run" for v in value["operations"].values()))


if __name__ == "__main__":
    unittest.main()
