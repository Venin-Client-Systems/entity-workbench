"""Offline development producer, never a runtime installer or worker launcher.

Exactly two fresh Maven packages if the first succeeds. Cached inputs only;
--offline is a resolver setting, not a network sandbox. Private cache names stay
in ignored artifacts; the public receipt exposes only selected output inputs.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import uuid

import java_build_inputs as inputs
from test_native_collection_transport import source_identity, Failure
from test_native_durable_https import run_logged, ProcessTimeout

ROOT = Path(__file__).resolve().parents[1]
POLICY = "offline-java-search-producer-v1"
TIMESTAMP = "2020-01-01T00:00:00Z"
OPTIONS = ["--offline", "--batch-mode", "--no-transfer-progress", "--strict-checksums",
           "-Dmaven.test.skip=true", "-Dmaven.compiler.proc=none", "-Dmaven.compiler.fork=false",
           "-Dcyclonedx.skip=true", "-Dproject.build.outputTimestamp=" + TIMESTAMP]
PLUGINS = {"maven-resources-plugin": "3.3.1", "maven-compiler-plugin": "3.14.1",
           "maven-surefire-plugin": "3.5.4", "maven-jar-plugin": "3.4.1",
           "maven-dependency-plugin": "3.8.1"}
LIFECYCLE = [("resources", "3.3.1", "resources"), ("compiler", "3.14.1", "compile"),
             ("resources", "3.3.1", "testResources"), ("compiler", "3.14.1", "testCompile"),
             ("surefire", "3.5.4", "test"), ("jar", "3.4.1", "jar"),
             ("dependency", "3.8.1", "copy-dependencies")]
OFFLINE_BOM_SKIP = ("[WARNING] Goal makeAggregateBom requires online mode for execution "
                    "but Maven is currently offline, skipping")


def require(value, reason):
    inputs.require(value, reason)


def tracked_source(signers):
    result = source_identity(signers)
    result["shared_source_reader_sha256"] = result.pop("runner_sha256")
    result["runner_sha256"] = inputs.identity(inputs.file_bytes(Path(__file__)))["sha256"]
    names = subprocess.check_output(["git", "ls-files", "workers/java", "scripts"], cwd=ROOT).decode().splitlines()
    result["files"] = [{"path": name, **inputs.identity(inputs.file_bytes(ROOT / name, 4 * 1024 * 1024))}
                       for name in names]
    result["inputs_sha256"] = inputs.summary(result["files"])["sha256"]
    return result


def environment(jdk, maven, private_home):
    # Avoid implicit user options, agents, classpaths, settings and extensions.
    return {"PATH": "/usr/bin:/bin", "JAVA_HOME": str(jdk), "M2_HOME": str(maven),
            "MAVEN_SKIP_RC": "true", "MAVEN_USER_HOME": str(private_home),
            "MAVEN_OPTS": "-Dfile.encoding=UTF-8 -Duser.timezone=UTC",
            "LANG": "C", "LC_ALL": "C", "TZ": "UTC"}


def command(maven, project, repository, settings, toolchains):
    return [str(maven / "bin/mvn"), *OPTIONS, "--settings", str(settings),
            "--global-settings", str(settings), "--toolchains", str(toolchains),
            "--global-toolchains", str(toolchains), "-Dmaven.repo.local=" + str(repository),
            "--file", str(project / "pom.xml"), "package"]


def selected_plugins(cache, rows):
    by_name = {row["path"]: row for row in rows}
    selected = []
    for name, version in PLUGINS.items():
        relative = f"org/apache/maven/plugins/{name}/{version}/{name}-{version}.jar"
        require(relative in by_name, "required_cached_plugin_missing")
        selected.append({"artifact": f"org.apache.maven.plugins:{name}:{version}",
                         **inputs.identity(inputs.file_bytes(cache / relative))})
    relative = "org/cyclonedx/cyclonedx-maven-plugin/2.9.1/cyclonedx-maven-plugin-2.9.1.jar"
    require(relative in by_name, "required_cached_plugin_missing")
    selected.append({"artifact": "org.cyclonedx:cyclonedx-maven-plugin:2.9.1",
                     **inputs.identity(inputs.file_bytes(cache / relative))})
    return selected


def plugin_headers(log):
    text = inputs.file_bytes(log, 4 * 1024 * 1024).decode("utf-8", errors="replace")
    text = re.sub(r"\x1b\[[0-9;]*m", "", text)
    lines = text.splitlines()
    headers = []
    for line in lines:
        if line.startswith("[INFO] --- "):
            match = re.fullmatch(r"\[INFO\] --- ([^:\s]+):([^:\s]+):([^\s]+) "
                                 r"\([^()\r\n]+\) @ workers ---", line)
            require(match is not None, "unexpected_build_plugin_header")
            headers.append(match.groups())
    require(headers == LIFECYCLE, "actual_build_plugin_versions_differ")
    # Maven refuses this online-only goal before plugin execution, so no
    # CycloneDX header is expected. Its pinned cached input is still recorded.
    require(lines.count(OFFLINE_BOM_SKIP) == 1, "offline_bom_skip_not_observed")
    return [list(row) for row in headers]


def dependencies(target, cached):
    rows = inputs.scan(target / "lib", maximum_files=512, maximum_bytes=256 * 1024 * 1024)
    result = []
    for row in rows:
        require("/" not in row["path"] and row["path"].endswith(".jar"), "unexpected_runtime_dependency")
        matches = [old for old in cached if Path(old["path"]).name == row["path"]
                   and old["sha256"] == row["sha256"] and old["bytes"] == row["bytes"]]
        require(len(matches) == 1, "runtime_dependency_not_unique_cached_input")
        result.append({"name": row["path"], "bytes": row["bytes"], "sha256": row["sha256"]})
    require(result, "runtime_dependencies_missing")
    return result


def validate_success(report):
    require(report["policy"] == POLICY and report["options"] == OPTIONS
            and report["build_goal"] == "package" and report["offline_is_network_sandbox"] is False,
            "producer_contract_mismatch")
    builds = report["builds"]
    require(len(builds) == 2 and all(type(b["number"]) is int for b in builds)
            and [b["number"] for b in builds] == [1, 2]
            and all(type(b["exit_code"]) is int and b["exit_code"] == 0 for b in builds),
            "two_complete_builds_required")
    require(builds[0]["jar"] == builds[1]["jar"]
            and builds[0]["dependencies"] == builds[1]["dependencies"], "build_output_not_reproducible")
    require(report["source_unchanged"] is True and report["tool_inputs_unchanged"] is True
            and report["cache_unchanged"] is True and report["staged_search_unchanged"] is True,
            "producer_inputs_drifted")
    expected = builds[0]["jar"]["sha256"] == report["staged_worker"]["sha256"]
    require(report["staged_worker_byte_equal"] is expected, "staged_equivalence_misreported")


def observe(artifact, *, signers, jdk, maven, cache, staged):
    report = {"receipt_version": 1, "policy": POLICY, "passed": False, "phase": "initial",
              "failure": None, "builds": [], "options": OPTIONS, "build_goal": "package",
              "offline_is_network_sandbox": False, "worker_execution": "not_run",
              "tests_in_maven": "skipped", "runtime_staging": "not_modified_by_this_tool",
              "limits": {"builds": 2, "build_seconds_each": 600, "tool_identity_seconds": 30,
                         "cache_files": 4096, "cache_entries": 8192, "cache_bytes": 512 * 1024 * 1024,
                         "input_file_bytes": 256 * 1024 * 1024, "input_depth": 20},
              "complete_release": False}
    inputs.save(artifact / "receipt.json", report)
    try:
        report["phase"] = "source"
        report["source"] = tracked_source(signers)
        report["phase"] = "input_inventory"
        source_rows = inputs.scan(ROOT / "workers/java", maximum_files=128, maximum_bytes=4 * 1024 * 1024)
        tracked_java = {row["path"][len("workers/java/"):] for row in report["source"]["files"]
                        if row["path"].startswith("workers/java/")}
        require({row["path"] for row in source_rows} == tracked_java, "untracked_java_build_inputs")
        cached, jdk_rows, maven_rows = (inputs.scan(path) for path in (cache, jdk, maven))
        staged_rows = inputs.scan(staged, maximum_files=128, maximum_bytes=64 * 1024 * 1024)
        report["cache"] = inputs.summary(cached)
        report["cache"]["scope"] = "bounded_private_input_snapshot_not_minimal_resolution_closure"
        report["jdk"] = inputs.summary(jdk_rows)
        report["maven"] = inputs.summary(maven_rows)
        report["selected_build_plugins"] = selected_plugins(cache, cached)
        report["staged_search"] = inputs.summary(staged_rows)
        report["staged_worker"] = inputs.jar_inventory(staged / "workers-0.1.0.jar")
        inputs.save(artifact / "private-inputs.json", {"cache": cached, "jdk": jdk_rows, "maven": maven_rows,
                                                     "staged_search": staged_rows, "java_sources": source_rows})
        settings, toolchains = artifact / "settings.xml", artifact / "toolchains.xml"
        settings.write_text('<settings xmlns="http://maven.apache.org/SETTINGS/1.2.0"/>\n')
        toolchains.write_text('<toolchains xmlns="http://maven.apache.org/TOOLCHAINS/1.1.0"/>\n')
        report["settings"] = inputs.identity(settings.read_bytes())
        report["toolchains"] = inputs.identity(toolchains.read_bytes())
        private_home = artifact / "maven-user"
        private_home.mkdir(mode=0o700)
        env = environment(jdk, maven, private_home)
        report["phase"] = "tool_identity"
        for name, invocation in [("java", [str(jdk / "bin/java"), "-version"]),
                                 ("javac", [str(jdk / "bin/javac"), "-version"]),
                                 ("maven", [str(maven / "bin/mvn"), "--version"])]:
            require(run_logged(invocation, artifact / f"{name}-version.log", 30, env) == 0,
                    "tool_identity_failed")
        # Version logs are private because Maven prints local tool paths.
        release = inputs.file_bytes(jdk / "release", 64 * 1024).decode()
        report["jdk_release"] = {key: value.strip('"') for key, value in
                                 (line.split("=", 1) for line in release.splitlines() if "=" in line)
                                 if key in ("IMPLEMENTOR", "JAVA_VERSION", "JAVA_RUNTIME_VERSION", "OS_NAME", "OS_ARCH")}
        report["compile_target"] = "Java 21 classfile target via declared POM release; compiler identity is the selected JDK"
        inputs.save(artifact / "receipt.json", report)
        for number in (1, 2):
            report["phase"] = f"build_{number}"
            area = artifact / f"build-{number}"
            area.mkdir(mode=0o700)
            inputs.copy_verified(ROOT / "workers/java", area / "project", source_rows)
            inputs.copy_verified(cache, area / "repository", cached)
            invocation = command(maven, area / "project", area / "repository", settings, toolchains)
            inputs.save(area / "invocation.json", {"command": invocation, "environment": env})
            record = {"number": number, "exit_code": None}
            report["builds"].append(record)
            inputs.save(artifact / "receipt.json", report)
            record["exit_code"] = run_logged(invocation, artifact / f"build-{number}.log", 600, env)
            require(record["exit_code"] == 0, "offline_maven_package_failed")
            record["plugin_headers"] = plugin_headers(artifact / f"build-{number}.log")
            record["cyclonedx_execution"] = "skipped_by_maven_offline_mode"
            target = area / "project/target"
            record["jar"] = inputs.jar_inventory(target / "workers-0.1.0.jar", (2020, 1, 1, 0, 0, 0))
            required_classes = {row["path"][len("src/main/java/"):-len(".java")] + ".class"
                                for row in source_rows if row["path"].startswith("src/main/java/")
                                and row["path"].endswith(".java")}
            require(required_classes and required_classes <= {row["path"] for row in record["jar"]["members"]},
                    "compiled_production_classes_missing")
            record["dependencies"] = dependencies(target, cached)
            # Only the existing Mac staging selection, without touching any staged path.
            record["search_dependencies"] = [row for row in record["dependencies"]
                                              if row["name"].startswith(("lucene-", "jackson-"))]
            require(record["search_dependencies"], "search_dependencies_missing")
        report["phase"] = "final_verification"
        report["source_unchanged"] = tracked_source(signers) == report["source"]
        report["tool_inputs_unchanged"] = inputs.scan(jdk) == jdk_rows and inputs.scan(maven) == maven_rows
        report["cache_unchanged"] = inputs.scan(cache) == cached
        report["staged_search_unchanged"] = inputs.scan(staged, maximum_files=128, maximum_bytes=64 * 1024 * 1024) == staged_rows
        report["staged_worker_byte_equal"] = report["builds"][0]["jar"]["sha256"] == report["staged_worker"]["sha256"]
        report["staged_search_dependencies_equal"] = report["builds"][0]["search_dependencies"] == [
            {"name": row["path"][4:], "bytes": row["bytes"], "sha256": row["sha256"]}
            for row in staged_rows if row["path"].startswith("lib/")]
        validate_success(report)
        report.update(passed=True, phase="complete")
    except ProcessTimeout as error:
        report.update(failure="owned_build_process_timeout", owned_process_stop=error.termination)
    except (inputs.InvalidBuild, Failure) as error:
        report["failure"] = str(error)
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        report["failure"] = "input_or_tool_failure"
    finally:
        try:
            report["logs"] = [{"path": path.name, **inputs.identity(inputs.file_bytes(path, 4 * 1024 * 1024))}
                              for path in sorted(artifact.glob("*.log"))]
        except (OSError, inputs.InvalidBuild):
            report["passed"] = False
            report["failure"] = report["failure"] or "diagnostic_identity_unavailable"
            report.setdefault("evidence_failures", []).append("diagnostic_identity_unavailable")
        try:
            inputs.save(artifact / "receipt.json", report)
        except (OSError, inputs.InvalidBuild):
            report["passed"] = False
            report["failure"] = report["failure"] or "final_receipt_write_failed"
            report.setdefault("evidence_failures", []).append("final_receipt_write_failed")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("allowed-signers", "jdk", "maven", "cache", "staged-search"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    require(sys.platform == "darwin", "macos_development_producer_only")
    parent = ROOT / "artifacts/java-search-producer"
    parent.mkdir(parents=True, mode=0o700, exist_ok=True)
    inputs.ordinary_root(parent)
    artifact = parent / str(uuid.uuid4())
    artifact.mkdir(mode=0o700)
    report = observe(artifact, signers=args.allowed_signers, jdk=args.jdk, maven=args.maven,
                     cache=args.cache, staged=args.staged_search)
    print(json.dumps({"artifact_id": artifact.name, "passed": report["passed"],
                      "phase": report["phase"], "failure": report["failure"]}))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
