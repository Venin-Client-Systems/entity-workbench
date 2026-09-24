"""Verify durable selected-page PDF OCR jobs against the actual app-local Mac workers."""
import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import uuid

import test_pdf_workers as pdf_runner

ROOT = Path(__file__).resolve().parents[1]
SOURCES = sorted(set(pdf_runner.SOURCES + [
    "crates/core/src/lib.rs", "crates/core/src/processing.rs",
    "crates/core/src/coordinator.rs", "crates/core/src/coordinator_pdf_tests.rs",
    "crates/core/src/domain.rs", "crates/core/src/store.rs",
    "crates/core/src/store/processing.rs", "crates/core/src/store/processing_publication.rs",
    "crates/core/src/store/processing_pdf_publication.rs",
    "crates/core/src/store/processing_pdf_fixtures.rs",
    "crates/core/src/store/processing_pdf_demo.rs",
    "crates/core/src/store/processing_pdf_tests.rs", "crates/core/src/bin/ew-dev.rs",
    "schemas/command.v9.schema.json", "schemas/processing-job.v1.schema.json",
    "schemas/processing-job.v2.schema.json", "schemas/processing-job.v3.schema.json",
    "schemas/extraction.v1.schema.json", "schemas/image-extraction.v1.schema.json",
    "schemas/pdf-extraction.v1.schema.json", "scripts/test_pdf_jobs.py",
    "scripts/test_pdf_jobs_evidence.py",
]))
EXPECTED = {
    "pdf_request_identity_binds_page_dpi_operation_and_preserves_legacy_versions",
    "pdf_publication_is_atomic_replayable_and_keeps_original_text_and_raster_policy",
    "pdf_acceptance_rejects_page_dpi_raster_recognition_and_operation_tampering",
    "pdf_typed_outcomes_and_prior_attempts_survive_backup_without_raster_bytes",
    "pdf_cancellation_stale_leases_and_unverified_exit_preserve_shared_precedence",
    "pdf_changed_original_never_publishes_and_fixed_review_seed_never_overwrites",
    "pdf_and_legacy_jobs_share_two_slots_and_joined_shutdown",
    "pdf_running_cancellation_and_panic_do_not_publish_or_hide_unknown_exit",
    "native_pdf_jobs_publish_selected_pages_and_restore_exact_unreviewed_provenance",
}


def native_tests(runtime, report):
    report["phase"] = "native_tests"
    environment = dict(os.environ, WORKBENCH_TEST_PDF_RUNTIME=str(runtime))
    command = [
        "cargo", "test", "--locked", "-p", "workbench-core", "--lib", "--",
        "coordinator::pdf_tests::", "store::processing::pdf_tests::",
        "--include-ignored", "--test-threads=1",
    ]
    return subprocess.run(command, cwd=ROOT, env=environment, capture_output=True,
                          text=True, timeout=240)


def observe(runtime):
    report = {
        "schema_version": 1,
        "observed_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scope": "development Mac; actual durable selected-page PDF OCR, typed outcomes and restore; synthetic state/publication tests",
        "platform": {"os": platform.system(), "version": platform.mac_ver()[0],
                     "architecture": platform.machine()},
        "phase": "source_identity", "tests": {}, "passed": False, "outcome": "failed",
        "complete_release": False,
        "unverified": ["signed helpers", "Intel Mac and Windows runtime integration",
                       "clean installed package", "general PDFs and font rendering",
                       "word/source-region acceptance", "retained raster derivatives",
                       "recognition accuracy benchmark", "supervisor-crash process termination"],
    }
    diagnostics = ""
    try:
        report.update(
            source_revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
            source_dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
            source_sha256={name: pdf_runner.digest(ROOT / name) for name in SOURCES},
        )
        report["phase"] = "platform"
        if platform.system() != "Darwin":
            raise pdf_runner.VerificationFailure("unsupported_native_platform")
        report["phase"] = "runtime_inventory"
        runtime = runtime.resolve(strict=True)
        report["runtime"] = pdf_runner.inspect_runtime(runtime)
        report["fixtures_sha256"] = {
            path.name: pdf_runner.digest(path)
            for path in sorted((ROOT / "fixtures/pdf-render").iterdir())
            if not path.name.startswith(".")
        }
        result = native_tests(runtime, report)
        diagnostics = result.stdout + result.stderr
        report["tests"] = dict(re.findall(
            r"test (?:\w+::)+(\w+) \.\.\. (ok|FAILED|ignored)", result.stdout))
        if (result.returncode != 0 or set(report["tests"]) != EXPECTED
                or any(value != "ok" for value in report["tests"].values())):
            raise pdf_runner.VerificationFailure("native_test_failure_or_incomplete_suite")
        report.update(passed=True, outcome="passed", phase="complete")
    except (pdf_runner.VerificationFailure, pdf_runner.ocr_verifier.VerificationFailure) as error:
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


def save(report, diagnostics):
    artifacts = ROOT / "artifacts/pdf-jobs"
    artifacts.mkdir(parents=True, exist_ok=True)
    timestamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    encoded = json.dumps(report, indent=2) + "\n"
    with (artifacts / f"{timestamp}-{uuid.uuid4().hex}.json").open("x") as stream:
        stream.write(encoded)
    (ROOT / "artifacts/pdf-jobs-result.json").write_text(encoded)
    (ROOT / "artifacts/pdf-jobs-test-output.txt").write_text(diagnostics)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, required=True)
    report, diagnostics = observe(parser.parse_args().runtime)
    save(report, diagnostics)
    print(json.dumps({key: report[key] for key in (
        "source_revision", "source_dirty", "platform", "tests", "phase", "outcome",
        "failure", "passed", "complete_release") if key in report}, indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
