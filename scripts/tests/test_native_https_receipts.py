"""Synthetic offline oracles only. Passing these tests is never native HTTPS proof."""
import copy
import json
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import native_https_receipt as receipt
import test_native_durable_https as runner

SOURCE = "a" * 40
NONCE = "00000000-0000-4000-8000-000000000001"


def events():
    scope = {"urls": [receipt.SEED], "max_hops": 0, "max_requests": 2, "max_seconds": 20}
    preview = {"schema_version": 1, "input": scope, "collector_policy": "direct-https-durable-v3",
               "selected_hosts": ["raw.githubusercontent.com"], "robots_urls": [receipt.ROBOTS],
               "preview_sha256": "b" * 64, "disclosure": {"dns_hostnames": True, "connection_metadata": True,
               "selected_and_followed_urls": True, "automatic_case_contents": False,
               "followed_hosts": "selected_hosts_only"}}
    preview["preview_sha256"] = receipt.preview_digest(preview)
    values = [{"kind": "start", "details": {"fixture_sha256": receipt.FIXTURE_SHA, "fixture_bytes": 863,
               "max_requests": 2, "max_hops": 0, "max_seconds": 20, "synthetic": False}},
              {"kind": "queued", "details": {"preview": preview, "job_id": "fixed-job",
               "request_key": NONCE, "record_version": 3, "synthetic": False, "production_native_enabled": False}}]
    requests = []
    for index, url in enumerate((receipt.ROBOTS, receipt.SEED)):
        sha = "c" * 64 if index == 0 else receipt.FIXTURE_SHA
        size = 0 if index == 0 else 863
        observed = {"schema_version": 1, "outcome": {"kind": "complete", "head": {
                    "status": 404 if index == 0 else 200, "identity_encoding": True,
                    "redirect_url": None, "media_type": "text/plain"}, "sha256": sha, "bytes": size},
                    "phase": "body", "http_delivery": "may_have_been_sent", "elapsed_milliseconds": 10,
                    "observed_wall_ms": 1010 + index * 1000,
                    "resolved": {"method": "macos_dns_service_observed_batch", "addresses": ["8.8.8.8:443"],
                                 "authoritative_complete_set": False},
                    "resolver_uncertainty": None, "stop_observed": None, "locally_quiescent": True}
        values.extend([{"kind": "launch", "details": {"sequence": index, "url": url,
                       "job_id": "fixed-job", "generation": 1}},
                       {"kind": "observation", "details": {"sequence": index, "receipt": observed}}])
        requests.append({"sequence": index, "generation": 1, "reserved_at_ms": 1000 + index * 1000,
                         "entry": {"url": url, "hop": 0, "redirects": 0, "purpose": ["robots", "seed"][index], "parent": None},
                         "progress": {"state": "observed", "receipt": copy.deepcopy(observed)},
                         "original": {"evidence_id": sha, "sha256": sha, "bytes": size}})
    run = {"id": "fixed-job", "request_key": NONCE, "record_version": 3, "mode": "live",
           "collector_policy": "direct-https-durable-v3", "input": scope, "state": "successful",
           "generation": 1, "requests_used": 2, "pages_retained": 1, "cancellation_requested": False,
           "first_started_at_ms": 1000, "deadline_at_ms": 21000}
    final = {key: True for key in ("passed", "shutdown_ok", "ownership_released", "reopened_verified",
             "backup_restored_verified", "observations_unchanged", "receipt_settled_exactly", "acquisition_valid")}
    final.update(synthetic=False, production_native_enabled=False, guard={"admitted": 2, "refused": False},
                 fixture_sha256=receipt.FIXTURE_SHA, fixture_bytes=863, canonical_record_sha256="d" * 64,
                 elapsed_milliseconds=2000, inspection={"schema_version": 1, "workspace_revision": 7, "availability": "standalone_unavailable",
                                                        "controls": {"can_cancel": False, "can_resume": False, "can_retry_settlement": False}, "native_execution_enabled": False,
                                                        "run": run, "requests": requests})
    values.append({"kind": "final", "details": final})
    return [{"policy": receipt.POLICY, "source": SOURCE, "nonce": NONCE, **value} for value in values]


