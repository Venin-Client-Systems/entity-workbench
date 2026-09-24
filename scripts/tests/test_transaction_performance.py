"""The evidence reducer cannot promote missing, failed or malformed runs to measured."""
import importlib.util
import math
import sys
from pathlib import Path
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("transaction_performance", Path(__file__).resolve().parents[1] / "transaction_performance.py")
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


def records():
    return [{"event": "sample", "operation": "patterns_all", "sample": i,
             "elapsed_ms": value, "oracle_passed": True}
            for i, value in enumerate((30, 10, 20, 15))] + [
                {"event": "complete", "operation": "patterns_all", "samples": 4}]


class EvidenceTests(unittest.TestCase):
    @unittest.skipUnless(runner.platform.system() in ("Darwin", "Linux"), "Process-group runner is Unix-only")
    def test_actual_timeout_retains_partial_output_without_success(self):
        with tempfile.TemporaryDirectory() as temporary:
            observed = runner.invoke([sys.executable, "-c",
                'import time; print(\'{"event":"started"}\', flush=True); time.sleep(10)'],
                Path(temporary), "synthetic-timeout", .2)
            self.assertTrue(observed["timed_out"])
            self.assertNotEqual(observed["returncode"], 0)
            self.assertEqual(observed["events"], [{"event": "started"}])
            self.assertFalse(runner.summarize(observed["events"], "view")["measurement_complete"])
            self.assertTrue((Path(temporary)/observed["stdout"]).is_file())

    def test_three_samples_descriptive_p95_is_max_never_release_gate(self):
        report = runner.summarize(records(), "patterns_all")
        self.assertTrue(report["measurement_complete"])
        self.assertEqual(report["first_call_ms"], 30)
        self.assertEqual(report["warm_p95_nearest_rank_ms"], 20)
        self.assertFalse(report["p95_qualified"])
        self.assertFalse(report["under_two_seconds_gate_passed"])

    def test_missing_duplicate_failed_and_invalid_samples_never_pass(self):
        good = records()
        bad = [[], good[:-1], good[1:], good + [good[0]],
               good + [{"event": "failure"}], good[:-1]+[dict(good[-1], samples=3)]]
        for value in (math.nan, math.inf, -1, "12", True, None):
            bad.append([dict(good[0], elapsed_ms=value), *good[1:]])
        bad += [[dict(good[0], oracle_passed=False), *good[1:]],
                [dict(good[0], sample=3), *good[1:]]]
        for data in bad:
            with self.subTest(data=data):
                self.assertFalse(runner.summarize(data, "patterns_all")["measurement_complete"])

    def test_partial_jsonl_does_not_manufacture_a_measurement(self):
        self.assertEqual(runner.events('noise\n{"event":"started"}\n{"event":"sample"'),
                         [{"event": "started"}])
        report = runner.summarize(runner.events('{"event":"started"}\n'), "view")
        self.assertIsNone(report["first_call_ms"])
        self.assertIsNone(report["warm_p95_nearest_rank_ms"])

    def test_rss_units_are_platform_explicit_and_missing_is_not_zero(self):
        self.assertEqual(runner.peak_rss("  12345  maximum resident set size\n", "Darwin"), 12345)
        self.assertEqual(runner.peak_rss(" Maximum resident set size (kbytes): 12345\n", "Linux"), 12345*1024)
        self.assertIsNone(runner.peak_rss("killed before resource report", "Darwin"))

    def test_failed_build_is_retained_and_all_queries_remain_not_run(self):
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(runner, "ROOT", Path(temporary)), \
                mock.patch.object(runner, "sources", return_value={"synthetic": "hash"}), \
                mock.patch.object(runner, "capture", return_value="synthetic"), \
                mock.patch.object(runner, "host", return_value={"os": "synthetic"}), \
                mock.patch.object(runner, "invoke", return_value={"returncode": 1, "events": []}), \
                mock.patch("builtins.print"):
            self.assertEqual(runner.run("smoke"), 1)
            import json
            path = next((Path(temporary)/"artifacts/transaction-performance").glob("*/report.json"))
            report = json.loads(path.read_text())
            self.assertEqual(report["outcome"], "failed")
            self.assertTrue(all(v["outcome"] == "not_run" for v in report["operations"].values()))
            self.assertFalse(report["complete_release"])

    def test_partial_canonical_setup_is_retained_without_query_execution(self):
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(runner, "ROOT", Path(temporary)), \
                mock.patch.object(runner, "sources", return_value={"synthetic": "hash"}), \
                mock.patch.object(runner, "capture", return_value="synthetic"), \
                mock.patch.object(runner, "host", return_value={"os": "synthetic"}), \
                mock.patch.object(runner, "digest", return_value="synthetic-hash"), \
                mock.patch.object(runner.platform, "system", return_value="Darwin"), \
                mock.patch.object(runner, "invoke", side_effect=[{"returncode": 0, "events": []},
                    {"returncode": -9, "timed_out": True, "events": [{"event": "progress", "rows": 10000}]}]) as invoke, \
                mock.patch("builtins.print"):
            self.assertEqual(runner.run("baseline"), 1)
            import json
            path = next((Path(temporary)/"artifacts/transaction-performance").glob("*/report.json"))
            report = json.loads(path.read_text())
            self.assertEqual(invoke.call_count, 2)
            self.assertTrue(report["setup"]["timed_out"])
            self.assertEqual(report["setup"]["events"][0]["rows"], 10000)
            self.assertTrue(all(v["outcome"] == "not_run" for v in report["operations"].values()))
            self.assertFalse(report["complete_release"])


if __name__ == "__main__":
    unittest.main()
