"""One opt-in macOS coordinator Search campaign; no worker launch without reviewed source."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import time
import uuid

from test_native_collection_transport import (Failure, digest, read_log, source_identity as shared_identity,
                                             text_command, save, evidence_failure)
from test_native_durable_https import run_logged, ProcessTimeout
import native_search_runtime as runtime_inventory
import native_search_receipt as receipt

ROOT = Path(__file__).resolve().parents[1]


def identity(signers):
    result = shared_identity(signers)
    result["shared_runner_sha256"] = result.pop("runner_sha256")
    result["runner_sha256"] = digest(Path(__file__))
    # All tracked source/build/profile inputs, including transitive local runner imports.
    tracked = text_command(["git", "ls-files", "--", "Cargo.toml", "Cargo.lock", ".cargo", "crates", "scripts", "workers"])
    files = sorted(name for name in tracked.splitlines() if Path(name).suffix in (".rs", ".py", ".java", ".toml", ".lock", ".sb"))
    if not files or len(files) > 2048:
        raise Failure("source_inventory_missing_or_excessive")
    result["files"] = [{"path": name, "sha256": digest(ROOT / name)} for name in files]
    encoded = b"".join(row["path"].encode() + b"\0" + row["sha256"].encode() + b"\0" for row in result["files"])
    result["files_sha256"] = hashlib.sha256(encoded).hexdigest()
    return result


def build_binary(artifact, source):
    environment = dict(os.environ, EW_NATIVE_SEARCH_BUILD_SOURCE=source)
    status = run_logged(["cargo", "test", "--locked", "--offline", "-p", "workbench-core", "--lib", "--no-run", "--message-format=json"],
                        artifact / "build.log", 600, environment)
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


def retain_events(report, log):
    try:
        report["events"] = receipt.decode(read_log(log))
        return True
    except receipt.EventDecodeFailure as error:
        report["events"] = error.events
        report["event_parse_error"] = str(error)
        evidence_failure(report, "partial_event_decode_failed")
        return False


def observe(artifact, approved, signers, runtime, expected_sha):
    report = {"schema_version": 1, "policy": receipt.POLICY, "passed": False,
              "outcome": "not_started", "phase": "authorization", "events": [],
              "at": datetime.datetime.now(datetime.timezone.utc).isoformat(), "nonce": artifact.name,
              "limits": {"recipe_entries": 5, "worker_seconds": 30, "outer_native_seconds": 180,
                         "offline_build_seconds": 600, "owned_process_stop_seconds": 5, "retries": 0},
              "claims": {"complete_release": False, "windows_native": False, "intel_native": False,
                         "ranked_benchmark": False, "cold_corpus_measurement": False,
                         "recipe_entries_are_spawn_evidence": False, "rebuilt_from_current_java_source": False},
              "runtime_postread_permitted": False}
    binary = None
    native_log = artifact / "native.log"
    try:
        save(artifact, report)  # Failure receipt exists before metadata, build or any worker launch.
        if not approved:
            raise Failure("explicit_reviewed_search_opt_in_required")
        if str(uuid.UUID(report["nonce"])) != report["nonce"] or not receipt.sha(expected_sha):
            raise Failure("invalid_campaign_nonce_or_runtime_pin")
        report["phase"] = "platform"
        report["platform"] = {"os": platform.system(), "version": platform.mac_ver()[0], "architecture": platform.machine()}
        if platform.system() != "Darwin" or platform.machine() != "arm64":
            raise Failure("native_macos_arm64_required")
        report["phase"] = "source"
        report["source"] = identity(signers)
        report["compiler"] = text_command(["rustc", "--version"])
        report["phase"] = "runtime_inventory"
        report["runtime_before"] = runtime_inventory.inventory(runtime)
        if report["runtime_before"]["sha256"] != expected_sha:
            raise Failure("selected_runtime_pin_mismatch")
        report["phase"] = "build"
        save(artifact, report)
        binary = build_binary(artifact, report["source"]["revision"])
        report["binary_sha256"] = digest(binary)
        if identity(signers) != report["source"]:
            raise Failure("source_changed_before_native_run")
        report["phase"] = "native"
        report["outcome"] = "incomplete"
        save(artifact, report)
        environment = dict(os.environ, EW_NATIVE_SEARCH_PROOF=receipt.POLICY,
                           EW_NATIVE_SEARCH_SOURCE=report["source"]["revision"], EW_NATIVE_SEARCH_NONCE=report["nonce"],
                           EW_NATIVE_SEARCH_RUNTIME=str(runtime), EW_NATIVE_SEARCH_RUNTIME_SHA256=expected_sha)
        # Exact private invocation retained separately; absolute paths are not public receipt fields.
        invocation = {"command": [str(binary), receipt.TEST, "--exact", "--ignored", "--nocapture", "--test-threads=1"],
                      "runtime": str(runtime), "source": report["source"]["revision"], "nonce": report["nonce"]}
        with (artifact / "invocation.json").open("x") as stream:
            json.dump(invocation, stream, sort_keys=True, indent=2)
        started = time.monotonic()
        try:
            report["native_exit_code"] = run_logged(invocation["command"], native_log, 180, environment)
        finally:
            report["native_elapsed_ms"] = round((time.monotonic() - started) * 1000)
        if not retain_events(report, native_log):
            raise Failure("partial_event_decode_failed")
        report["fixed_outcomes"] = receipt.validate(report["events"], report["source"]["revision"], report["nonce"],
                                                    report["runtime_before"], report["native_exit_code"])
        # Only a complete, strictly accepted native receipt permits dependent reads here.
        report["runtime_postread_permitted"] = True
        report["runtime_after"] = runtime_inventory.inventory(runtime)
        if report["runtime_after"] != report["runtime_before"]:
            raise Failure("selected_runtime_changed")
        report.update(passed=True, outcome="passed", phase="complete")
    except ProcessTimeout as error:
        report.update(outcome="failed", failure="outer_process_timeout", owned_process_termination=error.termination,
                      java_termination="unverified", runtime_postread_permitted=False)
        # The JVM owns a different process group. Killing/reaping the Rust test is NOT JVM proof.
    except (Failure, receipt.InvalidReceipt, runtime_inventory.InvalidRuntime) as error:
        report.update(outcome="failed", failure=str(error))
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        report.update(outcome="failed", failure="invalid_input_receipt_or_tool_failure")
    finally:
        # These are host source/binary/logs, never worker-owned paths or runtime postreads.
        if "source" in report:
            try:
                report["source_unchanged_after"] = identity(signers) == report["source"]
                if not report["source_unchanged_after"]:
                    evidence_failure(report, "source_changed_during_observation")
            except (Failure, OSError, ValueError, subprocess.SubprocessError):
                evidence_failure(report, "post_source_identity_unavailable")
        if binary is not None:
            try:
                report["binary_unchanged_after"] = digest(binary) == report["binary_sha256"]
                if not report["binary_unchanged_after"]:
                    evidence_failure(report, "binary_changed_during_observation")
            except (OSError, KeyError):
                evidence_failure(report, "post_binary_identity_unavailable")
        for name in ("build.log", "native.log", "invocation.json"):
            try:
                path = artifact / name
                if path.is_file():
                    report.setdefault("logs", {})[name] = {"sha256": digest(path), "bytes": path.stat().st_size}
            except OSError:
                evidence_failure(report, "diagnostic_identity_unavailable")
        if native_log.is_file() and not report["events"] and "event_parse_error" not in report:
            try:
                retain_events(report, native_log)
            except (OSError, Failure, ValueError):
                evidence_failure(report, "partial_log_parse_failed")
        try:
            save(artifact, report)
        except OSError:
            evidence_failure(report, "final_report_write_failed")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execute-reviewed-search", action="store_true")
    parser.add_argument("--allowed-signers", type=Path, required=True)
    parser.add_argument("--runtime", type=Path, required=True)
    parser.add_argument("--runtime-sha256", required=True)
    args = parser.parse_args()
    parent = ROOT / "artifacts/native-coordinator-search"
    parent.mkdir(parents=True, mode=0o700, exist_ok=True)
    runtime_inventory.ordinary_root(parent)
    artifact = parent / str(uuid.uuid4())
    artifact.mkdir(mode=0o700)
    report = observe(artifact, args.execute_reviewed_search, args.allowed_signers, args.runtime, args.runtime_sha256)
    print(json.dumps({"artifact_id": artifact.name, **{key: report[key] for key in ("passed", "phase", "failure") if key in report}}))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
