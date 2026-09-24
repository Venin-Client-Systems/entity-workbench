"""Record a bounded native canonical-parser campaign; never release acceptance."""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import platform
import re
import subprocess
import sys
import uuid

ROOT = Path(__file__).resolve().parents[1]
SHA = re.compile(r"[0-9a-f]{64}\Z")
COMMIT = re.compile(r"[0-9a-f]{40}\Z")
FIXTURES = [
    ("notice.txt", "fixtures/parser/notice.txt", None, "completed", "utf8-v1"),
    ("unreviewed.source", None, b"Synthetic unreviewed UTF-8 source. None must remain None.\n", "completed", "utf8-v1"),
    ("notice.pdf", "fixtures/parser/notice.pdf", None, "partial", "pdfbox-3.0.8-local-fonts-v1"),
    ("notice.docx", "fixtures/parser/notice.docx", None, "partial", "tika-ooxml-3.3.2"),
    ("no-text.pdf", "fixtures/parser/no-text.pdf", None, "partial", "pdfbox-3.0.8-local-fonts-v1"),
    ("malformed.pdf", None, b"%PDF-synthetic malformed input\n", "failed", "pdfbox-3.0.8-local-fonts-v1"),
    ("traversal.zip", "fixtures/parser/traversal.zip", None, "failed", "zip-preflight-v1"),
    ("unsupported.bin", None, b"\0\xff\x80synthetic unsupported", "blocked", "unsupported-v1"),
    ("font-corpus.pdf", "fixtures/parser-fonts/corpus.pdf", None, "partial", "pdfbox-3.0.8-local-fonts-v1"),
    ("embedded-font.pdf", "fixtures/parser-fonts/embedded.pdf", None, "partial", "pdfbox-3.0.8-local-fonts-v1"),
]
CHECKS = {"fixture_" + row[0] for row in FIXTURES} | {
    "queued_cancellation_unclaimed", "evidence_unchanged", "idempotent_queue",
    "one_result_per_job", "restart_unchanged", "frozen_backup_isolated",
    "full_backup_restored", "runtime_control_baseline", "missing_runtime_blocked",
    "altered_runtime_blocked", "wrong_role_runtime_blocked", "control_records_unchanged",
    "joined_shutdown", "scratch_cleanup", "staged_runtime_unchanged",
}
PHASES = {"not_started", "restart_and_restore", "missing_runtime", "runtime_control_copy",
          "runtime_control_baseline", "altered_runtime", "wrong_role_runtime", "cleanup", "complete"} | {
              "fixture_" + row[0] for row in FIXTURES}
FAILURES = {"not_started", "arguments", "unsupported_host", "workspace", "command", "deadline",
            "fixture", "extraction", "publication", "recovery", "runtime_control", "shutdown",
            "cleanup", "receipt", "unexpected"}


def require(condition):
    if not condition:
        raise ValueError("closed campaign contract rejected")


def pairs(items):
    result = {}
    for key, value in items:
        require(key not in result)
        result[key] = value
    return result


def read_json(path, limit=128 * 1024):
    with path.open("rb") as handle:
        data = handle.read(limit + 1)
    require(len(data) <= limit)
    return json.loads(data.decode("utf-8-sig"), object_pairs_hook=pairs,
                      parse_constant=lambda _: (_ for _ in ()).throw(ValueError("nonfinite JSON")))


def digest(path, limit):
    require(path.is_file() and not path.is_symlink() and path.stat().st_size <= limit)
    result = hashlib.sha256()
    count = 0
    with path.open("rb") as handle:
        while block := handle.read(65536):
            count += len(block)
            require(count <= limit)
            result.update(block)
    return result.hexdigest()


