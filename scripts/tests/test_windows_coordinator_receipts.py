"""Negative receipt/runner tests, not native execution evidence."""
from pathlib import Path
import copy
import importlib.util
import json
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("coordinator_campaign", Path(__file__).resolve().parents[1] / "test_windows_parser_coordinator.py")
CAMPAIGN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CAMPAIGN)
SOURCE = "a" * 40
NONCE = "d210ee11-5199-4d47-a68a-4b973639f968"


def success(nonce=NONCE):
    identities = CAMPAIGN.fixture_identities()
    return {
        "schema_version": 1, "source_commit": SOURCE, "build_source_commit": SOURCE,
        "nonce": nonce, "complete_release": False, "passed": True, "phase": "complete",
        "failure": None, "checks": dict.fromkeys(CAMPAIGN.CHECKS, True), "fixtures": identities,
        "derivatives": [{"fixture": row[0], "source_sha256": identities[index]["sha256"],
                         "record_sha256": "1" * 64, "result_sha256": "2" * 64,
                         "text_sha256": "3" * 64, "schema_version": 2,
                         "state": row[3], "parser": row[4]} for index, row in enumerate(CAMPAIGN.FIXTURES)],
        "joined_coordinators": 4, "retained_workspace": False, "in_flight_cancellation_proven": False,
    }


