"""Source-bound fixed Mac durable HTTPS proof. No query without explicit reviewed opt-in."""
import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import time
import uuid

from test_native_collection_transport import (
    Failure, digest, read_log, source_identity as shared_identity, text_command,
    save, evidence_failure,
)
import native_https_receipt as receipt
import native_cancellation_receipt as cancellation_receipt

ROOT = Path(__file__).resolve().parents[1]
TEST = "coordinator::native_https_proof::native_durable_https_campaign"


class ProcessTimeout(Failure):
    def __init__(self, termination):
        super().__init__("outer_process_timeout")
        self.termination = termination


def run_logged(command, log, timeout, environment=None):
    # Preserve the initial deadline even if termination/reaping cannot be proved.
    # No second launch or signal retry; this is not native-resource/provider proof.
    with log.open("xb") as stream:
        process = subprocess.Popen(command, cwd=ROOT, env=environment,
                                   stdout=stream, stderr=subprocess.STDOUT,
                                   start_new_session=True)
        try:
            return process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            termination = {"kill_signal_sent": False, "process_exit_confirmed": False,
                           "exit_code": None, "errors": []}
            try:
                os.killpg(process.pid, signal.SIGKILL)
                termination["kill_signal_sent"] = True
            except OSError:
                termination["errors"].append("process_group_kill_failed")
            try:
                termination["exit_code"] = process.wait(timeout=5)
                termination["process_exit_confirmed"] = True
            except subprocess.TimeoutExpired:
                termination["errors"].append("process_reap_timeout")
            except OSError:
                termination["errors"].append("process_reap_failed")
            raise ProcessTimeout(termination) from None


def retain_events(report, native_log):
    try:
        report["events"] = receipt.decode(read_log(native_log))
    except receipt.EventDecodeFailure as error:
        report["events"] = error.events
        report["event_parse_error"] = str(error)
        evidence_failure(report, "partial_event_decode_failed")
        return False
    return True


def identity(signers):
    result = shared_identity(signers)
    result["shared_runner_sha256"] = result.pop("runner_sha256")
    result["runner_sha256"] = digest(Path(__file__))
    result["validator_sha256"] = digest(ROOT / "scripts/native_https_receipt.py")
    result["native_test_sha256"] = digest(ROOT / "crates/core/src/coordinator_native_https_proof.rs")
    return result


def build_binary(artifact, source):
    environment = dict(os.environ, EW_NATIVE_HTTPS_BUILD_SOURCE=source)
    status = run_logged(["cargo", "test", "--locked", "--offline", "-p", "workbench-core",
                         "--lib", "--no-run", "--message-format=json"], artifact / "build.log",
                        600, environment)
    if status:
        raise Failure("native_build_failed")
    binaries = []
    for line in read_log(artifact / "build.log").splitlines():
        try:
            item = json.loads(line)
        except ValueError:
            continue
        if (item.get("reason") == "compiler-artifact" and item.get("executable")
                and item.get("profile", {}).get("test") is True
                and item.get("target", {}).get("name") == "workbench_core"):
            binaries.append(Path(item["executable"]))
    if len(binaries) != 1:
        raise Failure("native_binary_missing_or_ambiguous")
    return binaries[0].resolve(strict=True)


