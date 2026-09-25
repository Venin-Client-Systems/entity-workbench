"""Failure evidence must exist even when metadata or source preflight cannot start."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import desktop_summary_payload as probe


class EvidenceTests(unittest.TestCase):
    def test_metadata_failure_retains_failed_report(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(probe, "ROOT", Path(directory)), patch.object(
                    probe.baseline, "capture", side_effect=OSError("synthetic unavailable tool")):
                path, report = probe.run(Path(directory) / "missing")
            self.assertEqual(report["outcome"], "failed")
            self.assertEqual(report["phase"], "metadata")
            self.assertEqual(json.loads(path.read_text()), report)
            self.assertNotIn("comparison", report)

    def test_fixed_clock_runs_keep_separate_failure_history(self):
        fixed = probe.dt.datetime(2026, 1, 1, tzinfo=probe.dt.timezone.utc)
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(probe, "ROOT", Path(directory)), patch.object(
                    probe.baseline, "capture", side_effect=OSError("synthetic failure")), patch.object(
                    probe.dt, "datetime") as clock:
                clock.now.return_value = fixed
                first, _ = probe.run(Path(directory) / "missing")
                second, _ = probe.run(Path(directory) / "missing")
            self.assertNotEqual(first, second)
            self.assertTrue(first.exists() and second.exists())


if __name__ == "__main__":
    unittest.main()