def fixture_identities():
    result = []
    for name, relative, inline, _, _ in FIXTURES:
        if relative:
            with (ROOT / relative).open("rb") as handle:
                data = handle.read(16 * 1024 * 1024 + 1)
        else:
            data = inline
        require(len(data) <= 16 * 1024 * 1024)
        result.append({"name": name, "sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)})
    return result


def validate_probe(value, source, nonce, require_pass=False):
    require(type(value) is dict and set(value) == {
        "schema_version", "source_commit", "build_source_commit", "nonce", "complete_release", "passed", "phase", "failure",
        "checks", "fixtures", "derivatives", "joined_coordinators", "retained_workspace",
        "in_flight_cancellation_proven"})
    require(type(value["schema_version"]) is int and value["schema_version"] == 1)
    require(value["source_commit"] == source and value["build_source_commit"] == source and value["nonce"] == nonce)
    require(value["complete_release"] is False and value["in_flight_cancellation_proven"] is False)
    require(type(value["passed"]) is bool and type(value["retained_workspace"]) is bool)
    require(value["phase"] in PHASES and (value["failure"] is None or value["failure"] in FAILURES))
    checks = value["checks"]
    require(type(checks) is dict and set(checks) <= CHECKS and all(item is True for item in checks.values()))
    identities = fixture_identities()
    require(value["fixtures"] == identities)
    derivatives = value["derivatives"]
    require(type(derivatives) is list and len(derivatives) <= len(FIXTURES))
    for position, record in enumerate(derivatives):
        require(type(record) is dict and set(record) == {
            "fixture", "source_sha256", "record_sha256", "result_sha256", "text_sha256",
            "schema_version", "state", "parser"})
        require(record["fixture"] == FIXTURES[position][0] and record["source_sha256"] == identities[position]["sha256"])
        require(type(record["schema_version"]) is int and record["schema_version"] == 2)
        require(record["state"] == FIXTURES[position][3] and record["parser"] == FIXTURES[position][4])
        require(all(type(record[key]) is str and SHA.fullmatch(record[key]) for key in
                    ("source_sha256", "record_sha256", "result_sha256", "text_sha256")))
    require(type(value["joined_coordinators"]) is int and 0 <= value["joined_coordinators"] <= 4)
    if value["passed"] or require_pass:
        require(value["passed"] is True and value["failure"] is None and value["phase"] == "complete")
        require(set(checks) == CHECKS and len(derivatives) == 10 and value["joined_coordinators"] == 4)
        require(value["retained_workspace"] is False)
    return value


def write_report(path, value):
    temporary = path.with_suffix(".pending")
    temporary.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def source_identity():
    def git(*args):
        return subprocess.check_output(["git", *args], cwd=ROOT, stderr=subprocess.DEVNULL, timeout=20).decode().strip()
    source = git("rev-parse", "HEAD")
    tree = git("rev-parse", "HEAD^{tree}")
    parents = git("show", "-s", "--format=%P", "HEAD").split()
    require(COMMIT.fullmatch(source) and COMMIT.fullmatch(tree) and all(COMMIT.fullmatch(p) for p in parents))
    require(not git("status", "--porcelain"))
    return {"source_commit": source, "source_tree": tree, "source_parents": parents, "source_clean": True}


def campaign(binary, runtime, destination, identity, timeout=900):
    destination.mkdir(parents=True, exist_ok=True)
    report_path = destination / "coordinator-report.json"
    probe_path = destination / "coordinator-probe.receipt"
    stderr_path = destination / "coordinator-stderr.log"
    nonce = str(uuid.uuid4())
    report = {"schema_version": 1, **identity, "nonce": nonce,
              "scope": "Windows Server development coordinator probe; no Windows 11 or release acceptance",
              "complete_release": False, "passed": False, "failure": "not_started", "probe": None,
              "recorded_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    write_report(report_path, report)
    # An old success cannot survive a failed launch, timeout or invalid receipt.
    probe_path.unlink(missing_ok=True)
    try:
        report["binary_sha256"] = digest(binary, 512 * 1024 * 1024)
        report["runtime_manifests"] = {role: digest(runtime / role / "manifest.json", 1024 * 1024) for role in ("parser", "search")}
        report["fixtures"] = fixture_identities()
        report["failure"] = "probe_not_completed"
        write_report(report_path, report)
        with stderr_path.open("wb") as stderr:
            try:
                process = subprocess.run([str(binary.resolve()), "--report", str(probe_path.resolve()),
                                          "--runtime", str(runtime.resolve()), "--source", identity["source_commit"],
                                          "--nonce", nonce], cwd=ROOT, stdout=subprocess.DEVNULL, stderr=stderr,
                                         timeout=timeout, check=False)
                report["exit_code"] = process.returncode
            except subprocess.TimeoutExpired:
                report["failure"] = "timeout"
                report["timed_out"] = True
                process = None
        report["stderr_sha256"] = digest(stderr_path, 1024 * 1024)
        report["stderr_bytes"] = stderr_path.stat().st_size
        try:
            report["probe"] = validate_probe(read_json(probe_path), identity["source_commit"], nonce)
        except (OSError, ValueError, TypeError, KeyError):
            if process is not None:
                report["failure"] = "invalid_receipt"
        if process is not None and process.returncode != 0:
            report["failure"] = "probe_exit"
        elif process is not None:
            validate_probe(report["probe"], identity["source_commit"], nonce, require_pass=True)
            require(digest(binary, 512 * 1024 * 1024) == report["binary_sha256"])
            require(all(digest(runtime / role / "manifest.json", 1024 * 1024) == digest_value
                        for role, digest_value in report["runtime_manifests"].items()))
            require(source_identity() == identity)
            report["passed"] = True
            report["failure"] = None
    except (OSError, ValueError, TypeError, KeyError, subprocess.SubprocessError):
        if report["failure"] not in {"timeout", "probe_exit", "invalid_receipt"}:
            report["failure"] = "verification_failed"
    write_report(report_path, report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--runtime", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()
    args.destination.mkdir(parents=True, exist_ok=True)
    initial = {"schema_version": 1, "complete_release": False, "passed": False,
               "failure": "preflight_not_completed", "probe": None}
    write_report(args.destination / "coordinator-report.json", initial)
    try:
        require(platform.system() == "Windows")
        identity = source_identity()
        report = campaign(args.binary, args.runtime, args.destination, identity)
    except (OSError, ValueError, subprocess.SubprocessError):
        initial["failure"] = "preflight_rejected"
        write_report(args.destination / "coordinator-report.json", initial)
        print("Native coordinator preflight rejected; retained failed receipt.")
        return 1
    print("Native canonical parser campaign: " + ("passed development checks" if report["passed"] else "failed; retained receipt"))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
