"""Synthetic receipt/control fixtures; no native Windows or network claim."""
import contextlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import windows_dns_receipt as receipt
import test_windows_native_dns as runner

SOURCE = "a" * 40
NONCE = "00000000-0000-4000-8000-000000000001"


def probe():
    value = {key: [] for key in ("wsa_startup", "launch_returns", "cancel_returns", "completion_returns",
                                "wait_errors", "event_close", "wsa_cleanup")}
    value.update({key: 0 for key in ("events_created", "contexts_created", "wait_signalled", "wait_timeouts",
                                   "contexts_dropped", "contexts_retained", "results_freed")})
    value["trace_truncated"] = False
    return value


def released(code):
    value = probe()
    value.update(wsa_startup=[0], events_created=1, contexts_created=1,
                 launch_returns=[997], completion_returns=[code], wait_signalled=1,
                 contexts_dropped=1, event_close=[True], wsa_cleanup=[0])
    return value


def transport(reason, quiescent, phase="before_request", context=None, stop=None):
    return {"kind": "transport", "reason": reason, "phase": phase, "locally_quiescent": quiescent,
            "caller_context": context, "stop_observed": stop}


def events(retained=False):
    values = []
    for index, name in enumerate(receipt.CASES):
        value = {"schema_version": 1, "source_commit": SOURCE, "build_source_commit": SOURCE,
                 "nonce": NONCE, "policy": receipt.POLICY, "case": name, "hostname": receipt.HOSTS[index],
                 "state": "observed", "passed": True, "elapsed_milliseconds": 1, "followup": None}
        if index < 2:
            reason = "cancelled" if index == 0 else "deadline"
            value.update(probe=probe(), outcome=transport(reason, True, stop=reason))
        elif index == 2:
            p = released(0)
            p["results_freed"] = 1
            value.update(probe=p, outcome={"kind": "resolved", "candidate_count": 2,
                                          "method": "windows_completed_system_candidates", "authoritative_complete_set": False})
        elif index == 3:
            value.update(probe=released(11001), outcome={"kind": "stopped", "reason": "network"})
        else:
            p = released(10111)
            p["cancel_returns"] = [0]
            context = "released_after_completion"
            if retained:
                p.update(contexts_dropped=0, contexts_retained=1, completion_returns=[10036],
                         event_close=[], wsa_cleanup=[])
                context = "retained_pending_completion"
            value.update(probe=p, outcome=transport("quiescence_unverified", False, "dns", context, "cancelled"),
                         followup={"outcome": transport("recovery_required", False), "probe": probe()})
        values.append(value)
    return values


class ReceiptTests(unittest.TestCase):
    def validate(self, value, code=0):
        return receipt.validate(value, SOURCE, NONCE, code)

    def test_completed_and_retained_contexts_remain_distinct(self):
        self.validate(events())
        self.validate(events(retained=True))

    def test_event_or_cancel_alone_cannot_authorize_context_release(self):
        for codes in ([], [10036], [996], [997]):
            value = events()
            value[-1]["probe"]["completion_returns"] = codes
            with self.subTest(codes=codes), self.assertRaises(ValueError):
                self.validate(value)

    def test_pending_context_cannot_free_event_results_or_winsock(self):
        for field, entry in [("contexts_dropped", 1), ("event_close", [True]),
                             ("wsa_cleanup", [0]), ("results_freed", 1), ("completion_returns", [0])]:
            value = events(retained=True)
            value[-1]["probe"][field] = entry
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.validate(value)

    def test_cancelled_ack_and_reset_after_unknown_cannot_pass(self):
        changes = [("reason", "cancelled"), ("locally_quiescent", True)]
        for field, entry in changes:
            value = events()
            value[-1]["outcome"][field] = entry
            with self.assertRaises(ValueError):
                self.validate(value)
        value = events()
        value[-1]["followup"]["probe"] = released(0)
        with self.assertRaises(ValueError):
            self.validate(value)

    def test_wrong_source_nonce_order_zero_selection_and_sync_cancel_are_incomplete(self):
        values = [[], events()[:-1], list(reversed(events()))]
        for key, entry in [("source_commit", "b" * 40), ("build_source_commit", "b" * 40),
                           ("nonce", "00000000-0000-4000-8000-000000000002"), ("state", "not_exercised")]:
            value = events()
            value[-1][key] = entry
            values.append(value)
        value = events()
        value[-1]["probe"]["launch_returns"] = [0]
        values.append(value)
        for value in values:
            with self.assertRaises(ValueError):
                self.validate(value)
        for code in (101, False, 0.0, None):
            with self.subTest(code=code), self.assertRaises(ValueError):
                self.validate(events(), code)

    def test_json_duplicate_fields_limits_and_unknown_keys_are_refused(self):
        encoded = receipt.PREFIX + json.dumps(events()[0])
        self.assertEqual(receipt.decode(encoded), events()[:1])
        duplicate = encoded.replace('"passed": true', '"passed": true, "passed": true')
        for text in [duplicate, "x" * (1024 * 1024 + 1), "\n".join([encoded] * 6)]:
            with self.assertRaises(ValueError):
                receipt.decode(text)
        value = events()
        value[0]["extra"] = "not allowed"
        with self.assertRaises(ValueError):
            self.validate(value)


