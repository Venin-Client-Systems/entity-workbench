"""Read retained canonical benchmark workspaces; quantify payload bytes, never benchmark latency."""
import argparse
import datetime as dt
import json
from pathlib import Path
import re
import uuid

import transaction_performance as baseline


def html_sections(data):
    if len(data) > 256 * 1024 * 1024:
        raise ValueError("Diagnostic HTML exceeds 256 MiB")
    starts = [m.start() for m in re.finditer(b"<h2>", data)]
    if not 1 <= len(starts) <= 32:
        raise ValueError("Unexpected generated HTML section count")
    result = [{"section": "document_prefix", "raw_utf8_bytes": starts[0]}]
    for index, start in enumerate(starts):
        end = starts[index+1] if index+1 < len(starts) else len(data)
        label_end = data.find(b"</h2>", start, start+256)
        if label_end < 0:
            raise ValueError("Missing bounded generated heading")
        result.append({"section": data[start+4:label_end].decode("utf-8"), "raw_utf8_bytes": end-start})
    if sum(v["raw_utf8_bytes"] for v in result) != len(data):
        raise ValueError("HTML section partition is incomplete")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", required=True, help="Existing baseline run directory name, not a path")
    args = parser.parse_args()
    if re.fullmatch(r"\d{8}T\d{6}-[0-9a-f]{32}", args.run) is None:
        parser.error("Use the fixed retained run identifier")
    root = baseline.ROOT / "artifacts/transaction-performance" / args.run
    if not root.is_dir():
        parser.error("Retained run not found")
    directory = root / f"payload-diagnostic-{uuid.uuid4().hex}"
    directory.mkdir()
    path = directory / "diagnostic.json"
    report = {"schema_version": 1, "scope": "separate canonical payload byte diagnostic",
              "observed_at": dt.datetime.now(dt.timezone.utc).isoformat(),
              "outcome": "incomplete", "timing_claim": False, "complete_release": False,
              "baseline_report_sha256": baseline.digest(root / "report.json"),
              "source_revision": baseline.capture(["git", "rev-parse", "HEAD"]),
              "source_dirty": bool(baseline.capture(["git", "status", "--porcelain"])),
              "source_sha256": baseline.sources(), "script_sha256": baseline.digest(Path(__file__)),
              "runner_sha256": baseline.digest(Path(baseline.__file__)),
              "cases": {name: {"outcome": "not_run"} for name in ("baseline", "one_report")}}
    baseline.save(path, report)
    try:
        report["build"] = baseline.invoke(["cargo", "build", "--locked", "--release", "-p", "workbench-core", "--example", "performance_baseline"], directory, "build", 600)
        if report["build"]["returncode"] != 0 or baseline.sources() != report["source_sha256"]:
            raise ValueError("Diagnostic build failed or sources changed")
        report["rustc"] = baseline.capture(["rustc", "--version"])
        fixture = json.loads((root / "fixture.json").read_text())
        if fixture["rows"] != 100_000 or baseline.digest(root / "fixture.csv") != fixture["fixture_sha256"]:
            raise ValueError("Retained original fixture differs")
        binary = baseline.ROOT / "target/release/examples/performance_baseline"
        report["binary_sha256"] = baseline.digest(binary)
        report["fixture_sha256"] = fixture["fixture_sha256"]
        for case in report["cases"]:
            observed = baseline.invoke([str(binary), "diagnose", str(root), case, "0"], directory, case, 60)
            found = [v for v in observed["events"] if v.get("event") == "payload_diagnostic" and v.get("case") == case]
            if observed["returncode"] != 0 or len(found) != 1:
                report["cases"][case] = dict(observed, outcome="failed")
                raise ValueError("Canonical payload diagnostic failed")
            report["cases"][case] = dict(observed, outcome="measured", payload=found[0])
            baseline.save(path, report)
        snapshot = report["cases"]["one_report"]["payload"]["reports_html"][0]
        exported = root / "measure-html_export/exports" / (snapshot["id"] + ".html")
        if exported.stat().st_size > 256*1024*1024 or baseline.digest(exported) != snapshot["sha256"]:
            raise ValueError("Retained HTML differs from canonical snapshot")
        data = exported.read_bytes()
        report["html"] = {"sha256": snapshot["sha256"], "raw_utf8_bytes": len(data),
                          "section_partition": html_sections(data)}
        if baseline.digest(root / "report.json") != report["baseline_report_sha256"]:
            raise ValueError("Original timing report changed")
        if baseline.sources() != report["source_sha256"]:
            raise ValueError("Diagnostic sources changed during read")
        report["outcome"] = "measured"
    except (OSError, ValueError, KeyError, IndexError) as error:
        report["outcome"] = "failed"
        report["failure"] = str(error)
    baseline.save(path, report)
    print(json.dumps({"diagnostic": str(path.relative_to(baseline.ROOT)), "outcome": report["outcome"], "timing_claim": False}, indent=2))
    return 0 if report["outcome"] == "measured" else 1


if __name__ == "__main__":
    raise SystemExit(main())
