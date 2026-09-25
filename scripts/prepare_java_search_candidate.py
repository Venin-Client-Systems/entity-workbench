"""Prepare one inert Search candidate from pinned local bytes; never execute it."""
import argparse
import copy
import json
import os
from pathlib import Path
import stat
import subprocess
import uuid

import build_java_search as producer
import java_build_inputs as inputs
import native_search_runtime as runtime

ROOT = Path(__file__).resolve().parents[1]
POLICY = "current-java-search-candidate-v1"
RUNTIME_PIN = "cb3f7cc163a851eec4f70ba11c96029458524e8465210720a57f318503aef9a7"
WORKER_PIN = "a555216f95cd7ea44c9e16718e18bcd5b88f6089eac36680909192bfc9ca1877"
WORKER = "search/workers-0.1.0.jar"
EVIDENCE = "docs/search/verification/offline-java-producer-6c467bd.json"
READ_FLAGS = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)


def require(value, reason):
    inputs.require(value, reason)


def strict_json(data):
    def pairs(items):
        value = {}
        for key, item in items:
            require(key not in value, "duplicate_receipt_key")
            value[key] = item
        return value
    def invalid_constant(_):
        raise inputs.InvalidBuild("invalid_receipt_number")
    return json.loads(data, object_pairs_hook=pairs, parse_constant=invalid_constant)


def bound_receipt(data, evidence):
    name = f"artifacts/java-search-producer/{evidence['artifact_id']}/receipt.json"
    selected = [row for row in evidence["artifacts"] if row["path"] == name]
    require(len(selected) == 1 and inputs.identity(data) ==
            {key: selected[0][key] for key in ("bytes", "sha256")}, "producer_receipt_identity_mismatch")
    report = strict_json(data)
    producer.validate_success(report)
    require(report["passed"] is True and report["phase"] == "complete"
            and report["failure"] is None and report["source"]["revision"] == evidence["source"]["revision"]
            and report["source"]["tree"] == evidence["source"]["tree"], "producer_evidence_mismatch")
    require(report["builds"][0]["jar"]["sha256"] == WORKER_PIN, "unexpected_worker_identity")
    return report


def producer_input(artifact, signers):
    inputs.ordinary_root(artifact)
    evidence = strict_json(inputs.file_bytes(ROOT / EVIDENCE, 2 * 1024 * 1024))
    require(artifact.name == evidence["artifact_id"], "producer_artifact_id_mismatch")
    raw = inputs.file_bytes(artifact / "receipt.json", 2 * 1024 * 1024)
    report = bound_receipt(raw, evidence)
    subprocess.run(["git", "-c", "gpg.ssh.allowedSignersFile=" + str(signers), "verify-commit",
                    report["source"]["revision"]], cwd=ROOT, check=True, capture_output=True, timeout=30)
    names = subprocess.check_output(["git", "ls-files", "workers/java"], cwd=ROOT, timeout=30).decode().splitlines()
    java = [row for row in report["source"]["files"] if row["path"].startswith("workers/java/")]
    require(sorted(names) == sorted(row["path"] for row in java), "java_source_set_changed")
    for row in java:
        require(inputs.identity(inputs.file_bytes(ROOT / row["path"], 4 * 1024 * 1024)) ==
                {key: row[key] for key in ("bytes", "sha256")}, "java_source_bytes_changed")
    bodies = []
    for number, build in enumerate(report["builds"], 1):
        path = artifact / f"build-{number}/project/target/workers-0.1.0.jar"
        body = inputs.file_bytes(path, 16 * 1024 * 1024)
        require(inputs.identity(body) == {key: build["jar"][key] for key in ("bytes", "sha256")},
                "produced_worker_changed")
        bodies.append(body)
    require(bodies[0] == bodies[1], "produced_workers_differ")
    return report, raw, bodies[0]


