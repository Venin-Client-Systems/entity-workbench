"""Rejected/unfinished image verification cannot retain the preceding success projection."""
import json
import datetime
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import test_image_workers as runner


class EvidenceTests(unittest.TestCase):
    def check_failure(self, phase, failure, action):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "fixtures/images").mkdir(parents=True)
            with patch.object(runner, "ROOT", root), patch.object(runner, "SOURCES", []), patch.object(runner.platform, "system", return_value="Darwin"), patch.object(runner.subprocess, "check_output", return_value="synthetic"):
                runner.save({"passed": True, "outcome": "passed"}, "previous output")
                with action:
                    report, diagnostics = runner.observe(root)
                runner.save(report, diagnostics)
                latest = json.loads((root / "artifacts/image-result.json").read_text())
                self.assertFalse(latest["passed"])
                self.assertEqual(latest["phase"], phase)
                self.assertEqual(latest["failure"], failure)
                self.assertEqual(len(list((root / "artifacts/image").glob("*.json"))), 2)
                self.assertNotIn("private-path", json.dumps(latest))

    def test_inventory_rejection_replaces_prior_success(self):
        self.check_failure("runtime_inventory", "unexpected_image_classpath", patch.object(runner, "inspect_runtime", side_effect=runner.VerificationFailure("unexpected_image_classpath")))

    def test_timeout_replaces_prior_success(self):
        def timeout(_runtime, report):
            report["phase"] = "native_tests"
            raise subprocess.TimeoutExpired(["private-path"], 180)
        with patch.object(runner, "inspect_runtime", return_value={}):
            self.check_failure("native_tests", "tool_timeout", patch.object(runner, "native_tests", side_effect=timeout))

    def test_incomplete_suite_replaces_prior_success(self):
        def incomplete(_runtime, report):
            report["phase"] = "native_tests"
            return subprocess.CompletedProcess([], 0, stdout="", stderr="")
        with patch.object(runner, "inspect_runtime", return_value={}):
            self.check_failure("native_tests", "native_test_failure_or_incomplete_suite", patch.object(runner, "native_tests", side_effect=incomplete))

    def test_same_clock_tick_retains_both_observations(self):
        class FixedClock(datetime.datetime):
            @classmethod
            def now(cls, tz=None):
                return cls(2026, 1, 1, tzinfo=tz)
        with patch.object(runner.datetime, "datetime", FixedClock):
            self.test_inventory_rejection_replaces_prior_success()


if __name__ == "__main__":
    unittest.main()
