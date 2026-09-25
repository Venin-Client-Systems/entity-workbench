"""Independent Python CSV-reader checks against real canonical Rust CLI responses.

This is a small synthetic interoperability test, not a spreadsheet application test.
No network, GUI, native export handler or database mutation outside Rust is used.
"""
import argparse
import csv
import datetime as dt
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
COLUMNS = (
    ("workspace_revision", "unsigned_integer", "uint:", False),
    ("id", "text", "text:", False),
    ("version", "unsigned_integer", "uint:", False),
    ("account", "text", "text:", False),
    ("date", "calendar_date_text", "date:", False),
    ("posting_date", "calendar_date_text", "date:", True),
    ("description", "text", "text:", False),
    ("amount", "exact_decimal_text", "decimal:", False),
    ("currency", "text", "text:", False),
    ("balance", "exact_decimal_text", "decimal:", True),
    ("anchor", "source_anchor_json", "json:", False),
    ("review", "text", "text:", False),
    ("duplicate_candidates", "string_array_json", "json:", False),
    ("transfer_peer", "text", "text:", True),
    ("merchant", "text", "text:", True),
)
SOURCES = (
    "crates/core/src/transaction_csv.rs",
    "crates/core/src/store/transaction_csv.rs",
    "crates/core/src/store/transaction_csv_tests.rs",
    "crates/core/src/store/transaction_export.rs",
    "crates/core/src/store/transaction_export_selection.rs",
    "crates/core/src/domain.rs", "crates/core/src/bin/ew-dev.rs",
    "crates/core/src/lib.rs", "crates/core/src/store.rs",
    "Cargo.lock", "Cargo.toml", "crates/core/Cargo.toml",
    "scripts/verify_transaction_csv.py", "scripts/tests/test_transaction_csv.py",
)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(value):
    return hashlib.sha256(value).hexdigest()


def compact(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True).encode()


def decode(response):
    """Decode only this version's declared literals; never evaluate spreadsheet syntax."""
    require(response["schema_version"] == 1 and response["format"] == "typed_literal_v1",
            "Unexpected CSV contract")
    data = response["csv"].encode("utf-8")
    require(data.startswith(b"\xef\xbb\xbf") and data.endswith(b"\r\n"), "Missing BOM/terminator")
    require(response["bytes"] == len(data) and response["sha256"] == digest(data),
            "CSV byte identity mismatch")
    dictionary = response["dictionary"]
    require(dictionary["null_literal"] == "null", "Unknown missing value encoding")
    actual = tuple((c["name"], c["logical_type"], c["prefix"], c["nullable"])
                   for c in dictionary["columns"])
    require(actual == COLUMNS, "Unexpected or duplicate dictionary columns")
    require(response["format_sha256"] == digest(compact([response["format"], dictionary])),
            "Format/dictionary identity mismatch")
    reader = csv.reader(io.StringIO(data.decode("utf-8-sig"), newline=""), strict=True)
    require(next(reader) == [c[0] for c in COLUMNS], "Unexpected header")
    rows = []
    for fields in reader:
        require(len(fields) == len(COLUMNS), "Wrong field count")
        row = {}
        for field, (name, logical_type, prefix, nullable) in zip(fields, COLUMNS):
            if field == "null":
                require(nullable, "Missing non-nullable field")
                value = None
            else:
                require(field.startswith(prefix), "Missing literal prefix")
                value = field[len(prefix):]
                if logical_type == "unsigned_integer":
                    require(re.fullmatch(r"0|[1-9][0-9]*", value) is not None,
                            "Invalid unsigned integer")
                    value = int(value)
                    require(value <= 2**64 - 1, "Unsigned integer overflow")
                elif logical_type in ("source_anchor_json", "string_array_json"):
                    value = json.loads(value)
            row[name] = value
        require(row.pop("workspace_revision") == response["workspace_revision"],
                "Row revision mismatch")
        rows.append(row)
    require(len(rows) == response["row_count"], "Incomplete row count")
    return rows