def candidate_rows(before, report, body):
    require(before["sha256"] == RUNTIME_PIN, "selected_runtime_pin_mismatch")
    rows = copy.deepcopy(before["files"])
    old = [row for row in rows if row[0] == WORKER]
    require(len(old) == 1, "selected_worker_missing_or_ambiguous")
    require(inputs.identity(body) == {key: report["builds"][0]["jar"][key] for key in ("bytes", "sha256")}
            and inputs.identity(body)["sha256"] == WORKER_PIN, "replacement_worker_mismatch")
    selected = [{"name": row[0][len("search/lib/"):], "bytes": row[1], "sha256": row[2]}
                for row in rows if row[0].startswith("search/lib/")]
    require(len(selected) == 9 and selected == report["builds"][0]["search_dependencies"],
            "search_dependency_selection_mismatch")
    require({row[0] for row in rows if row[0].startswith("search/")} ==
            {WORKER, *("search/lib/" + row["name"] for row in selected)}, "unexpected_search_member")
    old[0][1:3] = [len(body), WORKER_PIN]
    require(len(rows) <= 1024 and sum(row[1] for row in rows) <= 256 * 1024 * 1024,
            "candidate_output_bound")
    return rows


def open_directory(parent, name, key, created):
    if key not in created:
        os.mkdir(name, 0o700, dir_fd=parent)
    info = os.stat(name, dir_fd=parent, follow_symlinks=False)
    require(stat.S_ISDIR(info.st_mode), "candidate_directory_link_or_special")
    node = (info.st_dev, info.st_ino)
    require(key not in created or created[key] == node, "candidate_directory_replaced")
    created[key] = node
    fd = os.open(name, READ_FLAGS | os.O_DIRECTORY, dir_fd=parent)
    try:
        require(inputs.signature(info) == inputs.signature(os.fstat(fd)), "candidate_directory_replaced")
    except BaseException:
        os.close(fd)
        raise
    return fd


def copy_candidate(source, destination, rows, body):
    # Existing destinations are never adopted, even if empty or byte-identical.
    inputs.ordinary_root(source)
    inputs.ordinary_root(destination.parent)
    destination.mkdir(mode=0o700)
    root = os.open(destination, READ_FLAGS | os.O_DIRECTORY)
    identity = os.fstat(root)
    created = {}
    try:
        for name, count, digest, executable in rows:
            parts = Path(name).parts
            require(parts and parts[0] in ("java", "search") and len(parts) <= 9
                    and not Path(name).is_absolute() and all(part not in (".", "..") for part in parts),
                    "invalid_candidate_member")
            data = body if name == WORKER else inputs.file_bytes(source / name, 128 * 1024 * 1024)
            require(inputs.identity(data) == {"bytes": count, "sha256": digest}, "copy_input_drift")
            parent = os.dup(root)
            try:
                for index, part in enumerate(parts[:-1], 1):
                    child = open_directory(parent, part, parts[:index], created)
                    os.close(parent)
                    parent = child
                fd = os.open(parts[-1], os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                             0o700 if executable else 0o600, dir_fd=parent)
                with os.fdopen(fd, "wb") as output:
                    output.write(data)
                    output.flush()
                    os.fsync(output.fileno())
            finally:
                os.close(parent)
        require((identity.st_dev, identity.st_ino) == (destination.lstat().st_dev, destination.lstat().st_ino),
                "candidate_root_replaced")
    finally:
        os.close(root)


def verify_candidate(path, rows):
    require(sorted(p.name for p in path.iterdir()) == ["java", "search"], "unexpected_candidate_root_entry")
    actual = runtime.inventory(path)
    require(actual["files"] == rows, "candidate_inventory_mismatch")
    # The copy creates only required ancestors; extra empty directories also fail.
    expected = {str(parent) for row in rows for parent in Path(row[0]).parents if str(parent) != "."}
    found, pending, entries = set(), [(path, 0)], 0
    while pending:
        directory, depth = pending.pop()
        require(depth <= 8, "candidate_depth_exceeded")
        inputs.ordinary_root(directory)
        with os.scandir(directory) as children:
            for child in children:
                entries += 1
                require(entries <= 1024, "candidate_entries_exceeded")
                info = child.stat(follow_symlinks=False)
                if stat.S_ISDIR(info.st_mode):
                    nested = directory / child.name
                    found.add(nested.relative_to(path).as_posix())
                    pending.append((nested, depth + 1))
                else:
                    require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1, "candidate_link_or_special")
    require(found == expected, "unexpected_candidate_directory")
    return actual


