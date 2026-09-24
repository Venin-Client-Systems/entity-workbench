"""Native synthetic parser/search boundary tests; not a signed release assertion."""
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
SOURCES = ["crates/core/src/lib.rs", "crates/core/src/engines.rs", "crates/core/src/engines/parser.rs", "crates/core/src/engines/parser/tests.rs", "crates/core/src/engines/supervision.rs", "crates/core/src/engines/supervision/tests.rs", "workers/java/src/main/java/workbench/ParseWorker.java", "workers/java/src/main/java/workbench/AppLocalFonts.java", "scripts/stage_parser_engines.py", "workers/java/src/main/java/workbench/SearchWorker.java", "workers/java/src/main/java/workbench/FlatIndex.java", "workers/java/src/main/java/workbench/CappedDirectory.java", "workers/java/src/main/java/workbench/Protocol.java", "workers/java/src/main/java/workbench/HostileProbe.java", "scripts/test_parser_workers.py"]
EXPECTED = {"unverified_termination_overrides_cancellation_and_retains_private_scratch", "cancelled_parser_never_launches_or_stages_input", "parser_results_require_source_binding_and_honest_status", "native_parser_extracts_supported_formats_and_preserves_limitations", "native_parser_has_no_search_access_and_cancels_running_worker", "index_acknowledgement_is_required_before_revision_publication", "cleanup_repairs_directories_without_touching_link_targets_and_reports_failure", "index_rejects_links_and_tree_budget_overruns", "native_java_hostile_and_benign_boundaries", "native_lucene_uses_separate_jobs_and_read_only_search_index", "process_setup_closes_inheritable_descriptor_and_reaps_timeout_group", "profile_has_distinct_read_and_write_access", "result_rejects_links_and_oversize_and_special_files"}


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, default=ROOT / "runtime/staged/engines")
    args = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("A native Mac and staged Java/parser/search runtime are required")
    runtime = args.runtime.resolve(strict=True)
    if list((runtime / "parser/lib").glob("lucene-*.jar")):
        parser.error("Parser classpath must exclude Lucene")
    for component in ("parser", "search"):
        if not (runtime / component / "workers-0.1.0.jar").is_file():
            parser.error("Both independent adapter JARs must be staged")
    command = ["cargo", "test", "--locked", "-p", "workbench-core", "--lib", "engines::", "--", "--include-ignored", "--skip", "engines::ocr::", "--skip", "engines::supervision::ocr::", "--skip", "engines::image::", "--skip", "engines::supervision::image_tests::", "--skip", "engines::pdf_render::", "--skip", "engines::supervision::pdf_tests::", "--test-threads=1", "--nocapture"]
    environment = dict(os.environ, WORKBENCH_TEST_RUNTIME=str(runtime), WORKBENCH_TEST_SECRET="synthetic-environment-sentinel")
    result = subprocess.run(command, cwd=ROOT, env=environment, capture_output=True, text=True, timeout=180)
    cases = dict(re.findall(r"test engines::(?:\w+::)*tests::(\w+) \.\.\. (ok|FAILED|ignored)", result.stdout))
    passed = result.returncode == 0 and set(cases) == EXPECTED and all(value == "ok" for value in cases.values())
    now = datetime.datetime.now(datetime.timezone.utc)
    report = {"schema_version": 1, "observed_at": now.isoformat(), "scope": "development Mac; synthetic parser/search and hostile sentinel tests", "source_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(), "source_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)), "source_sha256": {name: digest(ROOT / name) for name in SOURCES}, "platform": {"os": "macos", "version": platform.mac_ver()[0], "architecture": platform.machine()}, "runtime_sha256": {str(path.relative_to(runtime)): digest(path) for path in sorted(runtime.glob("*/**/*.jar"))}, "java_sha256": digest(runtime / "java/bin/java"), "fixtures_sha256": digest(ROOT / "fixtures/parser/manifest.json"), "tests": cases, "passed": passed, "complete_release": False, "unverified": ["signed helpers", "Intel Mac", "clean installed package", "OCR and page/cell anchors", "rendering", "all other document formats"]}
    artifacts = ROOT / "artifacts/parser"
    artifacts.mkdir(parents=True, exist_ok=True)
    (artifacts / f"{now.strftime('%Y%m%dT%H%M%S.%fZ')}.json").write_text(json.dumps(report, indent=2) + "\n")
    (ROOT / "artifacts/parser-result.json").write_text(json.dumps(report, indent=2) + "\n")
    (ROOT / "artifacts/parser-test-output.txt").write_text(result.stdout + result.stderr)
    print(json.dumps({key: report[key] for key in ("source_revision", "source_dirty", "platform", "tests", "passed", "complete_release")}, indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
