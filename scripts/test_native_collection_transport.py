"""Opt-in fixed Mac DNS evidence; never sends HTTP or activates collection jobs."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import uuid

ROOT = Path(__file__).resolve().parents[1]
TEST = "collection_transport::resolver::platform::native_proof::native_macos_dns_campaign"
OPT_IN = "fixed-three-subscriptions-no-http-v2"
PREFIX = "EW_NATIVE_DNS_CASE="
CASES = ["transport_pre_cancel", "transport_expired", "native_success",
         "native_negative", "native_active_deadline"]
LIMITS = {"native_subscriptions": 3, "http_requests": 0,
          "cooperative_campaign_seconds": 20, "outer_native_process_seconds": 30,
          "dns_stage_seconds": 5, "retries": 0}


class Failure(Exception):
    pass


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def text_command(command):
    return subprocess.check_output(command, cwd=ROOT, text=True,
                                   stderr=subprocess.DEVNULL, timeout=20).strip()


def source_identity(signers):
    if text_command(["git", "status", "--porcelain", "--untracked-files=normal"]):
        raise Failure("source_not_clean")
    text_command(["git", "-c", f"gpg.ssh.allowedSignersFile={signers}",
                  "verify-commit", "HEAD"])
    return {"revision": text_command(["git", "rev-parse", "HEAD"]),
            "tree": text_command(["git", "rev-parse", "HEAD^{tree}"]),
            "signature_verified": True, "allowed_signers_sha256": digest(signers),
            "runner_sha256": digest(Path(__file__)),
            "lockfile_sha256": digest(ROOT / "Cargo.lock")}


def run_logged(command, log, timeout, environment=None):
    # A unique local output retains partial observations on failure. Kill/join the
    # whole owned process group on the outer bound; this is not DNS-daemon proof.
    with log.open("xb") as stream:
        process = subprocess.Popen(command, cwd=ROOT, env=environment,
                                   stdout=stream, stderr=subprocess.STDOUT,
                                   start_new_session=True)
        try:
            return process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=5)
            raise Failure("outer_process_timeout") from None


def read_log(path):
    with path.open("rb") as stream:
        data = stream.read(4 * 1024 * 1024 + 1)
    if len(data) > 4 * 1024 * 1024:
        raise Failure("diagnostic_output_limit")
    return data.decode("utf-8", errors="replace")


def build_binary(artifact):
    log = artifact / "build.log"
    result = run_logged(["cargo", "test", "--locked", "--offline", "-p", "workbench-core",
                         "--lib", "--no-run", "--message-format=json"], log, 180)
    if result:
        raise Failure("native_build_failed")
    binaries = []
    for line in read_log(log).splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if (event.get("reason") == "compiler-artifact" and event.get("executable")
                and event.get("profile", {}).get("test") is True
                and event.get("target", {}).get("name") == "workbench_core"):
            binaries.append(Path(event["executable"]))
    if len(binaries) != 1:
        raise Failure("native_test_binary_missing_or_ambiguous")
    return binaries[0].resolve(strict=True)


def decode_cases(output):
    events = []
    for line in output.splitlines():
        if PREFIX in line:
            # libtest may put the first marker after its `test ...` prefix.
            event = json.loads(line.split(PREFIX, 1)[1])
            events.append(event)
    return events


def validate_cases(events, exit_code):
    if exit_code != 0 or [event.get("case") for event in events] != CASES:
        raise Failure("native_failure_or_incomplete_campaign")
    for index, event in enumerate(events):
        subscriptions = 0 if index < 2 else 1
        probe = event.get("probe", {})
        if (event.get("passed") is not True
                or event.get("hostname") != ("ew-native-proof.invalid" if index == 3 else "example.com")
                or any(probe.get(key) != subscriptions for key in ("attempted", "created", "deallocated"))):
            raise Failure("native_case_or_owned_cleanup_failed")
    if events[2]["outcome"].get("authoritative_complete_set") is not False:
        raise Failure("invalid_complete_rrset_claim")
    if not any(error == -65554 for _, error in events[3]["probe"]["callbacks"]):
        raise Failure("negative_dns_outcome_not_observed")


def save(artifact, report):
    temporary = artifact / "report.pending.json"
    temporary.write_text(json.dumps(report, indent=2) + "\n")
    temporary.replace(artifact / "report.json")


def evidence_failure(report, reason):
    if report["passed"]:
        report["phase"] = "post_run_evidence"
    report["passed"] = False
    report["outcome"] = "failed"
    report.setdefault("failure", reason)
    report.setdefault("evidence_failures", []).append(reason)


def observe(artifact, allow_fixed_dns, signers):
    report = {"schema_version": 1, "campaign_policy": OPT_IN,
              "passed": False, "outcome": "incomplete",
              "observed_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "phase": "authorization", "limits": LIMITS, "cases": [],
              "https": {"state": "not_attempted", "reason": "reviewed_access_not_established"},
              "windows_native": "not_run", "complete_release": False,
              "claims": {"complete_dns_rrset": False, "dns_daemon_quiescence": False,
                         "uncached_wire_dns": False, "durable_live_activation": False}}
    # Persist before platform, source, signer or build lookup, so a metadata failure
    # cannot leave a previous successful run looking like this invocation's result.
    native_log = artifact / "native.log"
    binary = None
    try:
        save(artifact, report)
        if not allow_fixed_dns:
            raise Failure("explicit_fixed_dns_opt_in_required")
        report["phase"] = "platform"
        report["platform"] = {"os": platform.system(), "version": platform.mac_ver()[0],
                              "architecture": platform.machine()}
        if platform.system() != "Darwin":
            raise Failure("native_macos_required")
        report["phase"] = "source_identity"
        report["source"] = source_identity(signers)
        report["compiler"] = text_command(["rustc", "--version"])
        sdk = Path(text_command(["xcrun", "--show-sdk-path"]))
        report["dns_sd_header_sha256"] = digest(sdk / "usr/include/dns_sd.h")
        report["phase"] = "build"
        save(artifact, report)
        binary = build_binary(artifact)
        report["test_binary_sha256"] = digest(binary)
        if source_identity(signers) != report["source"]:
            raise Failure("source_changed_before_native_run")
        report["phase"] = "native_campaign"
        save(artifact, report)
        environment = dict(os.environ, EW_NATIVE_DNS_PROOF=OPT_IN)
        report["native_exit_code"] = run_logged(
            [str(binary), TEST, "--exact", "--ignored", "--nocapture", "--test-threads=1"],
            native_log, LIMITS["outer_native_process_seconds"], environment)
        report["cases"] = decode_cases(read_log(native_log))
        validate_cases(report["cases"], report["native_exit_code"])
        report.update(passed=True, outcome="passed", phase="complete")
    except Failure as error:
        report.update(outcome="failed", failure=str(error))
    except (subprocess.SubprocessError, OSError, ValueError, KeyError, TypeError):
        report.update(outcome="failed", failure="missing_invalid_input_or_tool_failure")
    finally:
        # Check even failed/partial native runs. Preserve the first failure while
        # separately recording any source/evidence problem encountered afterwards.
        if "source" in report:
            try:
                report["source_unchanged_after"] = source_identity(signers) == report["source"]
                if not report["source_unchanged_after"]:
                    evidence_failure(report, "source_changed_during_observation")
            except (Failure, subprocess.SubprocessError, OSError, ValueError):
                evidence_failure(report, "post_run_source_identity_unavailable")
        if binary is not None and "test_binary_sha256" in report:
            try:
                report["binary_unchanged_after"] = digest(binary) == report["test_binary_sha256"]
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
                report["cases"] = decode_cases(read_log(native_log))
        except (OSError, Failure, ValueError, KeyError, TypeError):
            evidence_failure(report, "partial_log_parse_failed")
        try:
            save(artifact, report)
        except OSError:
            # The last complete report remains incomplete/failed if replacement
            # fails. The returned/printed result must never claim success either.
            evidence_failure(report, "final_report_write_failed")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-fixed-dns", action="store_true")
    parser.add_argument("--allowed-signers", type=Path, required=True)
    args = parser.parse_args()
    artifact = ROOT / "artifacts/native-collection-transport" / uuid.uuid4().hex
    artifact.mkdir(parents=True)
    report = observe(artifact, args.allow_fixed_dns, args.allowed_signers.resolve())
    print(json.dumps({"artifact_id": artifact.name, **{key: report[key] for key in
                     ("passed", "outcome", "phase", "failure") if key in report}}, indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