class ContractTests(unittest.TestCase):
    def test_source_identity_rejects_nonignored_untracked_modules(self):
        outputs = [SOURCE.encode(), b"b" * 40, b"c" * 40]
        with patch.object(CAMPAIGN.subprocess, "check_output", side_effect=outputs + [b""]) as git:
            self.assertTrue(CAMPAIGN.source_identity()["source_clean"])
            self.assertEqual(git.call_args.args[0], ["git", "status", "--porcelain"])
        for dirty in [b"?? crates/core/examples/untracked.rs", b" M crates/core/src/lib.rs"]:
            with self.subTest(dirty=dirty), patch.object(CAMPAIGN.subprocess, "check_output", side_effect=outputs + [dirty]):
                with self.assertRaises(ValueError):
                    CAMPAIGN.source_identity()

    def test_requires_every_exact_case_and_both_source_identities(self):
        CAMPAIGN.validate_probe(success(), SOURCE, NONCE, require_pass=True)
        for key in CAMPAIGN.CHECKS:
            value = success()
            del value["checks"][key]
            with self.assertRaises(ValueError):
                CAMPAIGN.validate_probe(value, SOURCE, NONCE, require_pass=True)
        for key, value in [("source_commit", "b" * 40), ("build_source_commit", None),
                           ("nonce", "another-run"), ("joined_coordinators", 3),
                           ("joined_coordinators", True), ("schema_version", True),
                           ("retained_workspace", True), ("complete_release", True),
                           ("in_flight_cancellation_proven", True), ("failure", "deadline")]:
            result = success()
            result[key] = value
            with self.assertRaises(ValueError):
                CAMPAIGN.validate_probe(result, SOURCE, NONCE, require_pass=True)

    def test_wrong_duplicate_absent_and_unbound_derivatives_reject(self):
        for mutation in range(6):
            value = success()
            if mutation == 0:
                value["derivatives"].pop()
            elif mutation == 1:
                value["derivatives"][1] = copy.deepcopy(value["derivatives"][0])
            elif mutation == 2:
                value["derivatives"][0]["source_sha256"] = "0" * 64
            elif mutation == 3:
                value["derivatives"][0]["schema_version"] = 1
            elif mutation == 4:
                value["derivatives"][0]["parser"] = "unexpected"
            else:
                value["fixtures"][0]["sha256"] = "0" * 64
            with self.assertRaises(ValueError):
                CAMPAIGN.validate_probe(value, SOURCE, NONCE, require_pass=True)

    def test_arbitrary_diagnostics_are_not_copied_to_the_artifact(self):
        for key in ["phase", "failure"]:
            value = success()
            value[key] = "unbounded worker-provided path or message"
            with self.assertRaises(ValueError):
                CAMPAIGN.validate_probe(value, SOURCE, NONCE)
        value = success()
        value["worker_log"] = "not permitted"
        with self.assertRaises(ValueError):
            CAMPAIGN.validate_probe(value, SOURCE, NONCE)

    def test_partial_receipt_cannot_become_success(self):
        value = success()
        value.update(passed=False, phase="fixture_notice.txt", failure="deadline", joined_coordinators=1, retained_workspace=True)
        value["checks"] = {}
        value["derivatives"] = []
        CAMPAIGN.validate_probe(value, SOURCE, NONCE)
        with self.assertRaises(ValueError):
            CAMPAIGN.validate_probe(value, SOURCE, NONCE, require_pass=True)

    def test_duplicate_json_nonfinite_and_oversize_reject(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "probe.receipt"
            for raw in [b'{"passed":false,"passed":true}', b'{"x":NaN}', b" " * (128 * 1024 + 1)]:
                path.write_bytes(raw)
                with self.assertRaises(ValueError):
                    CAMPAIGN.read_json(path)


class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.binary = self.root / "synthetic-test-binary"
        self.binary.write_bytes(b"runner contract fixture, never executable")
        self.runtime = self.root / "runtime"
        for role in ("parser", "search"):
            (self.runtime / role).mkdir(parents=True)
            (self.runtime / role / "manifest.json").write_text("{}")
        self.output = self.root / "receipt"
        self.output.mkdir()
        self.identity = {"source_commit": SOURCE, "source_tree": "b" * 40,
                         "source_parents": ["c" * 40], "source_clean": True}

    def execute(self, replacement):
        with patch.object(CAMPAIGN.subprocess, "run", side_effect=replacement), patch.object(CAMPAIGN, "source_identity", return_value=self.identity):
            return CAMPAIGN.campaign(self.binary, self.runtime, self.output, self.identity, timeout=1)

    def write_success(self, argv):
        Path(argv[2]).write_text(json.dumps(success(argv[8])))

    def test_runner_initializes_failed_before_launch_and_accepts_only_exact_run(self):
        (self.output / "coordinator-report.json").write_text('{"passed":true}')
        def replacement(argv, **_):
            pending = json.loads((self.output / "coordinator-report.json").read_text())
            self.assertIs(pending["passed"], False)
            self.write_success(argv)
            return subprocess.CompletedProcess(argv, 0)
        result = self.execute(replacement)
        self.assertIs(result["passed"], True)
        self.assertIsNone(result["failure"])

    def test_timeout_cannot_leave_earlier_or_late_success(self):
        for late in [False, True]:
            with self.subTest(late=late):
                (self.output / "coordinator-probe.receipt").write_text(json.dumps(success()))
                def replacement(argv, **_):
                    if late:
                        self.write_success(argv)
                    raise subprocess.TimeoutExpired(argv, 1)
                result = self.execute(replacement)
                self.assertIs(result["passed"], False)
                self.assertEqual(result["failure"], "timeout")

    def test_nonzero_exit_overrides_a_complete_receipt(self):
        def replacement(argv, **_):
            self.write_success(argv)
            return subprocess.CompletedProcess(argv, 1)
        result = self.execute(replacement)
        self.assertIs(result["passed"], False)
        self.assertEqual(result["failure"], "probe_exit")

    def test_unknown_output_is_redacted_and_malformed_receipt_fails(self):
        for raw in [b"{", b'{"raw_worker_message":"never export this"}']:
            def replacement(argv, **_):
                Path(argv[2]).write_bytes(raw)
                return subprocess.CompletedProcess(argv, 0)
            result = self.execute(replacement)
            self.assertIs(result["passed"], False)
            self.assertIsNone(result["probe"])
            self.assertNotIn("never export this", (self.output / "coordinator-report.json").read_text())

    def test_runtime_identity_change_refuses_success(self):
        def replacement(argv, **_):
            self.write_success(argv)
            (self.runtime / "parser/manifest.json").write_text('{"changed":true}')
            return subprocess.CompletedProcess(argv, 0)
        result = self.execute(replacement)
        self.assertIs(result["passed"], False)

    def test_untracked_source_preflight_replaces_old_success_without_launch(self):
        (self.output / "coordinator-report.json").write_text('{"passed":true}')
        argv = ["campaign", "--binary", str(self.binary), "--runtime", str(self.runtime),
                "--destination", str(self.output)]
        outputs = [SOURCE.encode(), b"b" * 40, b"c" * 40, b"?? crates/core/examples/untracked.rs"]
        with patch.object(CAMPAIGN.sys, "argv", argv), patch.object(CAMPAIGN.platform, "system", return_value="Windows"), \
                patch.object(CAMPAIGN.subprocess, "check_output", side_effect=outputs), \
                patch.object(CAMPAIGN.subprocess, "run") as run:
            self.assertEqual(CAMPAIGN.main(), 1)
            run.assert_not_called()
        retained = CAMPAIGN.read_json(self.output / "coordinator-report.json")
        self.assertIs(retained["passed"], False)
        self.assertEqual(retained["failure"], "preflight_rejected")


if __name__ == "__main__":
    unittest.main()
