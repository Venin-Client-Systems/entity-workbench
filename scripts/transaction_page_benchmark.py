"""Measure real revision-bound pages on copies of the retained synthetic 100k corpus."""
import argparse
import datetime as dt
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import uuid

import transaction_performance as baseline

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = "c587e3ad59c11445f780306b9aef370e4d97310cf66728b1aabe5f9819c5aef7"
OPERATIONS = ("all_first", "all_next", "descending_first", "filtered_first", "filtered_next",
              "pending_first", "empty_first", "stale_revision", "presentation")
SAMPLES = 21


def source_identity(source):
    paths = {"workspace.db": source / "workspace.db", "originals/" + FIXTURE: source / "originals" / FIXTURE}
    for suffix in ("-wal", "-shm", "-journal"):
        if (source / ("workspace.db" + suffix)).exists():
            raise ValueError("Retained source must be closed and checkpointed before copying")
    for label in ("report.json", "fixture.json", "fixture.csv"):
        candidate = source.parent / label
        if candidate.exists():
            paths["prior_campaign/" + label] = candidate
    if any(path.is_symlink() or not path.is_file() for path in paths.values()):
        raise ValueError("Source identities must be ordinary files")
    if paths["originals/" + FIXTURE].stat().st_size != 5_010_041:
        raise ValueError("Wrong original size")
    result = {name: {"sha256": baseline.digest(path), "bytes": path.stat().st_size} for name, path in paths.items()}
    if result["originals/" + FIXTURE]["sha256"] != FIXTURE:
        raise ValueError("Wrong frozen original")
    return result


def copy_workspace(source, target):
    target.mkdir(mode=0o700)
    (target / "originals").mkdir(mode=0o700)
    for relative in ("workspace.db", "originals/" + FIXTURE):
        destination = target / relative
        with (source / relative).open("rb") as original, destination.open("xb") as copied:
            shutil.copyfileobj(original, copied, 1024 * 1024)
            copied.flush()
            os.fsync(copied.fileno())
        destination.chmod(0o600 if relative == "workspace.db" else 0o400)
        if baseline.digest(destination) != baseline.digest(source / relative):
            raise ValueError("Isolated copy differs")


def summarize(observed, operation):
    result = baseline.summarize(observed["events"], operation, SAMPLES)
    warm = [sample.get("elapsed_ms") for sample in result["samples"][1:]]
    if len(warm) == SAMPLES - 1 and all(type(value) in (int, float) and math.isfinite(value) and value >= 0 for value in warm):
        ordered = sorted(warm)
        result["warm_p50_nearest_rank_ms"] = ordered[math.ceil(0.5 * len(ordered)) - 1]
    else:
        result["warm_p50_nearest_rank_ms"] = None
    result["outcome"] = "measured" if observed["returncode"] == 0 and result["measurement_complete"] else "failed"
    return result


