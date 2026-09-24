"""Native PNG/JPEG-to-OCR verification; development evidence, not a release assertion."""
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
import test_ocr_workers as ocr_verifier

ROOT = Path(__file__).resolve().parents[1]
EXPECTED = {
    "image_results_bind_original_raster_dimensions_and_honest_outcome",
    "cancelled_and_missing_image_runtime_fail_closed",
    "native_images_decode_and_ocr_with_exact_original_and_raster_binding",
    "native_images_reject_oversize_animation_malformed_and_unsupported_without_rasters",
    "image_profile_adds_only_its_assigned_raster_output",
    "native_image_worker_denies_outside_io_network_and_verifies_cancel_and_output_bound",
}
SOURCES = [
    "crates/core/src/engines.rs", "crates/core/src/engines/image.rs",
    "crates/core/src/engines/image/tests.rs", "crates/core/src/engines/supervision.rs",
    "crates/core/src/engines/supervision/image_tests.rs",
    "workers/java/src/main/java/workbench/ImageWorker.java",
    "workers/java/src/main/java/workbench/Protocol.java",
    "workers/java/src/main/java/workbench/HostileProbe.java",
    "scripts/stage_image_worker.py", "scripts/test_image_workers.py",
    "scripts/test_image_evidence.py", "scripts/GenerateImageFixtures.java",
    "crates/core/src/engines/ocr.rs", "crates/core/src/engines/ocr/tests.rs",
    "crates/core/src/engines/supervision/ocr.rs",
    "crates/core/src/engines/supervision/ocr/tests.rs",
    "scripts/stage_ocr_runtime.py", "scripts/test_ocr_workers.py",
    "scripts/test_ocr_evidence.py", "workers/native/ocr_probe.c",
]


class VerificationFailure(Exception):
    """Only fixed, sanitized failure codes are written to the public observation."""


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def inspect_runtime(runtime):
    expected = {"workers-0.1.0.jar", "lib/jackson-annotations-2.22.jar",
                "lib/jackson-core-2.22.3.jar", "lib/jackson-databind-2.22.3.jar"}
    actual = set()
    for path in (runtime / "image").rglob("*"):
        if path.is_symlink() or not (path.is_file() or path.is_dir()):
            raise VerificationFailure("linked_or_special_image_asset")
        if path.is_file():
            actual.add(str(path.relative_to(runtime / "image")))
    if actual != expected:
        raise VerificationFailure("unexpected_image_classpath")
    ocr_verifier.inspect_runtime(runtime)
    return {
        "image_sha256": {name: digest(runtime / "image" / name) for name in sorted(actual)},
        "java_sha256": {name: digest(runtime / "java" / name)
                        for name in ("bin/java", "lib/modules", "release")},
        "ocr_manifest_sha256": digest(runtime / "ocr/manifest.json"),
    }


def native_tests(runtime, report):
    report["phase"] = "relocate_runtime"
    with tempfile.TemporaryDirectory(prefix="image relocation ") as directory:
        moved = Path(directory).resolve() / "engines"
        for component in ("java", "image", "ocr"):
            shutil.copytree(runtime / component, moved / component)
        environment = dict(os.environ, WORKBENCH_TEST_IMAGE_RUNTIME=str(moved),
                           WORKBENCH_TEST_SECRET="synthetic-environment-sentinel")
        command = ["cargo", "test", "--locked", "-p", "workbench-core", "--lib", "image",
                   "--", "--include-ignored", "--skip", "engines::ocr::",
                   "--test-threads=1", "--nocapture"]
        report["phase"] = "native_tests"
        return subprocess.run(command, cwd=ROOT, env=environment, capture_output=True,
                              text=True, timeout=180)


def observe(runtime):
    report = {
        "schema_version": 1, "observed_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scope": "development Mac; relocated PNG/JPEG decoder, separate OCR and synthetic hostile probes",
        "platform": {"os": platform.system(), "version": platform.mac_ver()[0],
                     "architecture": platform.machine()},
        "phase": "source_identity", "tests": {}, "passed": False, "outcome": "failed",
        "complete_release": False,
        "unverified": ["signed helpers", "Intel Mac and Windows", "clean installed package",
                       "PDF rendering", "other image formats", "EXIF orientation correction",
                       "word regions and document page mapping", "recognition accuracy benchmark",
                       "hard resident-memory and aggregate disk ceilings",
                       "supervisor-crash process termination"],
    }
    diagnostics = ""
    try:
        report.update(
            source_revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
            source_dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
            source_sha256={name: digest(ROOT / name) for name in SOURCES},
        )
        report["phase"] = "platform"
        if platform.system() != "Darwin":
            raise VerificationFailure("unsupported_native_platform")
        report["phase"] = "runtime_inventory"
        runtime = runtime.resolve(strict=True)
        report["runtime"] = inspect_runtime(runtime)
        report["fixtures_sha256"] = {path.name: digest(path)
                                     for path in sorted((ROOT / "fixtures/images").iterdir())
                                     if not path.name.startswith(".")}
        result = native_tests(runtime, report)
        diagnostics = result.stdout + result.stderr
        cases = dict(re.findall(r"test engines::(?:\w+::)*(?:tests|image_tests)::(\w+) \.\.\. (ok|FAILED|ignored)", result.stdout))
        report["tests"] = cases
        if result.returncode != 0 or set(cases) != EXPECTED or any(value != "ok" for value in cases.values()):
            raise VerificationFailure("native_test_failure_or_incomplete_suite")
        report.update(passed=True, outcome="passed", phase="complete")
    except (VerificationFailure, ocr_verifier.VerificationFailure) as error:
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
    artifacts = ROOT / "artifacts/image"
    artifacts.mkdir(parents=True, exist_ok=True)
    timestamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    encoded = json.dumps(report, indent=2) + "\n"
    # Coarse clocks can repeat across successive observations (including Windows).
    with (artifacts / f"{timestamp}-{uuid.uuid4().hex}.json").open("x") as stream:
        stream.write(encoded)
    (ROOT / "artifacts/image-result.json").write_text(encoded)
    (ROOT / "artifacts/image-test-output.txt").write_text(diagnostics)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, default=ROOT / "runtime/staged/engines")
    report, diagnostics = observe(parser.parse_args().runtime)
    save(report, diagnostics)
    print(json.dumps({key: report[key] for key in ("source_revision", "source_dirty", "platform",
                     "tests", "phase", "outcome", "failure", "passed", "complete_release")
                     if key in report}, indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
