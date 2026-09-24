"""Failure observations must replace stale success while preserving history."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import test_ocr_workers as runner


class EvidenceTests(unittest.TestCase):
    def check_failure(self, phase, failure, action):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch.object(runner, "ROOT", root), patch.object(runner, "SOURCES", []), patch.object(runner, "digest", return_value="0" * 64), patch.object(runner.platform, "system", return_value="Darwin"), patch.object(runner.subprocess, "check_output", return_value="synthetic"):
                runner.save({"passed": True, "outcome": "passed"}, "previous output")
                with action:
                    report, diagnostics = runner.observe(root)
                runner.save(report, diagnostics)
                latest = json.loads((root / "artifacts/ocr-result.json").read_text())
                self.assertFalse(latest["passed"])
                self.assertEqual(latest["phase"], phase)
                self.assertEqual(latest["failure"], failure)
                self.assertEqual(len(list((root / "artifacts/ocr").glob("*.json"))), 2)
                self.assertNotIn("private-path", json.dumps(latest))

    def test_inventory_rejection_replaces_prior_success(self):
        self.check_failure("runtime_inventory", "incomplete_runtime_inventory", patch.object(runner, "inspect_runtime", side_effect=runner.VerificationFailure("incomplete_runtime_inventory")))

    def test_timeout_replaces_prior_success(self):
        def timeout(_runtime, report):
            report["phase"] = "native_tests"
            raise subprocess.TimeoutExpired(["private-path"], 120)
        with patch.object(runner, "inspect_runtime", return_value={}):
            self.check_failure("native_tests", "tool_timeout", patch.object(runner, "native_tests", side_effect=timeout))

    def test_compiler_failure_replaces_prior_success(self):
        with patch.object(runner, "inspect_runtime", return_value={}):
            self.check_failure("compile_probe", "tool_failed", patch.object(runner.subprocess, "run", side_effect=subprocess.CalledProcessError(1, ["private-path"])))


if __name__ == "__main__":
    unittest.main()