def observe(artifact, approved, signers, *, cancellation=False):
    # Two closed compiled profiles. No caller-supplied URL, test name or budget.
    profile = cancellation_receipt if cancellation else receipt
    cancellation_claim = "owned_response_cancellation_verified" if cancellation else "active_http_cancellation_verified"
    def source_identity():
        result = identity(signers)
        if cancellation:
            for name, relative in (
                ("cancellation_runner", "scripts/test_native_collection_cancellation.py"),
                ("cancellation_validator", "scripts/native_cancellation_receipt.py"),
                ("cancellation_native_test", "crates/core/src/coordinator_native_https_cancellation.rs"),
                ("cancellation_gate", "crates/core/src/collection_transport/cancellation_probe.rs"),
                ("transport", "crates/core/src/collection_transport.rs"),
            ):
                result[name + "_sha256"] = digest(ROOT / relative)
        return result

    report = {"schema_version": 1, "policy": profile.POLICY, "passed": False,
              "outcome": "incomplete", "phase": "authorization", "events": [],
              "at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "nonce": artifact.name, "complete_release": False,
              "scope": {"host": "raw.githubusercontent.com", "seed": receipt.SEED,
                        "robots": receipt.ROBOTS, "fixture_sha256": receipt.FIXTURE_SHA},
              "limits": {"charged_attempts": 2, "transport_launches": 2, "hops": 0,
                         "cooperative_seconds": 20, "outer_native_seconds": 30,
                         "owned_process_stop_seconds": 5,
                         "offline_build_seconds": 600, "retries": 0},
              "claims": {"production_activation": False, "broad_discovery_benchmark": False,
                         "complete_dns_rrset": False, "provider_quiescence": False,
                         cancellation_claim: False, "windows_native": False}}
    if cancellation:
        report["proof_boundary"] = "owned_response_before_body_consumption"
        report["limits"]["response_handshake_seconds"] = 5
        report["claims"]["body_already_buffered_by_os_or_client_possible"] = True
        report["claims"]["complete_activation_gate"] = False
    binary = None
    native_log = artifact / "native.log"
    try:
        save(artifact, report)
        if not approved:
            raise Failure("explicit_fixed_cancellation_opt_in_required" if cancellation else "explicit_fixed_https_opt_in_required")
        if str(uuid.UUID(report["nonce"])) != report["nonce"]:
            raise Failure("noncanonical_campaign_nonce")
        report["phase"] = "platform"
        report["platform"] = {"os": platform.system(), "version": platform.mac_ver()[0],
                              "architecture": platform.machine()}
        if platform.system() != "Darwin":
            raise Failure("native_macos_required")
        report["phase"] = "source"
        report["source"] = source_identity()
        report["compiler"] = text_command(["rustc", "--version"])
        sdk = Path(text_command(["xcrun", "--show-sdk-path"]))
        report["dns_header_sha256"] = digest(sdk / "usr/include/dns_sd.h")
        if digest(ROOT / "fixtures/brief.txt") != receipt.FIXTURE_SHA:
            raise Failure("frozen_synthetic_fixture_changed")
        report["phase"] = "build"
        save(artifact, report)
        binary = build_binary(artifact, report["source"]["revision"])
        report["binary_sha256"] = digest(binary)
        if source_identity() != report["source"]:
            raise Failure("source_changed_before_native_run")
        report["phase"] = "native"
        save(artifact, report)
        environment = dict(os.environ, EW_NATIVE_HTTPS_PROOF=profile.POLICY,
                           EW_NATIVE_HTTPS_SOURCE=report["source"]["revision"],
                           EW_NATIVE_HTTPS_NONCE=report["nonce"])
        started = time.monotonic()
        try:
            report["native_exit_code"] = run_logged(
                [str(binary), cancellation_receipt.TEST if cancellation else TEST, "--exact", "--ignored", "--nocapture", "--test-threads=1"],
                native_log, 30, environment)
        finally:
            report["owned_process_elapsed_ms"] = round((time.monotonic() - started) * 1000)
        if not retain_events(report, native_log):
            raise Failure("partial_event_decode_failed")
        profile.validate(report["events"], report["source"]["revision"], report["nonce"], report["native_exit_code"])
        report.update(passed=True, outcome="passed", phase="complete")
    except Failure as error:
        report.update(outcome="failed", failure=str(error))
        if isinstance(error, ProcessTimeout):
            report["owned_process_termination"] = error.termination
        if str(error) == "outer_process_timeout":
            report["owned_resource_cleanup_proven"] = False
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        report.update(outcome="failed", failure="invalid_input_receipt_or_tool_failure")
    finally:
        if "source" in report:
            try:
                report["source_unchanged_after"] = source_identity() == report["source"]
                if not report["source_unchanged_after"]:
                    evidence_failure(report, "source_changed_during_observation")
            except (Failure, OSError, ValueError, subprocess.SubprocessError):
                evidence_failure(report, "post_run_source_identity_unavailable")
        if binary is not None and "binary_sha256" in report:
            try:
                report["binary_unchanged_after"] = digest(binary) == report["binary_sha256"]
                if not report["binary_unchanged_after"]:
                    evidence_failure(report, "binary_changed_during_observation")
            except OSError:
                evidence_failure(report, "post_run_binary_identity_unavailable")
        for name in ("build.log", "native.log"):
            try:
                path = artifact / name
                if path.is_file():
                    report.setdefault("logs", {})[name] = {"sha256": digest(path), "bytes": path.stat().st_size}
            except OSError:
                evidence_failure(report, "diagnostic_identity_unavailable")
        try:
            if native_log.is_file() and not report["events"] and "event_parse_error" not in report:
                retain_events(report, native_log)
        except (OSError, Failure, ValueError, KeyError, TypeError):
            evidence_failure(report, "partial_log_parse_failed")
        if cancellation:
            # Narrow claim derived only after source/binary post-verification.
            report["claims"][cancellation_claim] = report["passed"]
        try:
            save(artifact, report)
        except OSError:
            evidence_failure(report, "final_report_write_failed")
            report["claims"][cancellation_claim] = False
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-fixed-https", action="store_true")
    parser.add_argument("--allowed-signers", type=Path, required=True)
    args = parser.parse_args()
    artifact = ROOT / "artifacts/native-durable-https" / str(uuid.uuid4())
    artifact.mkdir(parents=True, mode=0o700)
    report = observe(artifact, args.allow_fixed_https, args.allowed_signers.resolve())
    print(json.dumps({"artifact_id": artifact.name, **{k: report[k] for k in
                     ("passed", "outcome", "phase", "failure") if k in report}}))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