class NativeHttpsEvidenceTests(unittest.TestCase):
    def test_synthetic_oracle_is_complete_but_missing_duplicate_and_failed_events_refused(self):
        receipt.validate(events(), SOURCE, NONCE, 0)
        for values, exit_code in (([], 0), (events()[:-1], 0), (events() + events()[-1:], 0), (events(), 1)):
            with self.assertRaises(receipt.Failure):
                receipt.validate(values, SOURCE, NONCE, exit_code)

    def test_scope_nonce_modes_retained_bytes_clocks_and_uncertainty_cannot_false_pass(self):
        mutations = [
            lambda e: e[0].update(source="f" * 40),
            lambda e: e[0]["details"].update(max_hops=False),
            lambda e: e[1]["details"]["preview"].update(preview_sha256="f" * 64),
            lambda e: e[4]["details"].update(url="https://other.invalid/"),
            lambda e: e[1]["details"].update(request_key="wrong"),
            lambda e: e[-1]["details"].update(synthetic=True),
            lambda e: e[-1]["details"].update(shutdown_ok=False),
            lambda e: e[-1]["details"].update(ownership_released=False),
            lambda e: e[-1]["details"]["inspection"]["run"].update(mode="synthetic"),
            lambda e: e[-1]["details"]["inspection"]["run"].update(deadline_at_ms=22000),
            lambda e: e[-1]["details"]["inspection"]["requests"][1]["original"].update(sha256="f" * 64),
            lambda e: e[3]["details"]["receipt"].update(locally_quiescent=False),
        ]
        for mutation in mutations:
            changed = events(); mutation(changed)
            with self.subTest(mutation=mutation), self.assertRaises(receipt.Failure):
                receipt.validate(changed, SOURCE, NONCE, 0)
        for mutation in (
            lambda r: r.update(resolver_uncertainty={"caller_context": "released_after_completion"}),
            lambda r: r["resolved"].update(authoritative_complete_set=True),
            lambda r: r["resolved"].update(addresses=["127.0.0.1:443"]),
            lambda r: r.update(observed_wall_ms=900),
            lambda r: r["outcome"]["head"].update(identity_encoding=False),
            lambda r: r["outcome"]["head"].update(redirect_url=receipt.SEED),
        ):
            changed = events()
            mutation(changed[3]["details"]["receipt"])
            changed[-1]["details"]["inspection"]["requests"][0]["progress"]["receipt"] = copy.deepcopy(changed[3]["details"]["receipt"])
            with self.assertRaises(receipt.Failure):
                receipt.validate(changed, SOURCE, NONCE, 0)

    def test_duplicate_event_keys_are_refused(self):
        with self.assertRaisesRegex(receipt.Failure, "duplicate_event_key"):
            receipt.decode(receipt.PREFIX + '{"kind":"start","kind":"final"}')

    def test_decode_keeps_only_valid_prefix_without_skipping_malformed_event(self):
        for tail in ('{"kind":', '[]', '{"kind":"start","kind":"final"}'):
            output = "\n".join(receipt.PREFIX + json.dumps(value) for value in events()[:2])
            output += "\n" + receipt.PREFIX + tail + "\n" + receipt.PREFIX + json.dumps(events()[2])
            with self.assertRaises(receipt.EventDecodeFailure) as failure:
                receipt.decode(output)
            self.assertEqual(failure.exception.events, events()[:2])

    def test_outer_timeout_retains_kill_or_reap_failure_without_retry(self):
        timeout = subprocess.TimeoutExpired("synthetic", 30)
        cases = ((None, -9, True, True, []),
                 (OSError("private/path"), -9, False, True, ["process_group_kill_failed"]),
                 (None, timeout, True, False, ["process_reap_timeout"]),
                 (OSError("private/path"), timeout, False, False,
                  ["process_group_kill_failed", "process_reap_timeout"]),
                 (None, OSError("private/path"), True, False, ["process_reap_failed"]))
        for kill_error, completion, killed, confirmed, errors in cases:
            with self.subTest(errors=errors), tempfile.TemporaryDirectory() as temp:
                process = Mock(pid=12345)
                process.wait.side_effect = [timeout, completion]
                with patch.object(runner.subprocess, "Popen", return_value=process) as launch, \
                        patch.object(runner.os, "killpg", side_effect=kill_error) as kill:
                    with self.assertRaises(runner.ProcessTimeout) as failure:
                        runner.run_logged(["never-executed"], Path(temp) / "log", 30)
                self.assertEqual(str(failure.exception), "outer_process_timeout")
                self.assertEqual(failure.exception.termination, {
                    "kill_signal_sent": killed, "process_exit_confirmed": confirmed,
                    "exit_code": -9 if confirmed else None, "errors": errors})
                launch.assert_called_once(); kill.assert_called_once()
                self.assertEqual(process.wait.call_count, 2)
                self.assertEqual(process.wait.call_args_list[0].kwargs, {"timeout": 30})
                self.assertEqual(process.wait.call_args_list[1].kwargs, {"timeout": 5})

    def test_nonopted_runner_stops_before_source_build_or_native_process(self):
        with tempfile.TemporaryDirectory() as temp, patch.object(runner, "identity") as identity, \
                patch.object(runner, "run_logged") as execute:
            report = runner.observe(Path(temp), False, Path(temp) / "unused")
            identity.assert_not_called(); execute.assert_not_called()
            self.assertFalse(report["passed"])
            self.assertEqual(report["failure"], "explicit_fixed_https_opt_in_required")
            self.assertEqual(json.loads((Path(temp) / "report.json").read_text()), report)

    def test_metadata_failure_retains_initial_incomplete_receipt(self):
        with tempfile.TemporaryDirectory() as temp:
            artifact = Path(temp) / NONCE; artifact.mkdir()
            def fail(_):
                self.assertEqual(json.loads((artifact / "report.json").read_text())["outcome"], "incomplete")
                raise OSError("private/path")
            with patch.object(runner.platform, "system", return_value="Darwin"), \
                    patch.object(runner, "identity", side_effect=fail):
                report = runner.observe(artifact, True, artifact / "unused")
            self.assertFalse(report["passed"])
            self.assertEqual(report["phase"], "source")
            self.assertNotIn("private/path", json.dumps(report))

    def test_native_timeout_preserves_partial_evidence_and_never_claims_cleanup(self):
        with tempfile.TemporaryDirectory() as temp:
            artifact = Path(temp) / NONCE; artifact.mkdir()
            binary = artifact / "not-an-executable"; binary.write_bytes(b"synthetic")
            def fake_digest(path):
                return receipt.FIXTURE_SHA if path.name == "brief.txt" else "e" * 64
            def timeout(_command, log, _timeout, _environment):
                log.write_text("\n".join(receipt.PREFIX + json.dumps(value) for value in events()[:2])
                               + "\n" + receipt.PREFIX + '{"kind":')
                raise runner.ProcessTimeout({"kill_signal_sent": False,
                    "process_exit_confirmed": False, "exit_code": None,
                    "errors": ["process_group_kill_failed", "process_reap_timeout"]})
            with patch.object(runner.platform, "system", return_value="Darwin"), \
                    patch.object(runner, "identity", return_value={"revision": SOURCE}), \
                    patch.object(runner, "text_command", return_value="offline"), \
                    patch.object(runner, "digest", side_effect=fake_digest), \
                    patch.object(runner, "build_binary", return_value=binary), \
                    patch.object(runner, "run_logged", side_effect=timeout):
                report = runner.observe(artifact, True, artifact / "unused")
            self.assertFalse(report["passed"])
            self.assertEqual(report["failure"], "outer_process_timeout")
            self.assertEqual(report["events"], events()[:2])
            self.assertEqual(report["event_parse_error"], "malformed_event_json")
            self.assertIn("partial_event_decode_failed", report["evidence_failures"])
            self.assertTrue(report["source_unchanged_after"])
            self.assertTrue(report["binary_unchanged_after"])
            self.assertFalse(report["owned_process_termination"]["process_exit_confirmed"])
            self.assertEqual(report["owned_process_termination"]["errors"],
                             ["process_group_kill_failed", "process_reap_timeout"])
            self.assertFalse(report["owned_resource_cleanup_proven"])
            self.assertIn("native.log", report["logs"])

    def test_changed_source_after_build_never_launches_native_process(self):
        with tempfile.TemporaryDirectory() as temp:
            artifact = Path(temp) / NONCE; artifact.mkdir()
            def fake_digest(path):
                return receipt.FIXTURE_SHA if path.name == "brief.txt" else "e" * 64
            with patch.object(runner.platform, "system", return_value="Darwin"), \
                    patch.object(runner, "identity", side_effect=[{"revision": SOURCE}, {"revision": "b" * 40}, {"revision": "b" * 40}]), \
                    patch.object(runner, "text_command", return_value="offline"), \
                    patch.object(runner, "digest", side_effect=fake_digest), \
                    patch.object(runner, "build_binary", return_value=artifact / "binary"), \
                    patch.object(runner, "run_logged") as execute:
                report = runner.observe(artifact, True, artifact / "unused")
            execute.assert_not_called()
            self.assertFalse(report["passed"])
            self.assertEqual(report["failure"], "source_changed_before_native_run")


if __name__ == "__main__":
    unittest.main()
