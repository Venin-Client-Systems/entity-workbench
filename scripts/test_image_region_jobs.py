"""Verify durable retained image OCR word-region jobs against the actual app-local Mac workers."""
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
import test_ocr_regions as region_runner

ROOT = Path(__file__).resolve().parents[1]
SOURCES = sorted(set(image_runner.SOURCES + region_runner.SOURCES + [
    "crates/core/src/lib.rs", "crates/core/src/processing.rs", "crates/core/src/processing_regions.rs",
    "crates/core/src/coordinator.rs", "crates/core/src/coordinator_region_tests.rs",
    "crates/core/src/domain.rs", "crates/core/src/store.rs", "crates/core/src/engines/image_regions.rs",
    "crates/core/src/store/processing.rs", "crates/core/src/store/processing_publication.rs",
    "crates/core/src/store/processing_regions.rs", "crates/core/src/store/derivative_files.rs",
    "crates/core/src/store/recovery.rs", "crates/core/src/store/processing_region_tests.rs",
    "crates/core/src/bin/ew-dev.rs", "crates/core/src/store/processing_image_fixtures.rs",
    "fixtures/ocr/synthetic.pgm", "schemas/command.v10.schema.json",
    "schemas/processing-job.v4.schema.json", "schemas/image-region-extraction.v1.schema.json",
    "schemas/image-region-result.v1.schema.json", "schemas/image-region-inspection.v1.schema.json",
    "scripts/test_image_region_jobs.py", "scripts/test_image_region_jobs_evidence.py",
]))
EXPECTED = {
    "region_missing_runtime_is_visible_and_worker_pool_remains_bounded",
    "native_image_regions_retain_exact_provenance_raster_tsv_and_restore",
    "region_jobs_opt_in_identity_and_immutable_files_survive_restore",
    "region_publication_rollback_leaves_only_unreferenced_verified_objects",
    "region_files_and_original_tampering_block_reads_backup_and_restore",
    "region_cancellation_and_invalid_results_publish_no_objects",
    "region_existing_published_corruption_is_not_repaired_and_orphan_conflicts_are_distinct",
    "region_empty_and_rejected_results_and_retry_history_remain_distinct",
    "region_completed_replay_rejects_changed_raster_and_tsv_bytes",
    "region_backup_uses_snapshot_catalog_despite_concurrent_publication",
    "region_incomplete_or_mismatched_backup_manifest_never_publishes_database",
    "region_links_and_oversized_objects_fail_without_modifying_outside_sentinels",
    "backup_uses_snapshot_references_after_another_canonical_writer_commits",
    "schema_three_upgrade_backs_up_and_failed_upgrade_rolls_back_catalog",
}


def native_tests(runtime, report):
    report["phase"] = "native_tests"
    environment = dict(os.environ, WORKBENCH_TEST_IMAGE_RUNTIME=str(runtime))
    command = [
        "cargo", "test", "--locked", "-p", "workbench-core", "--lib", "--",
        "coordinator::region_tests::", "store::processing::region_tests::", "store::recovery::tests::",
        "--include-ignored", "--test-threads=1",
    ]
    return subprocess.run(command, cwd=ROOT, env=environment, capture_output=True,
                          text=True, timeout=240)


def observe(runtime):
    report = {
        "schema_version": 1,
        "observed_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scope": "development Mac; actual durable PNG/JPEG word-region OCR, retained verified CAS and restore; synthetic publication/recovery tests",
        "platform": {"os": platform.system(), "version": platform.mac_ver()[0],
                     "architecture": platform.machine()},
        "phase": "source_identity", "tests": {}, "passed": False, "outcome": "failed",
        "complete_release": False,
        "unverified": ["signed helpers", "Intel Mac and Windows runtime integration",
                       "clean installed package", "PDF word-region jobs",
                       "word/source-region acceptance", "aggregate disk quota and orphan garbage collection",
                       "recognition accuracy benchmark", "supervisor-crash process termination"],
    }
    diagnostics = ""
    try:
        report.update(
            source_revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
            source_dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
            source_sha256={name: image_runner.digest(ROOT / name) for name in SOURCES},
        )
        report["phase"] = "platform"
        if platform.system() != "Darwin":
            raise image_runner.VerificationFailure("unsupported_native_platform")
        report["phase"] = "runtime_inventory"
        runtime = runtime.resolve(strict=True)
        report["runtime"] = image_runner.inspect_runtime(runtime)
        report["fixtures_sha256"] = {
            path.name: image_runner.digest(path)
            for path in sorted((ROOT / "fixtures/images").iterdir())
            if not path.name.startswith(".")
        }
        result = native_tests(runtime, report)
        diagnostics = result.stdout + result.stderr
        report["tests"] = dict(re.findall(
            r"test (?:\w+::)+(\w+) \.\.\. (ok|FAILED|ignored)", result.stdout))
        if (result.returncode != 0 or set(report["tests"]) != EXPECTED
                or any(value != "ok" for value in report["tests"].values())):
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


def save(report, diagnostics):
    artifacts = ROOT / "artifacts/image-region-jobs"
    artifacts.mkdir(parents=True, exist_ok=True)
    timestamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    encoded = json.dumps(report, indent=2) + "\n"
    with (artifacts / f"{timestamp}-{uuid.uuid4().hex}.json").open("x") as stream:
        stream.write(encoded)
    (ROOT / "artifacts/image-region-jobs-result.json").write_text(encoded)
    (ROOT / "artifacts/image-region-jobs-test-output.txt").write_text(diagnostics)


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