def run(source, reuse_build=False):
    directory = ROOT / "artifacts/transaction-pages" / f"{dt.datetime.now(dt.timezone.utc):%Y%m%dT%H%M%S}-{uuid.uuid4().hex}"
    directory.mkdir(parents=True)
    path = directory / "report.json"
    report = {"schema_version": 1, "scope": "isolated synthetic 100k canonical transaction pages",
              "observed_at": dt.datetime.now(dt.timezone.utc).isoformat(), "host": baseline.host(),
              "source_revision": baseline.capture(["git", "rev-parse", "HEAD"]),
              "source_dirty": bool(baseline.capture(["git", "status", "--porcelain"])),
              "source_sha256": baseline.sources(), "runner_sha256": baseline.digest(Path(__file__)),
              "shared_runner_sha256": baseline.digest(Path(baseline.__file__)),
              "phase": "preflight", "outcome": "incomplete", "complete_release": False,
              "operations": {name: {"outcome": "not_run"} for name in OPERATIONS},
              "limits": {"operation_process_seconds": 180, "samples": SAMPLES, "page_size": 200},
              "unverified": ["cold OS caches", "GUI/IPC rendering", "statistically qualified tail latency",
                             "whole-application memory", "concurrent workers/maps/graphs", "clean installation",
                             "power and thermal controls"]}
    baseline.save(path, report)
    initial = None
    try:
        initial = source_identity(source)
        report["original_source_files"] = initial
        report["phase"] = "build"
        baseline.save(path, report)
        binary = ROOT / "target/release/examples/transaction_page_benchmark"
        receipt = ROOT / "artifacts/transaction-pages-build.json"
        if reuse_build:
            built = json.loads(receipt.read_text())
            if built["source_sha256"] != report["source_sha256"] or built["binary_sha256"] != baseline.digest(binary):
                raise ValueError("Build reuse refused: binary or source identity differs")
            report["build"] = dict(built, reused=True)
        else:
            report["build"] = baseline.invoke(["cargo", "build", "--locked", "--release", "-p", "workbench-core",
                                                "--example", "transaction_page_benchmark"], directory, "build", 600)
            if report["build"]["returncode"] != 0 or baseline.sources() != report["source_sha256"]:
                raise ValueError("Build failed or sources changed")
            built = {"source_sha256": report["source_sha256"], "binary_sha256": baseline.digest(binary),
                     "rustc": baseline.capture(["rustc", "--version"]), "profile": "release"}
            receipt.write_text(json.dumps(built, indent=2) + "\n")
            report["build"].update(built)
        retained_binary = directory / "transaction_page_benchmark"
        with binary.open("rb") as compiled, retained_binary.open("xb") as retained:
            shutil.copyfileobj(compiled, retained, 1024 * 1024)
        retained_binary.chmod(0o500)
        if baseline.digest(retained_binary) != report["build"]["binary_sha256"]:
            raise ValueError("Retained executable identity differs")
        binary = retained_binary
        report["retained_executable"] = retained_binary.name
        report["phase"] = "prepare_copy"
        baseline.save(path, report)
        template = directory / "prepared"
        copy_workspace(source, template)
        prepared = baseline.invoke([str(binary), str(template), "prepare"], directory, "prepare", 60)
        report["preparation"] = prepared
        prepared_events = [event for event in prepared["events"] if event.get("event") == "prepared"]
        if prepared["returncode"] != 0 or len(prepared_events) != 1:
            raise ValueError("Copied canonical workspace preparation failed")
        report["prepared_workspace"] = dict(prepared_events[0], database_sha256=baseline.digest(template / "workspace.db"))
        for operation in OPERATIONS:
            report["phase"] = operation
            report["operations"][operation] = {"outcome": "running"}
            baseline.save(path, report)
            workspace = directory / ("measure-" + operation)
            copy_workspace(template, workspace)
            before = baseline.digest(workspace / "workspace.db")
            observed = baseline.invoke([str(binary), str(workspace), operation], directory, operation, 180)
            observed.update(summarize(observed, operation))
            after = baseline.digest(workspace / "workspace.db")
            observed.update(database_sha256_before=before, database_sha256_after=after,
                            original_sha256_after=baseline.digest(workspace / "originals" / FIXTURE))
            if observed["original_sha256_after"] != FIXTURE:
                observed["outcome"] = "failed"
                observed["original_identity_failure"] = True
            if before != after or before != report["prepared_workspace"]["database_sha256"]:
                observed["outcome"] = "failed"
                observed["workspace_identity_failure"] = True
            report["operations"][operation] = observed
            baseline.save(path, report)
        report["phase"] = "empty_query_diagnostic"
        baseline.save(path, report)
        diagnostic = baseline.invoke([str(binary), str(template), "empty_query_diagnostic"],
                                     directory, "empty_query_diagnostic", 60)
        report["empty_query_diagnostic"] = diagnostic
        events = diagnostic["events"]
        if diagnostic["returncode"] != 0 or sum(e.get("event") == "query_diagnostic_complete" for e in events) != 1:
            raise ValueError("Fixed empty-query diagnostic failed")
        if baseline.digest(template / "workspace.db") != report["prepared_workspace"]["database_sha256"]:
            raise ValueError("Diagnostic changed its copied workspace")
        if baseline.sources() != report["source_sha256"]:
            raise ValueError("Benchmark sources changed during measurement")
        report["phase"] = "complete"
        report["outcome"] = "measured" if all(v["outcome"] == "measured" for v in report["operations"].values()) else "failed"
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        report["outcome"] = "failed"
        report["failure"] = str(error)  # Local diagnostics; inspect before selecting a public observation.
    finally:
        if initial is not None:
            try:
                report["original_source_unchanged"] = source_identity(source) == initial
            except (OSError, ValueError):
                report["original_source_unchanged"] = False
            if not report["original_source_unchanged"]:
                report["outcome"] = "failed"
        report["finished_at"] = dt.datetime.now(dt.timezone.utc).isoformat()
        baseline.save(path, report)
    print(json.dumps({"report": str(path.relative_to(ROOT)), "outcome": report["outcome"], "complete_release": False}))
    return 0 if report["outcome"] == "measured" else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-workspace", type=Path, required=True)
    parser.add_argument("--reuse-build", action="store_true")
    args = parser.parse_args()
    raise SystemExit(run(args.source_workspace.resolve(), args.reuse_build))
