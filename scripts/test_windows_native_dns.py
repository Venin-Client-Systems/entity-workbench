"""Prepare/run an explicitly opted-in Windows-only GetAddrInfoExW evidence campaign."""
import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import subprocess
import time
import uuid

from test_native_collection_transport import (
    Failure, digest, read_log, source_identity as shared_source_identity,
    text_command, save, evidence_failure,
)
import windows_dns_receipt as receipt

ROOT = Path(__file__).resolve().parents[1]
TEST = "collection_transport::resolver::native_windows_proof::native_windows_dns_campaign"


def identity(signers):
    value = shared_source_identity(signers)
    value["shared_identity_helper_sha256"] = value.pop("runner_sha256")
    value["runner_sha256"] = digest(Path(__file__))
    value["receipt_validator_sha256"] = digest(ROOT / "scripts/windows_dns_receipt.py")
    return value


def run_logged(command, log, timeout, environment, *, build=False):
    with log.open("xb") as stream:
        process = subprocess.Popen(command, cwd=ROOT, env=environment,
                                   stdout=stream, stderr=subprocess.STDOUT)
        try:
            return process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            if build:
                # Cargo can own compiler children. This invokes the local OS tool
                # only for our still-owned PID; it is not a network subprocess.
                subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                               timeout=10, check=False)
            else:
                # The libtest probe creates no child processes. Shared OS DNS
                # providers are outside this owned process and are not claimed stopped.
                process.kill()
            process.wait(timeout=5)
            raise Failure("owned_process_timeout") from None


def build_binary(artifact, source):
    environment = dict(os.environ, EW_WINDOWS_DNS_BUILD_SOURCE=source)
    result = run_logged(["cargo", "test", "--locked", "--offline", "-p", "workbench-core",
                         "--lib", "--no-run", "--message-format=json"], artifact / "build.log",
                        600, environment, build=True)
    if result:
        raise Failure("native_build_failed")
    candidates = []
    for line in read_log(artifact / "build.log").splitlines():
        try:
            value = json.loads(line)
        except ValueError:
            continue
        if (value.get("reason") == "compiler-artifact" and value.get("executable")
                and value.get("profile", {}).get("test") is True
                and value.get("target", {}).get("name") == "workbench_core"):
            candidates.append(Path(value["executable"]))
    if len(candidates) != 1:
        raise Failure("native_binary_missing_or_ambiguous")
    return candidates[0].resolve(strict=True)


def observe(artifact, allow_fixed_dns, signers):
    report = {"schema_version": 1, "policy": receipt.POLICY, "passed": False,
              "outcome": "incomplete", "phase": "authorization", "cases": [],
              "observed_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "nonce": str(uuid.uuid4()), "complete_release": False,
              "limits": {"native_launches": 3, "http_requests": 0, "retries": 0,
                         "offline_build_seconds": 600,
                         "cooperative_campaign_seconds": 20, "outer_native_process_seconds": 30,
                         "dns_stage_seconds": 5, "cancel_cleanup_seconds": 1},
              "claims": {"provider_quiescence": False, "windows_11_acceptance": False,
                         "complete_rrset": False, "live_coordinator_activation": False}}
    binary = None
    native_log = artifact / "native.log"
    try:
        save(artifact, report)
        if not allow_fixed_dns:
            raise Failure("explicit_fixed_dns_opt_in_required")
        report["phase"] = "platform"
        report["platform"] = {"os": platform.system(), "version": platform.version(),
                              "edition": platform.win32_edition(), "architecture": platform.machine()}
        if platform.system() != "Windows":
            raise Failure("native_windows_required")
        report["phase"] = "source_identity"
        report["source"] = identity(signers)
        report["compiler"] = text_command(["rustc", "--version"])
        report["phase"] = "build"
        save(artifact, report)
        binary = build_binary(artifact, report["source"]["revision"])
        report["binary_sha256"] = digest(binary)
        if identity(signers) != report["source"]:
            raise Failure("source_changed_before_native_run")
        report["phase"] = "native_campaign"
        save(artifact, report)
        environment = dict(os.environ, EW_WINDOWS_DNS_PROOF=receipt.POLICY,
                           EW_WINDOWS_DNS_SOURCE=report["source"]["revision"],
                           EW_WINDOWS_DNS_NONCE=report["nonce"])
        started = time.monotonic()
        try:
            report["native_exit_code"] = run_logged(
                [str(binary), TEST, "--exact", "--ignored", "--nocapture", "--test-threads=1"],
                native_log, 30, environment)
        finally:
            report["native_process_elapsed_ms"] = round((time.monotonic() - started) * 1000)
        report["cases"] = receipt.decode(read_log(native_log))
        receipt.validate(report["cases"], report["source"]["revision"], report["nonce"], report["native_exit_code"])
        report.update(passed=True, outcome="passed", phase="complete")
    except Failure as error:
        report.update(outcome="failed", failure=str(error))
        if str(error) == "owned_process_timeout" and report["phase"] == "native_campaign":
            report["native_process_forcibly_stopped"] = True
            report["caller_context_completion_proven"] = False
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        report.update(outcome="failed", failure="invalid_input_receipt_or_tool_failure")
    finally:
        if "source" in report:
            try:
                report["source_unchanged_after"] = identity(signers) == report["source"]
                if not report["source_unchanged_after"]:
                    evidence_failure(report, "source_changed_during_observation")
            except (OSError, ValueError, Failure, subprocess.SubprocessError):
                evidence_failure(report, "post_run_source_identity_unavailable")
        if binary is not None and "binary_sha256" in report:
            try:
                report["binary_unchanged_after"] = digest(binary) == report["binary_sha256"]
                if not report["binary_unchanged_after"]:
                    evidence_failure(report, "binary_changed_during_observation")
            except OSError:
                evidence_failure(report, "post_run_binary_identity_unavailable")
        for name in ("build.log", "native.log"):
            path = artifact / name
            try:
                if path.is_file():
                    report.setdefault("logs", {})[name] = {"sha256": digest(path), "bytes": path.stat().st_size}
            except OSError:
                evidence_failure(report, "diagnostic_identity_unavailable")
        try:
            if native_log.is_file() and not report["cases"]:
                report["cases"] = receipt.decode(read_log(native_log))
        except (OSError, ValueError, Failure):
            evidence_failure(report, "partial_log_parse_failed")
        try:
            save(artifact, report)
        except OSError:
            evidence_failure(report, "final_report_write_failed")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-fixed-dns", action="store_true")
    parser.add_argument("--allowed-signers", type=Path, required=True)
    args = parser.parse_args()
    artifact = ROOT / "artifacts/windows-native-dns" / uuid.uuid4().hex
    artifact.mkdir(parents=True)
    report = observe(artifact, args.allow_fixed_dns, args.allowed_signers.resolve())
    print(json.dumps({"artifact_id": artifact.name, **{key: report[key] for key in
                     ("passed", "outcome", "phase", "failure") if key in report}}, indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
