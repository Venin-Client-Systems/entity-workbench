"""Verify app-local English OCR and hostile native assignment on this Mac only."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tempfile
import uuid
from stage_ocr_runtime import dependencies, system

ROOT = Path(__file__).resolve().parents[1]
EXPECTED = {"raster_rejects_expansion_ambiguity_and_unsupported_encodings", "missing_or_cancelled_ocr_fails_without_staging_input", "ocr_acceptance_preserves_raster_binding_and_uncertainty", "native_ocr_reads_only_app_local_runtime_and_reports_empty_image", "runtime_manifest_rejects_unreviewed_model_bytes", "ocr_profile_excludes_other_workers_and_original_writes", "native_ocr_profile_denies_outside_io_network_fork_and_inheritance", "native_ocr_rejects_omitted_dylibs_and_unlisted_runtime_assets"}
SOURCES = ["crates/core/src/lib.rs", "crates/core/src/engines.rs", "crates/core/src/engines/ocr.rs", "crates/core/src/engines/ocr/tests.rs", "crates/core/src/engines/supervision.rs", "crates/core/src/engines/supervision/ocr.rs", "crates/core/src/engines/supervision/ocr/tests.rs", "workers/native/ocr_probe.c", "scripts/stage_ocr_runtime.py", "scripts/test_ocr_workers.py", "scripts/test_ocr_evidence.py"]


class VerificationFailure(Exception):
    """Only fixed, sanitized failure codes belong in the public observation."""


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def inspect_runtime(runtime):
    manifest = json.loads((runtime / "ocr/manifest.json").read_text())
    actual = set()
    for path in (runtime / "ocr").rglob("*"):
        if path.is_symlink():
            raise VerificationFailure("linked_runtime_asset")
        if path.is_file():
            actual.add(str(path.relative_to(runtime / "ocr")))
        elif not path.is_dir():
            raise VerificationFailure("special_runtime_asset")
    if actual != set(manifest["files"]) | {"manifest.json"}:
        raise VerificationFailure("incomplete_runtime_inventory")
    for name, asset in manifest["files"].items():
        path = runtime / "ocr" / name
        if not path.is_file() or path.stat().st_size != asset["bytes"] or digest(path) != asset["sha256"]:
            raise VerificationFailure("runtime_asset_integrity")
        if name.startswith(("lib/", "bin/")):
            for dep in dependencies(path):
                if system(dep):
                    continue
                if not dep.startswith("@loader_path/"):
                    raise VerificationFailure("nonlocal_loader_dependency")
                resolved = (path.parent / dep.removeprefix("@loader_path/")).resolve(strict=True)
                if not resolved.is_relative_to(runtime / "ocr"):
                    raise VerificationFailure("loader_dependency_escape")
                if str(resolved.relative_to(runtime / "ocr")) not in manifest["files"]:
                    raise VerificationFailure("uninventoried_loader_dependency")
    return manifest


def native_tests(runtime, report):
    report["phase"] = "compile_probe"
    artifacts = ROOT / "artifacts/ocr"
    artifacts.mkdir(parents=True, exist_ok=True)
    probe = artifacts / "native-probe"
    subprocess.run(["cc", "-Wall", "-Wextra", "-Werror", str(ROOT / "workers/native/ocr_probe.c"), "-o", str(probe)], check=True, capture_output=True, text=True, timeout=30)
    report["phase"] = "relocate_runtime"
    # Move the bundle away from its staging location; source installation is denied.
    with tempfile.TemporaryDirectory(prefix="ocr relocation ") as directory:
        moved = Path(directory).resolve() / "engines"
        shutil.copytree(runtime / "ocr", moved / "ocr")
        environment = dict(os.environ, WORKBENCH_TEST_OCR_RUNTIME=str(moved), WORKBENCH_TEST_OCR_PROBE=str(probe), WORKBENCH_TEST_SECRET="synthetic-environment-sentinel")
        command = ["cargo", "test", "--locked", "-p", "workbench-core", "--lib", "ocr::tests::", "--", "--include-ignored", "--test-threads=1", "--nocapture"]
        report["phase"] = "native_tests"
        return subprocess.run(command, cwd=ROOT, env=environment, capture_output=True, text=True, timeout=120)


def observe(runtime):
    now = datetime.datetime.now(datetime.timezone.utc)
    report = {"schema_version": 1, "observed_at": now.isoformat(), "scope": "development Mac; relocated app-local English OCR and synthetic native confinement probe", "platform": {"os": platform.system(), "version": platform.mac_ver()[0], "architecture": platform.machine()}, "tests": {}, "phase": "source_identity", "passed": False, "outcome": "failed", "complete_release": False, "unverified": ["signed helpers", "Intel Mac and Windows", "clean installed package", "general image/PDF decoding", "word regions and original-page mapping", "recognition accuracy benchmark", "hard resident-memory and aggregate disk ceilings", "supervisor-crash process termination"]}
    diagnostics = ""
    try:
        report.update(source_revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(), source_dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)), source_sha256={name: digest(ROOT / name) for name in SOURCES})
        report["phase"] = "platform"
        if platform.system() != "Darwin":
            raise VerificationFailure("unsupported_native_platform")
        report["phase"] = "runtime_inventory"
        runtime = runtime.resolve(strict=True)
        report["runtime_manifest"] = inspect_runtime(runtime)
        report["runtime_manifest_sha256"] = digest(runtime / "ocr/manifest.json")
        report["fixture_sha256"] = digest(ROOT / "fixtures/ocr/synthetic.pgm")
        result = native_tests(runtime, report)
        diagnostics = result.stdout + result.stderr
        cases = dict(re.findall(r"test engines::(?:\w+::)*tests::(\w+) \.\.\. (ok|FAILED|ignored)", result.stdout))
        report["tests"] = cases
        if result.returncode != 0 or set(cases) != EXPECTED or any(value != "ok" for value in cases.values()):
            raise VerificationFailure("native_test_failure_or_incomplete_suite")
        report.update(passed=True, outcome="passed", phase="complete")
    except VerificationFailure as error:
        report["failure"] = str(error)
    except subprocess.TimeoutExpired:
        report["failure"] = "tool_timeout"
    except subprocess.CalledProcessError:
        report["failure"] = "tool_failed"
    except (OSError, ValueError, KeyError, TypeError):
        report["failure"] = "missing_or_invalid_verification_input"
    except Exception:
        # Unexpected verifier defects are failed observations too, never stale success.
        report["failure"] = "unexpected_verification_failure"
    return report, diagnostics


def save(report, diagnostics):
    artifacts = ROOT / "artifacts/ocr"
    artifacts.mkdir(parents=True, exist_ok=True)
    timestamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    encoded = json.dumps(report, indent=2) + "\n"
    # Failed observations replace the latest projection and retain their own history.
    # Clock precision differs by host; two observations can have the same time.
    # A unique, exclusively created history name must never overwrite a prior run.
    with (artifacts / f"{timestamp}-{uuid.uuid4()}.json").open("x", encoding="utf-8") as stream:
        stream.write(encoded)
    (ROOT / "artifacts/ocr-result.json").write_text(encoded)
    (ROOT / "artifacts/ocr-test-output.txt").write_text(diagnostics)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, default=ROOT / "runtime/staged/engines")
    args = parser.parse_args()
    report, diagnostics = observe(args.runtime)
    save(report, diagnostics)
    print(json.dumps({key: report[key] for key in ("source_revision", "source_dirty", "platform", "tests", "phase", "outcome", "failure", "passed", "complete_release") if key in report}, indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