def fixture():
    output = io.StringIO(newline="")
    writer = csv.writer(output, lineterminator="\r\n")
    writer.writerow(["account", "date", "description", "amount", "currency"])
    values = ["=1+1", "+SUM(A1)", "-2+3", "@SUM(A1)", "  =1", "\t=1", "\r=1",
              "＝１", "＋１", "－１", "＠name", 'line1\r\n=HYPERLINK("https://example.invalid")',
              'quote"comma,semi;', "Καλημέρα 🚲", "null", "plain"]
    for number in range(32):
        amount = ("79228162514264337593543950335" if number == 0 else
                  "-79228162514264337593543950335" if number == 1 else "0.00000001")
        writer.writerow([f"{number % 3:06}", f"2024-02-{27 + number % 3:02}",
                         values[number % len(values)], amount, "AUD" if number % 2 == 0 else "USD"])
    return output.getvalue().encode()


def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def run(binary, output, baseline=None):
    output.mkdir(parents=True, exist_ok=False)
    report_path = output / "report.json"
    report = {"schema_version": 1, "outcome": "incomplete", "complete_release": False,
              "scope": "synthetic Rust CLI to independent Python csv.reader",
              "at": dt.datetime.now(dt.timezone.utc).isoformat(), "checks": [],
              "spreadsheet_application_executed": False, "network_requests": 0}
    save(report_path, report)
    calls = 0
    last_stdout = b""
    try:
        source = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, check=True,
                                capture_output=True, text=True, timeout=10).stdout.strip()
        dirty = subprocess.run(["git", "status", "--porcelain"], cwd=ROOT, check=True,
                               capture_output=True, text=True, timeout=10).stdout
        report.update(source_commit=source, source_dirty=bool(dirty),
                      source_sha256={p: digest((ROOT / p).read_bytes()) for p in SOURCES},
                      binary_sha256=digest(binary.read_bytes()))
        if baseline:
            report["baseline_binary_sha256"] = digest(baseline.read_bytes())
            require(report["baseline_binary_sha256"] != report["binary_sha256"],
                    "Current and baseline binary are identical; rebuild the intended source")
        save(report_path, report)
        workspace = output / "synthetic-workspace"

        def call(command, expected_ok=True, executable=binary):
            nonlocal calls, last_stdout
            calls += 1
            result = subprocess.run([str(executable), str(workspace), "--summary"],
                                    input=compact(command), capture_output=True, timeout=30)
            # Fixture is fixed/small; this is a post-capture sanity bound, not a pipe-memory quota.
            require(len(result.stdout) <= 8 * 1024 * 1024, "Unexpected response size")
            last_stdout = result.stdout
            (output / f"response-{calls:03}.json").write_bytes(result.stdout)
            response = json.loads(result.stdout)
            require((result.returncode == 0) == expected_ok, "Unexpected CLI success/failure")
            require(("error" not in response) == expected_ok, "Unexpected CLI error contract")
            return response

        def checked(name):
            report["checks"].append(name)
            save(report_path, report)

        raw_fixture = fixture()
        report["fixture_sha256"] = digest(raw_fixture)
        report["fixture_bytes"] = len(raw_fixture)
        (output / "fixture.csv").write_bytes(raw_fixture)
        view = call({"action": "import", "name": "typed-literal-synthetic.csv", "bytes": list(raw_fixture)})
        revision = view["workspace"]["revision"]
        default_filter = {"date_from": None, "date_to": None, "account": None, "currency": None, "review": None}
        selection = {"query": "", "filter": default_filter, "order": "date_ascending"}
        raw = call({"action": "export_transactions", "request": selection, "expected_revision": revision})
        canonical_rows = json.loads(raw["json"])
        for row, state in zip(canonical_rows, ("accepted", "rejected", "deferred")):
            view = call({"action": "review_transaction", "id": row["id"], "state": state,
                         "reason": "Fixed synthetic CSV interoperability check", "expected_revision": revision})
            revision = view["workspace"]["revision"]
        before = call({"action": "view"})
        hashes = set()
        for index, patch in enumerate(({}, {"order": "date_descending"},
                                      {"filter": {"account": "000001"}}, {"filter": {"currency": "USD"}},
                                      {"filter": {"date_from": "2024-02-29", "date_to": "2024-02-29"}},
                                      {"query": "HYPERLINK"}, {"query": "absent synthetic phrase"},
                                      {"filter": {"review": "accepted"}})):
            selected = {**selection, **patch, "filter": {**default_filter, **patch.get("filter", {})}}
            raw_command = {"action": "export_transactions", "request": selected, "expected_revision": revision}
            raw = call(raw_command)
            if baseline:
                current_stdout = last_stdout
                require(call(raw_command, executable=baseline) == raw, "Historical raw JSON changed")
                require(last_stdout == current_stdout, "Historical CLI response bytes changed")
            response = call({"action": "export_transaction_csv", "expected_revision": revision,
                             "request": {"selection": selected, "non_accepted": "allow_selected"}})
            require(response["request"]["selection"] == selected, "Scope echo changed")
            require(response["selection_sha256"] == raw["query_sha256"], "Scope identity differs")
            require(response["matching"] == raw["matching"], "Matching profile differs")
            require(decode(response) == json.loads(raw["json"]), "CSV differs from exact canonical JSON")
            if index == 0:
                require(response["row_count"] == 32, "Full selection lost rows")
                require("decimal:79228162514264337593543950335" in response["csv"], "Extreme amount lost")
                require("text:\t=1" in response["csv"] and "text:＝１" in response["csv"], "Formula literal lost")
                (output / "typed-literal.csv").write_bytes(response["csv"].encode())
            hashes.add(response["format_sha256"])
            checked(f"scope_{index}_independent_round_trip" + ("_and_historical_json_bytes" if baseline else ""))
        require(len(hashes) == 1, "Dictionary changed with row content")
        command = {"action": "export_transaction_csv", "expected_revision": revision,
                   "request": {"selection": selection, "non_accepted": "reject"}}
        call(command, expected_ok=False)
        checked("mixed_review_reject_is_whole_selection_failure")
        command["request"]["selection"] = {**selection, "filter": {**default_filter, "review": "accepted"}}
        require(len(decode(call(command))) == 1, "Explicit accepted selection failed")
        checked("explicit_accepted_selection_with_reject")
        command["expected_revision"] -= 1
        call(command, expected_ok=False)
        checked("stale_revision_refused")
        command["expected_revision"] = revision
        del command["request"]["non_accepted"]
        call(command, expected_ok=False)
        checked("missing_nonaccepted_policy_refused")
        require(call({"action": "view"}) == before, "Read-only exports mutated canonical summary")
        require((workspace / "originals" / digest(raw_fixture)).read_bytes() == raw_fixture,
                "Original bytes changed")
        require(digest(binary.read_bytes()) == report["binary_sha256"], "Binary changed during verification")
        require({p: digest((ROOT / p).read_bytes()) for p in SOURCES} == report["source_sha256"],
                "Source changed during verification")
        checked("canonical_summary_original_binary_and_source_unchanged")
        report["historical_json_artifact_and_envelope_byte_identity"] = True if baseline else None
        report["outcome"] = "passed"
    except Exception as error:
        report.update(outcome="failed", error_type=type(error).__name__)
        # Avoid leaking workspace paths from subprocess/OS exception messages.
        if isinstance(error, ValueError):
            report["failure"] = str(error)[:300]
    finally:
        report["command_count"] = calls
        save(report_path, report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/ew-dev")
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--output", type=Path, required=True, help="New ignored/private observation directory")
    args = parser.parse_args()
    report = run(args.binary.resolve(), args.output.resolve(), args.baseline.resolve() if args.baseline else None)
    print(json.dumps({"outcome": report["outcome"], "checks": len(report["checks"]),
                      "commands": report["command_count"]}))
    return 0 if report["outcome"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
