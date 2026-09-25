"""Portable evidence failure regressions; these do not render or invoke Cargo."""
import json
import subprocess
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

import test_docx_report as runner


class EvidenceTests(unittest.TestCase):
    def test_failed_preflight_retains_a_failed_observation(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            directory, passed = runner.run(root / "missing-runtime", root / "runs")
            self.assertFalse(passed)
            report = json.loads((directory / "report.json").read_text())
            self.assertEqual((report["phase"], report["outcome"]), ("preflight", "failed"))
            self.assertEqual(report["visual_review"], "not_performed")
            self.assertEqual(len(list(directory.glob("observation-*.json"))), 2)

    def test_timeout_replaces_latest_and_keeps_prior_observations(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            runtime = root / "runtime"
            python = runtime / "dependencies/python/bin/python3"
            python.parent.mkdir(parents=True)
            python.touch()
            (runtime / "runtime.json").write_text("{}")
            with patch.object(runner.sys, "executable", str(python)), \
                 patch.object(runner, "digest", return_value="0" * 64), \
                 patch.object(runner.subprocess, "check_output", return_value="test\n"), \
                 patch.object(runner, "run_command", side_effect=subprocess.TimeoutExpired("cargo", 300)):
                directory, passed = runner.run(runtime, root / "runs")
            report = json.loads((directory / "report.json").read_text())
            self.assertFalse(passed)
            self.assertEqual((report["phase"], report["outcome"]), ("generate", "failed"))
            self.assertEqual(report["error_type"], "TimeoutExpired")
            history = [json.loads(p.read_text()) for p in directory.glob("observation-*.json")]
            self.assertEqual(len(history), 3)
            self.assertEqual(sum(item["outcome"] == "running" for item in history), 2)

    def test_save_history_does_not_depend_on_clock_resolution(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            runner.save(root, {"outcome": "success"})
            runner.save(root, {"outcome": "failed"})
            self.assertEqual(len(list(root.glob("observation-*.json"))), 2)
            self.assertEqual(json.loads((root / "report.json").read_text()), {"outcome": "failed"})
            self.assertFalse(list(root.glob("pending-*.json")))

    def test_structure_rejects_external_relationship_before_render(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            artifact = root / "unsafe.docx"
            with zipfile.ZipFile(artifact, "w", compression=zipfile.ZIP_STORED) as archive:
                for name in runner.PARTS:
                    body = b"<root/>"
                    if name == "_rels/.rels":
                        body = b'<Relationships><Relationship Target="https://example.com" TargetMode="External"/></Relationships>'
                    archive.writestr(name, body)
            with self.assertRaisesRegex(ValueError, "external or unknown relationship"):
                runner.inspect(artifact, root / "absent.json")


if __name__ == "__main__":
    unittest.main()
