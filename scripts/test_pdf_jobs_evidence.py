"""Rejected/unfinished PDF job verification cannot retain the preceding success projection."""
import json
import datetime
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import test_pdf_jobs as runner


class EvidenceTests(unittest.TestCase):
    def check_failure(self, phase, failure, action):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "fixtures/pdf-render").mkdir(parents=True)
            with (
                patch.object(runner, "ROOT", root),
                patch.object(runner, "SOURCES", []),
                patch.object(runner.platform, "system", return_value="Darwin"),
                patch.object(runner.subprocess, "check_output", return_value="synthetic"),
            ):
                runner.save({"passed": True, "outcome": "passed"}, "previous output")
                with action:
                    report, diagnostics = runner.observe(root)
                runner.save(report, diagnostics)
                latest = json.loads((root / "artifacts/pdf-jobs-result.json").read_text())
                self.assertFalse(latest["passed"])
                self.assertEqual(latest["phase"], phase)
                self.assertEqual(latest["failure"], failure)
                self.assertEqual(len(list((root / "artifacts/pdf-jobs").glob("*.json"))), 2)
                self.assertNotIn("private-path", json.dumps(latest))

    def test_inventory_rejection_replaces_prior_success(self):
        rejection = patch.object(runner.pdf_runner, "inspect_runtime",
                                 side_effect=runner.pdf_runner.VerificationFailure("unexpected_pdf_classpath"))
        self.check_failure("runtime_inventory", "unexpected_pdf_classpath", rejection)

    def test_timeout_replaces_prior_success(self):
        def timeout(_runtime, report):
            report["phase"] = "native_tests"
            raise subprocess.TimeoutExpired(["private-path"], 180)
        with patch.object(runner.pdf_runner, "inspect_runtime", return_value={}):
            action = patch.object(runner, "native_tests", side_effect=timeout)
            self.check_failure("native_tests", "tool_timeout", action)

    def test_incomplete_suite_replaces_prior_success(self):
        def incomplete(_runtime, report):
            report["phase"] = "native_tests"
            return subprocess.CompletedProcess([], 0, stdout="", stderr="")
        with patch.object(runner.pdf_runner, "inspect_runtime", return_value={}):
            action = patch.object(runner, "native_tests", side_effect=incomplete)
            self.check_failure("native_tests", "native_test_failure_or_incomplete_suite", action)

    def test_same_clock_tick_retains_both_observations(self):
        class FixedClock(datetime.datetime):
            @classmethod
            def now(cls, tz=None):
                return cls(2026, 1, 1, tzinfo=tz)
        with patch.object(runner.datetime, "datetime", FixedClock):
            self.test_inventory_rejection_replaces_prior_success()


if __name__ == "__main__":
    unittest.main()
