"""Run native development probes through the real Rust supervisor, without downloads.

The runtime must contain the current SearchWorker and HostileProbe JAR. This script
records source/runtime hashes and sanitized test outcomes; it is not release evidence.
"""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import platform
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SOURCES = [
    "crates/core/src/engines.rs",
    "crates/core/src/engines/supervision.rs",
    "crates/core/src/engines/supervision/tests.rs",
    "workers/java/src/main/java/workbench/Protocol.java",
    "workers/java/src/main/java/workbench/SearchWorker.java",
    "workers/java/src/main/java/workbench/HostileProbe.java",
    "scripts/test_macos_confinement.py",
]


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, default=ROOT / "runtime/staged/engines")
    args = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("This probe requires a native Mac; no platform claim was produced")
    runtime = args.runtime.resolve(strict=True)
    java = runtime / "java/bin/java"
    jar = runtime / "search/workers-0.1.0.jar"
    java_version = subprocess.check_output([str(java), "-version"], stderr=subprocess.STDOUT, text=True).splitlines()[0]
    if not re.search(r'version "21\.', java_version):
        parser.error("This development profile has only been tested with Java 21")
    environment = dict(os.environ, WORKBENCH_TEST_RUNTIME=str(runtime), WORKBENCH_TEST_SECRET="synthetic-environment-sentinel")
    command = ["cargo", "test", "--locked", "-p", "workbench-core", "--lib", "engines::supervision::tests::", "--", "--include-ignored", "--test-threads=1", "--nocapture"]
    result = subprocess.run(command, cwd=ROOT, env=environment, capture_output=True, text=True, timeout=180)
    cases = dict(re.findall(r"test engines::supervision::tests::(\w+) \.\.\. (ok|FAILED|ignored)", result.stdout))
    expected = {
        "result_rejects_links_and_oversize_and_special_files",
        "index_rejects_links_and_tree_budget_overruns",
        "profile_has_distinct_read_and_write_access",
        "cleanup_repairs_directories_without_touching_link_targets_and_reports_failure",
        "process_setup_closes_inheritable_descriptor_and_reaps_timeout_group",
        "native_java_hostile_and_benign_boundaries",
        "native_lucene_uses_separate_jobs_and_read_only_search_index",
    }
    passed = result.returncode == 0 and set(cases) == expected and all(value == "ok" for value in cases.values())
    report = {
        "schema_version": 1,
        "observed_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "method": "development sandbox-exec profile and actual Rust Java/Lucene supervisor",
        "scope": "synthetic disposable sentinels and local listener; development host only",
        "platform": {"os": "macos", "version": platform.mac_ver()[0], "architecture": platform.machine()},
        "source_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "source_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
        "source_sha256": {name: digest(ROOT / name) for name in SOURCES},
        "runtime": {"java_version": java_version, "java_sha256": digest(java), "worker_jar_sha256": digest(jar), "library_sha256": {path.name: digest(path) for path in sorted((runtime / "search/lib").glob("*.jar"))}},
        "command": command,
        "tests": cases,
        "passed": passed,
        "complete_release": False,
        "unverified": ["signed helpers", "Intel Mac", "clean installed artifact", "parser/PDF/OCR/Python/Chromium compatibility", "hard aggregate disk and resident-memory limits"],
    }
    artifacts = ROOT / "artifacts"
    artifacts.mkdir(exist_ok=True)
    history = artifacts / "confinement"
    history.mkdir(exist_ok=True)
    name = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    (history / f"{name}.json").write_text(json.dumps(report, indent=2) + "\n")
    (artifacts / "confinement-result.json").write_text(json.dumps(report, indent=2) + "\n")
    # Raw process logs stay local and may contain development paths.
    (artifacts / "confinement-test-output.txt").write_text(result.stdout + result.stderr)
    print(json.dumps(report, indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