def prepare(artifact, source_runtime, producer_artifact, signers):
    report = {"schema_version": 1, "policy": POLICY, "prepared": False, "phase": "initial",
              "failure": None, "artifact_id": artifact.name, "runtime_execution": "not_run",
              "whole_runtime_reproduced_from_source": False, "complete_release": False,
              "failure_cleanup": "partial_files_retained_no_reuse_or_cleanup_claim"}
    inputs.save(artifact / "initial.json", report)
    inputs.save(artifact / "receipt.json", report)
    try:
        report["phase"] = "source"
        report["source"] = producer.tracked_source(signers)
        report["preparation_tool"] = inputs.identity(inputs.file_bytes(Path(__file__)))
        report["phase"] = "producer_binding"
        produced, receipt_bytes, body = producer_input(producer_artifact, signers)
        report["producer"] = {"receipt": inputs.identity(receipt_bytes), "source": produced["source"]["revision"],
                              "tree": produced["source"]["tree"], "artifact_id": producer_artifact.name,
                              "worker": inputs.identity(body), "signature_verified": True}
        report["phase"] = "runtime_inventory"
        before = runtime.inventory(source_runtime)
        report["runtime_before"] = before
        expected = candidate_rows(before, produced, body)
        report["phase"] = "copy"
        inputs.save(artifact / "receipt.json", report)
        copy_candidate(source_runtime, artifact / "runtime", expected, body)
        report["phase"] = "verification"
        report["candidate"] = verify_candidate(artifact / "runtime", expected)
        report["source_runtime_unchanged"] = runtime.inventory(source_runtime) == before
        report["producer_inputs_unchanged"] = producer_input(producer_artifact, signers) == (produced, receipt_bytes, body)
        report["source_unchanged"] = producer.tracked_source(signers) == report["source"]
        require(report["source_runtime_unchanged"] and report["producer_inputs_unchanged"]
                and report["source_unchanged"], "preparation_input_changed")
        report["changed_runtime_paths"] = [a[0] for a, b in zip(before["files"], expected) if a != b]
        require(report["changed_runtime_paths"] == [WORKER], "unexpected_runtime_substitution")
        report.update(prepared=True, phase="complete")
    except (inputs.InvalidBuild, runtime.InvalidRuntime, producer.Failure) as error:
        report["failure"] = str(error)
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        report["failure"] = "input_or_copy_failure"
    finally:
        try:
            inputs.save(artifact / "receipt.json", report)
        except (OSError, inputs.InvalidBuild):
            report["prepared"] = False
            report["failure"] = report["failure"] or "final_receipt_write_failed"
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("runtime", "producer-artifact", "allowed-signers"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    inputs.ordinary_root(ROOT)
    base = ROOT / "artifacts"
    base.mkdir(mode=0o700, exist_ok=True)
    inputs.ordinary_root(base)
    parent = base / "java-search-candidate"
    parent.mkdir(mode=0o700, exist_ok=True)
    inputs.ordinary_root(parent)
    artifact = parent / str(uuid.uuid4())
    artifact.mkdir(mode=0o700)
    report = prepare(artifact, args.runtime, args.producer_artifact, args.allowed_signers)
    print(json.dumps({"artifact_id": artifact.name, "prepared": report["prepared"],
                      "phase": report["phase"], "failure": report["failure"]}))
    return 0 if report["prepared"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