class RunnerTests(unittest.TestCase):
    def test_refused_opt_in_and_non_windows_never_launch(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(runner, "build_binary") as build:
            path = Path(directory)
            first = runner.observe(path, False, path / "unused")
            with patch.object(runner.platform, "system", return_value="Darwin"):
                second = runner.observe(path, True, path / "unused")
            self.assertFalse(first["passed"])
            self.assertEqual(second["failure"], "native_windows_required")
            build.assert_not_called()

    def test_metadata_failure_retains_incomplete_projection(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(runner.platform, "system", return_value="Windows"):
            path = Path(directory)
            with patch.object(runner, "identity", side_effect=OSError("private source path")):
                result = runner.observe(path, True, path / "unused")
            self.assertFalse(result["passed"])
            self.assertNotIn("private source path", json.dumps(result))
            self.assertEqual(json.loads((path / "report.json").read_text()), result)

    def test_timeout_preserves_partial_receipt_without_claiming_context_release(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.ExitStack() as stack:
            path = Path(directory)
            stack.enter_context(patch.object(runner.platform, "system", return_value="Windows"))
            stack.enter_context(patch.object(runner, "identity", return_value={"revision": SOURCE}))
            stack.enter_context(patch.object(runner, "text_command", return_value="synthetic"))
            stack.enter_context(patch.object(runner, "build_binary", return_value=path / "fake.exe"))
            stack.enter_context(patch.object(runner, "digest", return_value="a" * 64))

            def timeout(_command, log, _timeout, _environment):
                log.write_text(receipt.PREFIX + json.dumps(events()[0]) + "\n")
                raise runner.Failure("owned_process_timeout")

            stack.enter_context(patch.object(runner, "run_logged", side_effect=timeout))
            result = runner.observe(path, True, path / "unused")
            self.assertFalse(result["passed"])
            self.assertTrue(result["native_process_forcibly_stopped"])
            self.assertFalse(result["caller_context_completion_proven"])
            self.assertEqual(result["cases"], events()[:1])

    def test_post_failure_evidence_error_keeps_original_failure(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.ExitStack() as stack:
            path = Path(directory)
            stack.enter_context(patch.object(runner.platform, "system", return_value="Windows"))
            stack.enter_context(patch.object(runner, "identity", side_effect=[
                {"revision": SOURCE}, OSError("private diagnostic path")]))
            stack.enter_context(patch.object(runner, "text_command", return_value="synthetic"))
            stack.enter_context(patch.object(runner, "build_binary", side_effect=runner.Failure("native_build_failed")))
            result = runner.observe(path, True, path / "unused")
            self.assertFalse(result["passed"])
            self.assertEqual(result["failure"], "native_build_failed")
            self.assertIn("post_run_source_identity_unavailable", result["evidence_failures"])
            self.assertNotIn("private diagnostic path", json.dumps(result))
            self.assertEqual(json.loads((path / "report.json").read_text()), result)


if __name__ == "__main__":
    unittest.main()
