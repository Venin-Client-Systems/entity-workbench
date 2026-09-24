"""Verify unreviewed OCR word boxes against the actual app-local Mac worker."""
import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import uuid

import test_ocr_workers as ocr_runner
import shutil
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SOURCES = sorted(set(ocr_runner.SOURCES + [
    "Cargo.toml", "Cargo.lock", "crates/core/Cargo.toml",
    "crates/core/src/engines/ocr_regions.rs",
    "crates/core/src/engines/ocr_regions/tsv.rs",
    "crates/core/src/engines/ocr_regions/tests.rs",
    "crates/core/src/engines/supervision/ocr/region_tests.rs",
    "scripts/test_ocr_regions.py", "scripts/test_ocr_regions_evidence.py",
]))
EXPECTED = {
    "regions_parse_actual_hierarchy_and_separate_engine_confidence",
    "regions_reject_truncation_hierarchy_confidence_boxes_and_oversize",
    "regions_reject_typed_result_binding_and_schema_tampering",
    "regions_missing_runtime_and_cancelled_input_fail_without_scratch",
    "regions_enforce_word_and_row_caps_and_reject_duplicate_boxes",
    "native_regions_recognize_bounded_word_boxes_and_blank_raster",
    "regions_profile_grants_only_fixed_text_and_tsv_outputs",
    "native_regions_recipe_denies_outside_io_network_and_bounds_output_and_cancel",
}


def native_tests(runtime, report):
    report["phase"] = "compile_probe"
    artifacts = ROOT / "artifacts/ocr-regions"
    artifacts.mkdir(parents=True, exist_ok=True)
    probe = artifacts / "native-probe"
    subprocess.run(["cc", "-Wall", "-Wextra", "-Werror", str(ROOT / "workers/native/ocr_probe.c"),
                    "-o", str(probe)], check=True, capture_output=True, text=True, timeout=30)
    report["phase"] = "relocate_runtime"
    with tempfile.TemporaryDirectory(prefix="ocr regions relocation ") as directory:
        moved = Path(directory).resolve() / "engines"
        shutil.copytree(runtime / "ocr", moved / "ocr")
        environment = dict(os.environ, WORKBENCH_TEST_OCR_RUNTIME=str(moved),
                           WORKBENCH_TEST_OCR_PROBE=str(probe),
                           WORKBENCH_TEST_SECRET="synthetic-environment-sentinel")
        command = [
            "cargo", "test", "--locked", "-p", "workbench-core", "--lib", "--",
            "engines::ocr_regions::", "engines::supervision::ocr::region_tests::",
            "--include-ignored", "--test-threads=1",
        ]
        report["phase"] = "native_tests"
        return subprocess.run(command, cwd=ROOT, env=environment, capture_output=True,
                              text=True, timeout=180)


def observe(runtime):
    report = {
        "schema_version": 1,
        "observed_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scope": "development Mac; relocated app-local English OCR words/boxes and synthetic native confinement probe",
        "platform": {"os": platform.system(), "version": platform.mac_ver()[0],
                     "architecture": platform.machine()},
        "phase": "source_identity", "tests": {}, "passed": False, "outcome": "failed",
        "complete_release": False,
        "unverified": ["signed helpers", "Intel Mac and Windows runtime integration",
                       "clean installed package", "original-page mapping and reviewed word/source-region acceptance",
                       "canonical jobs and retained raster derivatives",
                       "recognition accuracy benchmark", "supervisor-crash process termination"],
    }
    diagnostics = ""
    try:
        report.update(
            source_revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
            source_dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
            source_sha256={name: ocr_runner.digest(ROOT / name) for name in SOURCES},
        )
        report["phase"] = "platform"
        if platform.system() != "Darwin":
            raise ocr_runner.VerificationFailure("unsupported_native_platform")
        report["phase"] = "runtime_inventory"
        runtime = runtime.resolve(strict=True)
        report["runtime"] = ocr_runner.inspect_runtime(runtime)
        report["fixtures_sha256"] = {
            "synthetic.pgm": ocr_runner.digest(ROOT / "fixtures/ocr/synthetic.pgm")
        }
        result = native_tests(runtime, report)
        diagnostics = result.stdout + result.stderr
        report["tests"] = dict(re.findall(
            r"test (?:\w+::)+(\w+) \.\.\. (ok|FAILED|ignored)", result.stdout))
        if (result.returncode != 0 or set(report["tests"]) != EXPECTED
                or any(value != "ok" for value in report["tests"].values())):
            raise ocr_runner.VerificationFailure("native_test_failure_or_incomplete_suite")
        report.update(passed=True, outcome="passed", phase="complete")
    except ocr_runner.VerificationFailure as error:
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
    artifacts = ROOT / "artifacts/ocr-regions"
    artifacts.mkdir(parents=True, exist_ok=True)
    timestamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    encoded = json.dumps(report, indent=2) + "\n"
    with (artifacts / f"{timestamp}-{uuid.uuid4().hex}.json").open("x") as stream:
        stream.write(encoded)
    (ROOT / "artifacts/ocr-regions-result.json").write_text(encoded)
    (ROOT / "artifacts/ocr-regions-test-output.txt").write_text(diagnostics)


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
