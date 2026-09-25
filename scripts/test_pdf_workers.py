"""Native PDF page-to-OCR verification; development evidence, not a release assertion."""
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
    "pdf_result_rejects_original_page_dpi_geometry_raster_and_protocol_tampering",
    "pdf_requests_and_unavailable_runtime_fail_closed",
    "native_pdf_scan_and_selected_page_render_then_ocr_with_bound_provenance",
    "native_pdf_crop_and_all_quarter_turns_match_pixel_mapping",
    "native_pdf_rejections_are_explicit_and_never_enter_ocr",
    "native_pdf_runtime_inventory_rejects_missing_unlisted_linked_or_modified_assets",
    "pdf_profile_adds_only_its_assigned_raster_output",
    "native_pdf_worker_denies_outside_io_network_and_verifies_cancel_and_output_bound",
}
SOURCES = [
    "crates/core/Cargo.toml", "Cargo.lock", "crates/core/src/engines.rs", "crates/core/src/engines/pdf_render.rs",
    "crates/core/src/engines/pdf_render/tests.rs", "crates/core/src/engines/pdf_render/runtime.rs",
    "crates/core/src/engines/supervision.rs", "crates/core/src/engines/supervision/pdf_tests.rs",
    "workers/java/src/main/java/workbench/PdfRenderWorker.java",
    "workers/java/src/main/java/workbench/PdfRenderPolicy.java",
    "workers/java/src/main/java/workbench/Protocol.java",
    "workers/java/src/main/java/workbench/HostileProbe.java",
    "scripts/stage_pdf_worker.py", "scripts/test_pdf_workers.py",
    "scripts/test_pdf_evidence.py", "scripts/GeneratePdfRenderFixtures.java",
    "crates/core/src/engines/ocr.rs", "crates/core/src/engines/ocr/tests.rs",
    "crates/core/src/engines/supervision/ocr.rs", "crates/core/src/engines/supervision/ocr/tests.rs",
    "scripts/stage_ocr_runtime.py", "scripts/test_ocr_workers.py", "scripts/test_ocr_evidence.py",
    "workers/native/ocr_probe.c",
]



class VerificationFailure(Exception):
    """Only fixed, sanitized failure codes are written to the public observation."""


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def inspect_runtime(runtime):
    component = runtime / "pdf-render"
    if component.is_symlink() or not component.is_dir():
        raise VerificationFailure("linked_or_missing_pdf_component")
    manifest = json.loads((component / "manifest.json").read_text())
    expected = {"workers-0.1.0.jar", "lib/pdfbox-3.0.8.jar", "lib/pdfbox-io-3.0.8.jar",
                "lib/fontbox-3.0.8.jar", "lib/commons-logging-1.4.0.jar", "lib/jackson-annotations-2.22.jar",
                "lib/jackson-core-2.22.3.jar", "lib/jackson-databind-2.22.3.jar"}
    actual = {}
    for path in component.rglob("*"):
        if path.is_symlink() or not (path.is_file() or path.is_dir()):
            raise VerificationFailure("linked_or_special_pdf_asset")
        if path.is_file() and path != component / "manifest.json":
            name = str(path.relative_to(component))
            if name not in expected and not name.startswith("notices/"):
                raise VerificationFailure("unexpected_pdf_classpath")
            actual[name] = digest(path)
    if not expected.issubset(actual) or manifest != {"schema_version": 1, "renderer": "pdfbox-3.0.8-scan-v1", "files": actual}:
        raise VerificationFailure("pdf_inventory_mismatch")
    ocr_verifier.inspect_runtime(runtime)
    return {"pdf_sha256": actual,
            "java_sha256": {name: digest(runtime / "java" / name) for name in ("bin/java", "lib/modules", "release")},
            "ocr_manifest_sha256": digest(runtime / "ocr/manifest.json")}


def native_tests(runtime, report):
    report["phase"] = "relocate_runtime"
    with tempfile.TemporaryDirectory(prefix="pdf relocation ") as directory:
        moved = Path(directory).resolve() / "engines"
        for component in ("java", "pdf-render", "ocr"):
            shutil.copytree(runtime / component, moved / component)
        environment = dict(os.environ, WORKBENCH_TEST_PDF_RUNTIME=str(moved),
                           WORKBENCH_TEST_SECRET="synthetic-environment-sentinel")
        command = ["cargo", "test", "--locked", "-p", "workbench-core", "--lib", "--",
                   "engines::pdf_render::", "engines::supervision::pdf_tests::",
                   "--include-ignored", "--test-threads=1", "--nocapture"]
        report["phase"] = "native_tests"
        return subprocess.run(command, cwd=ROOT, env=environment, capture_output=True,
                              text=True, timeout=240)


def observe(runtime):
    report = {
        "schema_version": 1, "observed_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scope": "development Mac; relocated PDF scan renderer, separate OCR and synthetic hostile probes",
        "platform": {"os": platform.system(), "version": platform.mac_ver()[0],
                     "architecture": platform.machine()},
        "phase": "source_identity", "tests": {}, "passed": False, "outcome": "failed",
        "complete_release": False,
        "unverified": ["signed helpers", "Intel Mac and Windows", "clean installed package",
                       "general PDFs and font rendering", "advanced graphics and image codecs",
                       "word regions and canonical PDF OCR jobs", "recognition accuracy benchmark",
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
                                     for path in sorted((ROOT / "fixtures/pdf-render").iterdir())
                                     if not path.name.startswith(".")}
        result = native_tests(runtime, report)
        diagnostics = result.stdout + result.stderr
        cases = dict(re.findall(r"test engines::(?:\w+::)*(?:tests|pdf_tests)::(\w+) \.\.\. (ok|FAILED|ignored)", result.stdout))
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
    artifacts = ROOT / "artifacts/pdf-render"
    artifacts.mkdir(parents=True, exist_ok=True)
    timestamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    encoded = json.dumps(report, indent=2) + "\n"
    # Coarse clocks can repeat across successive observations (including Windows).
    with (artifacts / f"{timestamp}-{uuid.uuid4().hex}.json").open("x") as stream:
        stream.write(encoded)
    (ROOT / "artifacts/pdf-render-result.json").write_text(encoded)
    (ROOT / "artifacts/pdf-render-test-output.txt").write_text(diagnostics)


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
