"""Offline evidence-runner regressions; these mocks never establish native proof."""
import contextlib
import copy
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import test_native_collection_transport as runner


def events():
    values = []
    for index, name in enumerate(runner.CASES):
        count = int(index >= 2)
        values.append({"case": name, "passed": True,
                       "hostname": "ew-native-proof.invalid" if index == 3 else "example.com",
                       "probe": {"attempted": count, "created": count, "deallocated": count,
                                 "callbacks": [[None, -65554]] if index == 3 else []},
                       "outcome": {"authoritative_complete_set": False}})
    return values


class EvidenceTests(unittest.TestCase):
    def test_no_opt_in_never_inspects_or_starts_network(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(runner, "source_identity") as identity:
            artifact = Path(directory)
            report = runner.observe(artifact, False, artifact / "unused")
            identity.assert_not_called()
            self.assertFalse(report["passed"])
            self.assertEqual(report["failure"], "explicit_fixed_dns_opt_in_required")
            self.assertEqual(json.loads((artifact / "report.json").read_text()), report)

    def test_metadata_failure_is_saved_before_lookup(self):
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory)

            def unavailable(_signers):
                initial = json.loads((artifact / "report.json").read_text())
                self.assertFalse(initial["passed"])
                self.assertEqual(initial["outcome"], "incomplete")
                raise OSError("private-host-path must not appear in report")

            with patch.object(runner.platform, "system", return_value="Darwin"), \
                    patch.object(runner, "source_identity", side_effect=unavailable):
                report = runner.observe(artifact, True, artifact / "unused")
            self.assertFalse(report["passed"])
            self.assertNotIn("private-host-path", json.dumps(report))
            self.assertEqual(report["phase"], "source_identity")

    def test_missing_duplicate_failure_and_unclosed_results_never_pass(self):
        runner.validate_cases(events(), 0)
        bad_sets = [[], events()[:-1], events() + events()[:1]]
        for key, value in [("passed", False), ("hostname", "other.invalid")]:
            bad = events()
            bad[0][key] = value
            bad_sets.append(bad)
        bad = events()
        bad[2]["probe"]["deallocated"] = 0
        bad_sets.append(bad)
        bad = events()
        bad[3]["probe"]["callbacks"] = [[0, -65563]]
        bad_sets.append(bad)
        for case in bad_sets:
            with self.subTest(case=case), self.assertRaises(runner.Failure):
                runner.validate_cases(case, 0)
        with self.assertRaises(runner.Failure):
            runner.validate_cases(events(), 1)

    def test_native_timeout_keeps_partial_events_and_failure(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.ExitStack() as stack:
            artifact = Path(directory)
            binary = artifact / "test-binary"
            binary.write_bytes(b"offline synthetic executable identity")
            stack.enter_context(patch.object(runner.platform, "system", return_value="Darwin"))
            stack.enter_context(patch.object(runner, "source_identity", return_value={"revision": "fixed"}))
            stack.enter_context(patch.object(runner, "text_command", return_value="synthetic"))
            stack.enter_context(patch.object(runner, "digest", return_value="0" * 64))
            stack.enter_context(patch.object(runner, "build_binary", return_value=binary))

            def timeout(_command, log, _timeout, _environment):
                log.write_text(runner.PREFIX + json.dumps(events()[0]) + "\n")
                raise runner.Failure("outer_process_timeout")

            stack.enter_context(patch.object(runner, "run_logged", side_effect=timeout))
            report = runner.observe(artifact, True, artifact / "unused")
            self.assertFalse(report["passed"])
            self.assertEqual(report["phase"], "native_campaign")
            self.assertEqual(report["failure"], "outer_process_timeout")
            self.assertEqual(report["cases"], events()[:1])
            self.assertIn("native.log", report["logs"])

    def test_source_change_prevents_native_attempt(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.ExitStack() as stack:
            artifact = Path(directory)
            stack.enter_context(patch.object(runner.platform, "system", return_value="Darwin"))
            stack.enter_context(patch.object(runner, "source_identity", side_effect=[
                {"revision": "A"}, {"revision": "B"}, {"revision": "B"}]))
            stack.enter_context(patch.object(runner, "text_command", return_value="synthetic"))
            stack.enter_context(patch.object(runner, "digest", return_value="0" * 64))
            stack.enter_context(patch.object(runner, "build_binary", return_value=artifact / "binary"))
            execute = stack.enter_context(patch.object(runner, "run_logged"))
            report = runner.observe(artifact, True, artifact / "unused")
            execute.assert_not_called()
            self.assertEqual(report["failure"], "source_changed_before_native_run")
            self.assertFalse(report["source_unchanged_after"])

    def test_log_failure_preserves_original_native_failure_and_postcheck(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.ExitStack() as stack:
            artifact = Path(directory)
            stack.enter_context(patch.object(runner.platform, "system", return_value="Darwin"))
            identity = stack.enter_context(patch.object(runner, "source_identity", return_value={"revision": "fixed"}))
            stack.enter_context(patch.object(runner, "text_command", return_value="synthetic"))
            stack.enter_context(patch.object(runner, "build_binary", return_value=artifact / "binary"))

            def digest(path):
                if path.name == "native.log":
                    raise OSError("private diagnostic path unavailable")
                return "0" * 64

            def timeout(_command, log, _timeout, _environment):
                log.write_text(runner.PREFIX + json.dumps(events()[0]) + "\n")
                raise runner.Failure("outer_process_timeout")

            stack.enter_context(patch.object(runner, "digest", side_effect=digest))
            stack.enter_context(patch.object(runner, "run_logged", side_effect=timeout))
            report = runner.observe(artifact, True, artifact / "unused")
            self.assertEqual(identity.call_count, 3)
            self.assertEqual(report["failure"], "outer_process_timeout")
            self.assertIn("diagnostic_identity_unavailable", report["evidence_failures"])
            self.assertEqual(report["cases"], events()[:1])
            self.assertNotIn("private diagnostic", json.dumps(report))

    def test_final_report_write_failure_cannot_return_success(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.ExitStack() as stack:
            artifact = Path(directory)
            stack.enter_context(patch.object(runner.platform, "system", return_value="Darwin"))
            stack.enter_context(patch.object(runner, "source_identity", return_value={"revision": "fixed"}))
            stack.enter_context(patch.object(runner, "text_command", return_value="synthetic"))
            stack.enter_context(patch.object(runner, "digest", return_value="0" * 64))
            stack.enter_context(patch.object(runner, "build_binary", return_value=artifact / "binary"))
            original_save = runner.save

            def save(path, report):
                if report["passed"]:
                    raise OSError("private final report failure")
                original_save(path, report)

            def complete(_command, log, _timeout, _environment):
                log.write_text("\n".join(runner.PREFIX + json.dumps(event) for event in events()))
                return 0

            stack.enter_context(patch.object(runner, "save", side_effect=save))
            stack.enter_context(patch.object(runner, "run_logged", side_effect=complete))
            report = runner.observe(artifact, True, artifact / "unused")
            self.assertFalse(report["passed"])
            self.assertEqual(report["failure"], "final_report_write_failed")
            self.assertFalse(json.loads((artifact / "report.json").read_text())["passed"])

    def test_explicit_full_rrset_claim_is_rejected(self):
        changed = copy.deepcopy(events())
        changed[2]["outcome"]["authoritative_complete_set"] = True
        with self.assertRaises(runner.Failure):
            runner.validate_cases(changed, 0)

    @unittest.skipUnless(os.name == "posix", "runner's native target is macOS")
    def test_outer_timeout_actually_joins_owned_process(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "timeout.log"
            command = [sys.executable, "-c",
                       "import os,time; print(os.getpid(),flush=True); time.sleep(60)"]
            with self.assertRaisesRegex(runner.Failure, "outer_process_timeout"):
                runner.run_logged(command, log, 0.2)
            process_id = int(log.read_text().strip())
            with self.assertRaises(ProcessLookupError):
                os.kill(process_id, 0)


if __name__ == "__main__":
    unittest.main()
