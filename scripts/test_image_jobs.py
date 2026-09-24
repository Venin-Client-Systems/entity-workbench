"""Verify durable image OCR publication/restore and legacy parser coordination on this Mac."""
import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import uuid
import test_image_workers as image_runner

ROOT = Path(__file__).resolve().parents[1]
SOURCES = sorted(set(image_runner.SOURCES + [
    "crates/core/src/processing.rs", "crates/core/src/coordinator.rs",
    "crates/core/src/coordinator_image_tests.rs", "crates/core/src/domain.rs",
    "crates/core/src/store.rs", "crates/core/src/store/processing.rs",
    "crates/core/src/store/processing_publication.rs", "crates/core/src/store/processing_image_tests.rs",
    "crates/core/src/store/processing_image_demo.rs", "crates/core/src/store/processing_image_fixtures.rs",
    "crates/core/src/store/processing_demo.rs", "crates/core/src/bin/ew-dev.rs",
    "crates/core/src/engines/parser.rs", "scripts/test_image_jobs.py",
    "schemas/processing-job.v1.schema.json", "schemas/processing-job.v2.schema.json",
    "schemas/extraction.v1.schema.json", "schemas/image-extraction.v1.schema.json", "schemas/command.v7.schema.json",
]))
EXPECTED = {
    "executor_panic_cannot_claim_worker_exit_or_start_queued_work",
    "image_and_parse_share_two_slots_and_joined_shutdown",
    "native_coordinator_images_publish_and_restore_recognition_and_provenance",
    "native_coordinator_publishes_and_restores_pdf_derivative",
    "responsive_cancellation_stops_execution_before_terminal_state_and_suppresses_success",
    "shutdown_between_claim_and_registration_does_not_launch_a_worker",
    "worker_pool_is_bounded_and_shutdown_reaps_all_active_executors",
    "canonical_image_acceptance_rejects_wrong_raster_recognition_and_operation",
    "fixed_image_review_fixture_is_canonical_and_never_overwrites",
    "image_cancellation_and_unverified_exit_keep_shared_attempt_and_suspension_rules",
    "image_publication_is_atomic_replayable_and_keeps_original_text_unmodified",
    "image_request_keys_bind_operation_and_preserve_parse_v1_schema",
    "typed_image_outcomes_and_retry_history_survive_backup_without_rasters",
}


def observe(runtime, parser_runtime):
    report = {
        "schema_version": 1, "observed_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scope": "development Mac; actual durable PNG/JPEG OCR, PDF parser regression and synthetic state/publication tests",
        "platform": {"os": platform.system(), "version": platform.mac_ver()[0], "architecture": platform.machine()},
        "phase": "source_identity", "tests": {}, "passed": False, "outcome": "failed", "complete_release": False,
        "unverified": ["signed helpers", "Intel Mac and Windows runtime integration", "clean installed package",
                       "PDF rendering and OCR", "word/source-region acceptance", "retained raster derivatives",
                       "recognition accuracy benchmark", "supervisor-crash process termination"],
    }
    diagnostics = ""
    try:
        report.update(source_revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                      source_dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
                      source_sha256={name: image_runner.digest(ROOT / name) for name in SOURCES})
        report["phase"] = "runtime_inventory"
        if platform.system() != "Darwin":
            raise image_runner.VerificationFailure("unsupported_native_platform")
        runtime = runtime.resolve(strict=True)
        parser_runtime = parser_runtime.resolve(strict=True)
        report["image_runtime"] = image_runner.inspect_runtime(runtime)
        report["parser_runtime_sha256"] = {str(path.relative_to(parser_runtime)): image_runner.digest(path)
                                            for path in sorted((parser_runtime / "parser").rglob("*.jar"))}
        report["parser_java_sha256"] = image_runner.digest(parser_runtime / "java/lib/modules")
        report["fixture_sha256"] = {name: image_runner.digest(ROOT / name) for name in [
            "fixtures/images/synthetic.png", "fixtures/images/synthetic.jpg", "fixtures/ocr/synthetic.pgm",
            "fixtures/images/unsupported.gif", "fixtures/parser/notice.pdf"]}
        report["phase"] = "native_tests"
        environment = dict(os.environ, WORKBENCH_TEST_IMAGE_RUNTIME=str(runtime), WORKBENCH_TEST_RUNTIME=str(parser_runtime))
        command = ["cargo", "test", "--locked", "-p", "workbench-core", "--lib", "--", "coordinator::tests::", "coordinator::image_tests::",
                   "store::processing::image_tests::", "--include-ignored", "--test-threads=1"]
        result = subprocess.run(command, cwd=ROOT, env=environment, capture_output=True, text=True, timeout=240)
        diagnostics = result.stdout + result.stderr
        report["tests"] = dict(re.findall(r"test (?:\w+::)+(\w+) \.\.\. (ok|FAILED|ignored)", result.stdout))
        if result.returncode != 0 or set(report["tests"]) != EXPECTED or any(value != "ok" for value in report["tests"].values()):
            raise image_runner.VerificationFailure("native_test_failure_or_incomplete_suite")
        report.update(passed=True, outcome="passed", phase="complete")
    except (image_runner.VerificationFailure, image_runner.ocr_verifier.VerificationFailure) as error:
        report["failure"] = str(error)
    except subprocess.TimeoutExpired:
        report["failure"] = "tool_timeout"
    except subprocess.CalledProcessError:
        report["failure"] = "tool_failed"
    except (OSError, ValueError, KeyError, TypeError):
        report["failure"] = "missing_or_invalid_verification_input"
    except Exception:
        report["failure"] = "unexpected_verification_failure"
    return report, diagnostics


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, required=True)
    parser.add_argument("--parser-runtime", type=Path, required=True)
    args = parser.parse_args()
    report, diagnostics = observe(args.runtime, args.parser_runtime)
    artifacts = ROOT / "artifacts/image-jobs"
    artifacts.mkdir(parents=True, exist_ok=True)
    timestamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    encoded = json.dumps(report, indent=2) + "\n"
    with (artifacts / f"{timestamp}-{uuid.uuid4().hex}.json").open("x") as stream:
        stream.write(encoded)
    (ROOT / "artifacts/image-jobs-result.json").write_text(encoded)
    (ROOT / "artifacts/image-jobs-test-output.txt").write_text(diagnostics)
    print(json.dumps({key: report[key] for key in ("source_revision", "source_dirty", "platform", "tests", "phase", "outcome", "failure", "passed", "complete_release") if key in report}, indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
