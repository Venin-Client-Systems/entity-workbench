"""Offline synthetic oracles; never launches a resolver, HTTPS request or native test."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from test_native_https_receipts import events as positive_events, SOURCE, NONCE
import native_https_receipt as shared
import native_cancellation_receipt as receipt
import test_native_durable_https as runner


def events():
    values = positive_events()
    for value in values:
        value["policy"] = receipt.POLICY
    values[0]["details"].update(proof_boundary="owned_response_before_body_consumption", handshake_seconds=5)
    observed = values[5]["details"]["receipt"]
    head = observed["outcome"]["head"]
    observed["outcome"] = {"kind": "stopped", "reason": "cancelled", "head": head}
    observed["stop_observed"] = "cancelled"
    final = values[-1]["details"]
    final["inspection"]["run"].update(state="cancelled", pages_retained=0, cancellation_requested=True)
    request = final["inspection"]["requests"][1]
    request["original"] = None
    request["progress"]["receipt"] = copy.deepcopy(observed)
    ack = {"job_id": "fixed-job", "generation": 1, "requests_used": 2,
           "cancellation_requested": True, "state": "running"}
    final.update(terminal_resume_refused=True, control_failed=False,
                 gate={"reached_headers": True, "notification_failed": False,
                       "cancellation_observed": True, "handshake_expired": False},
                 response_head=copy.deepcopy(head), cancel_acknowledgement=copy.deepcopy(ack),
                 proof_boundary="owned_response_before_body_consumption")
    controls = [
        ("response_headers", {"sequence": 1, "head": copy.deepcopy(head)}),
        ("cancel_intent", {"job_id": "fixed-job", "generation": 1, "sequence": 1}),
        ("cancel_acknowledged", copy.deepcopy(ack)),
    ]
    values[5:5] = [{"policy": receipt.POLICY, "source": SOURCE, "nonce": NONCE,
                    "kind": kind, "details": details} for kind, details in controls]
    return values


def replace_receipt(values, change):
    observed = next(e["details"]["receipt"] for e in values
                    if e["kind"] == "observation" and e["details"]["sequence"] == 1)
    change(observed)
    values[-1]["details"]["inspection"]["requests"][1]["progress"]["receipt"] = copy.deepcopy(observed)


class CancellationEvidenceTests(unittest.TestCase):
    def test_exact_two_allowed_ack_observation_orders_and_frozen_positive_compatibility(self):
        receipt.validate(events(), SOURCE, NONCE, 0)
        swapped = events()
        swapped[7], swapped[8] = swapped[8], swapped[7]
        receipt.validate(swapped, SOURCE, NONCE, 0)
        historical = json.loads((runner.ROOT / "docs/discovery/verification/native-durable-https-first-2026-09-26.json").read_text())
        shared.validate(historical["events"], historical["source"]["revision"],
                        historical["nonce"], historical["native_exit_code"])
        shared.validate(positive_events(), SOURCE, NONCE, 0)
        with self.assertRaises(shared.Failure):
            shared.validate(events(), SOURCE, NONCE, 0)
        with self.assertRaises(shared.Failure):
            receipt.validate(positive_events(), SOURCE, NONCE, 0)

    def test_missing_duplicate_failed_zero_selected_and_reordered_intent_refused(self):
        reordered = events(); reordered[6], reordered[8] = reordered[8], reordered[6]
        for values, code in (([], 0), (events()[:-1], 0), (events() + events()[-1:], 0),
                             (events(), 1), (events()[:7] + events()[8:], 0), (reordered, 0)):
            with self.subTest(code=code, size=len(values)), self.assertRaises(shared.Failure):
                receipt.validate(values, SOURCE, NONCE, code)

    def test_durable_cancellation_exact_gate_scope_and_recovery_required_for_pass(self):
        mutations = [
            lambda e: e[0]["details"].update(handshake_seconds=20),
            lambda e: e[4]["details"].update(url="https://other.invalid/"),
            lambda e: e[5]["details"].update(sequence=0),
            lambda e: e[6]["details"].update(generation=2),
            lambda e: e[7]["details"].update(cancellation_requested=False),
            lambda e: e[7]["details"].update(requests_used=True),
            lambda e: e[-1]["details"].update(terminal_resume_refused=False),
            lambda e: e[-1]["details"].update(control_failed=True),
            lambda e: e[-1]["details"]["gate"].update(handshake_expired=True),
            lambda e: e[-1]["details"]["gate"].update(notification_failed=True),
            lambda e: e[-1]["details"]["gate"].update(cancellation_observed=False),
            lambda e: e[-1]["details"]["inspection"]["run"].update(pages_retained=1),
            lambda e: e[-1]["details"]["inspection"]["run"].update(cancellation_requested=False),
            lambda e: e[-1]["details"]["inspection"]["requests"][1].update(original={"sha256": "d" * 64}),
            lambda e: e[-1]["details"].update(shutdown_ok=False),
            lambda e: e[-1]["details"].update(ownership_released=False),
            lambda e: e[-1]["details"].update(backup_restored_verified=False),
            lambda e: e[-1]["details"]["inspection"].update(native_execution_enabled=True),
        ]
        for index, change in enumerate(mutations):
            value = events(); change(value)
            with self.subTest(index=index), self.assertRaises(shared.Failure):
                receipt.validate(value, SOURCE, NONCE, 0)

    def test_unknown_timeout_or_phantom_body_never_substitutes_for_owned_cancel(self):
        mutations = [
            lambda r: r.update(locally_quiescent=False),
            lambda r: r.update(resolver_uncertainty={"caller_context": "released_after_completion"}),
            lambda r: r.update(stop_observed="timeout"),
            lambda r: r["outcome"].update(reason="network"),
            lambda r: r["outcome"].update(sha256="c" * 64, bytes=0),
            lambda r: r["outcome"].update(kind="complete"),
            lambda r: r["resolved"].update(addresses=["127.0.0.1:443"]),
            lambda r: r["resolved"].update(authoritative_complete_set=True),
        ]
        for index, change in enumerate(mutations):
            value = events(); replace_receipt(value, change)
            with self.subTest(index=index), self.assertRaises(shared.Failure):
                receipt.validate(value, SOURCE, NONCE, 0)

    def test_nonopted_cancellation_stops_before_source_or_native(self):
        with tempfile.TemporaryDirectory() as temp, patch.object(runner, "identity") as identity, \
                patch.object(runner, "run_logged") as execute:
            report = runner.observe(Path(temp), False, Path(temp) / "unused", cancellation=True)
            identity.assert_not_called(); execute.assert_not_called()
            self.assertFalse(report["passed"])
            self.assertFalse(report["claims"]["owned_response_cancellation_verified"])
            self.assertEqual(report["failure"], "explicit_fixed_cancellation_opt_in_required")
            self.assertEqual(json.loads((Path(temp) / "report.json").read_text()), report)

    def mocked_run(self, *, timeout=False, source_changed=False):
        with tempfile.TemporaryDirectory() as temp:
            artifact = Path(temp) / NONCE; artifact.mkdir()
            binary = artifact / "not-an-executable"; binary.write_bytes(b"offline")
            source = [{"revision": SOURCE}, {"revision": SOURCE},
                      {"revision": "b" * 40 if source_changed else SOURCE}]
            def fake_digest(path):
                return receipt.FIXTURE_SHA if path.name == "brief.txt" else "e" * 64
            def execute(command, log, seconds, environment):
                self.assertEqual(command, [str(binary), receipt.TEST, "--exact", "--ignored", "--nocapture", "--test-threads=1"])
                self.assertEqual(seconds, 30)
                self.assertEqual(environment["EW_NATIVE_HTTPS_PROOF"], receipt.POLICY)
                self.assertEqual(environment["EW_NATIVE_HTTPS_SOURCE"], SOURCE)
                self.assertEqual(environment["EW_NATIVE_HTTPS_NONCE"], NONCE)
                values = events()[:7] if timeout else events()
                text = "\n".join(receipt.PREFIX + json.dumps(value) for value in values)
                if timeout:
                    text += "\n" + receipt.PREFIX + '{"kind":'
                log.write_text(text)
                if timeout:
                    raise runner.ProcessTimeout({"kill_signal_sent": False, "process_exit_confirmed": False,
                        "exit_code": None, "errors": ["process_group_kill_failed", "process_reap_timeout"]})
                return 0
            with patch.object(runner.platform, "system", return_value="Darwin"), \
                    patch.object(runner, "identity", side_effect=source), \
                    patch.object(runner, "text_command", return_value="offline"), \
                    patch.object(runner, "digest", side_effect=fake_digest), \
                    patch.object(runner, "build_binary", return_value=binary), \
                    patch.object(runner, "run_logged", side_effect=execute) as launch:
                report = runner.observe(artifact, True, artifact / "unused", cancellation=True)
            launch.assert_called_once()
            self.assertEqual(json.loads((artifact / "report.json").read_text()), report)
            return report

    def test_runner_closed_profile_binds_source_files_and_only_one_exact_entry(self):
        report = self.mocked_run()
        self.assertTrue(report["passed"])
        self.assertTrue(report["claims"]["owned_response_cancellation_verified"])
        self.assertFalse(report["claims"]["complete_activation_gate"])
        self.assertTrue(report["claims"]["body_already_buffered_by_os_or_client_possible"])
        self.assertIn("cancellation_gate_sha256", report["source"])
        self.assertIn("cancellation_native_test_sha256", report["source"])

    def test_timeout_keeps_intent_prefix_primary_failure_and_unconfirmed_stop(self):
        report = self.mocked_run(timeout=True)
        self.assertFalse(report["passed"])
        self.assertFalse(report["claims"]["owned_response_cancellation_verified"])
        self.assertEqual(report["events"], events()[:7])
        self.assertEqual(report["failure"], "outer_process_timeout")
        self.assertEqual(report["event_parse_error"], "malformed_event_json")
        self.assertFalse(report["owned_process_termination"]["process_exit_confirmed"])
        self.assertFalse(report["owned_resource_cleanup_proven"])
        self.assertTrue(report["source_unchanged_after"])
        self.assertTrue(report["binary_unchanged_after"])

    def test_post_run_source_change_clears_success_and_cancellation_claim(self):
        report = self.mocked_run(source_changed=True)
        self.assertFalse(report["passed"])
        self.assertFalse(report["claims"]["owned_response_cancellation_verified"])
        self.assertIn("source_changed_during_observation", report["evidence_failures"])


if __name__ == "__main__":
    unittest.main()
