"""Compare exact serialized response sizes on an isolated retained synthetic 100k copy."""
import argparse
import datetime as dt
import json
import hashlib
from pathlib import Path
import shutil
import subprocess
import uuid

import transaction_performance as baseline
import transaction_page_benchmark as pages

ROOT = Path(__file__).resolve().parents[1]
CONFIG = {"fixture_sha256": pages.FIXTURE, "rows": 100_000,
          "build_timeout_seconds": 600, "prepare_timeout_seconds": 60,
          "compare_timeout_seconds": 120, "serialized_byte_limit": 256 * 1024 * 1024,
          "profile": "release", "timing_claim": False, "memory_claim": False}


def receipt(value):
    # The shared process runner retains raw resource logs. This diagnostic makes
    # no time/RSS comparison: opening, projection and oracle share one child.
    return {key: value[key] for key in ("returncode", "timed_out", "stdout", "stdout_sha256",
                                      "stderr", "stderr_sha256", "events")}


def copy_identity(source):
    # Prepared/candidate parents contain this diagnostic's mutable report; bind
    # only copied canonical files, not that unrelated parent reporting file.
    return {name: value for name, value in pages.source_identity(source).items()
            if not name.startswith("prior_campaign/")}


def run(source):
    directory = ROOT / "artifacts/desktop-summary-payload" / (
        f"{dt.datetime.now(dt.timezone.utc):%Y%m%dT%H%M%S}-{uuid.uuid4().hex}")
    directory.mkdir(parents=True)
    path = directory / "report.json"
    report = {"schema_version": 1, "scope": "payload-only opt-in summary versus presentation",
              "outcome": "incomplete", "phase": "metadata", "configuration": CONFIG,
              "complete_release": False}
    baseline.save(path, report)
    initial = None
    try:
        report.update(source_revision=baseline.capture(["git", "rev-parse", "HEAD"]),
                      source_dirty=bool(baseline.capture(["git", "status", "--porcelain"])),
                      source_sha256=baseline.sources(), host=baseline.host(),
                      script_sha256={name: baseline.digest(ROOT / "scripts" / name) for name in (
                          "desktop_summary_payload.py", "transaction_page_benchmark.py",
                          "transaction_performance.py")})
        report["configuration_sha256"] = hashlib.sha256(
            json.dumps(CONFIG, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        report["phase"] = "preflight"
        baseline.save(path, report)
        initial = pages.source_identity(source)
        if sorted(p.name for p in (source / "originals").iterdir()) != [pages.FIXTURE]:
            raise ValueError("Expected exactly the complete single-original synthetic corpus")
        report["retained_source_before"] = initial
        report["phase"] = "build"
        baseline.save(path, report)
        built = baseline.invoke(["cargo", "build", "--locked", "--release", "-p", "workbench-core",
                                 "--example", "desktop_summary_payload"], directory, "build", CONFIG["build_timeout_seconds"])
        report["build"] = receipt(built)
        if built["returncode"] != 0 or baseline.sources() != report["source_sha256"]:
            raise ValueError("Build failed or implementation sources changed")
        executable = directory / "desktop_summary_payload"
        shutil.copyfile(ROOT / "target/release/examples/desktop_summary_payload", executable)
        executable.chmod(0o700)
        report["executable_sha256"] = baseline.digest(executable)
        report["rustc"] = baseline.capture(["rustc", "--version"])
        report["phase"] = "prepare_copy"
        baseline.save(path, report)
        prepared = directory / "prepared"
        pages.copy_workspace(source, prepared)
        preparation = baseline.invoke([str(executable), str(prepared), "prepare"], directory, "prepare", CONFIG["prepare_timeout_seconds"])
        report["preparation"] = receipt(preparation)
        if preparation["returncode"] != 0 or len(preparation["events"]) != 1:
            raise ValueError("Isolated canonical workspace preparation failed")
        report["prepared_copy"] = copy_identity(prepared)
        report["phase"] = "compare"
        baseline.save(path, report)
        measured = directory / "compare"
        pages.copy_workspace(prepared, measured)
        before = copy_identity(measured)
        compared = baseline.invoke([str(executable), str(measured), "compare"], directory, "compare", CONFIG["compare_timeout_seconds"])
        report["comparison"] = receipt(compared)
        report["comparison_copy_before"] = before
        report["comparison_copy_after"] = copy_identity(measured)
        if before != report["comparison_copy_after"]:
            raise ValueError("Closed comparison database or original bytes changed")
        events = [v for v in compared["events"] if v.get("event") == "payload_comparison"]
        if compared["returncode"] != 0 or len(events) != 1 or events[0].get("projection_equal") is not True:
            raise ValueError("Complete exact projection comparison did not pass")
        if baseline.sources() != report["source_sha256"]:
            raise ValueError("Implementation sources changed during comparison")
        report["outcome"] = "measured"
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        report["outcome"] = "failed"
        report["failure_type"] = type(error).__name__
    finally:
        if initial is not None:
            try:
                report["retained_source_after"] = pages.source_identity(source)
                report["retained_source_unchanged"] = initial == report["retained_source_after"]
                if not report["retained_source_unchanged"]:
                    report["outcome"] = "failed"
                    report["failure_type"] = "RetainedSourceChanged"
            except (OSError, ValueError) as error:
                report["outcome"] = "failed"
                report["failure_type"] = type(error).__name__
                report["retained_source_unchanged"] = None
        baseline.save(path, report)
    return path, report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path,
                        help="Closed retained synthetic baseline case; opened only through a copy")
    args = parser.parse_args()
    path, report = run(args.source)
    print(json.dumps({"report": str(path.relative_to(ROOT)), "outcome": report["outcome"]}))
    return 0 if report["outcome"] == "measured" else 1


if __name__ == "__main__":
    raise SystemExit(main())
