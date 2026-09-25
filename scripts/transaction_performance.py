"""Failure-retaining development timings of the real canonical Rust transaction paths.

No GUI, network, fixture SQL writes or production algorithm substitutions. A fresh
process is not a cold filesystem cache. This is not the complete EW-36 workload.
"""
import argparse
import datetime as dt
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import signal
import subprocess
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
OPERATIONS = ("view", "patterns_all", "patterns_filtered", "patterns_empty", "comparison", "html_export")
SAMPLES = 4


def digest(path):
    sha = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            sha.update(block)
    return sha.hexdigest()


def sources():
    paths = [ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "crates/core/Cargo.toml"]
    paths += sorted((ROOT / "crates/core/src").rglob("*.rs"))
    paths += sorted((ROOT / "crates/core/examples").rglob("*.rs"))
    return {str(path.relative_to(ROOT)): digest(path) for path in paths}


def capture(args):
    return subprocess.check_output(args, cwd=ROOT, text=True, timeout=10).strip()


def host():
    result = {"os": platform.system(), "architecture": platform.machine(),
              "os_version": platform.mac_ver()[0] if platform.system() == "Darwin" else platform.release(),
              "logical_cpus": os.cpu_count(), "physical_memory_bytes": None, "cpu_model": None,
              "power_thermal_state": "not_controlled_or_measured", "exclusive_reservation": False}
    try:
        if platform.system() == "Darwin":
            result["physical_memory_bytes"] = int(capture(["sysctl", "-n", "hw.memsize"]))
            result["cpu_model"] = capture(["sysctl", "-n", "machdep.cpu.brand_string"])
        elif platform.system() == "Linux":
            result["physical_memory_bytes"] = os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES")
            for line in Path("/proc/cpuinfo").read_text().splitlines():
                if line.startswith("model name"):
                    result["cpu_model"] = line.partition(":")[2].strip()
                    break
    except (OSError, ValueError, subprocess.SubprocessError):
        result["host_measurement_incomplete"] = True
    return result


def peak_rss(stderr, system):
    """The time wrapper measures a child's whole lifetime, including restore/oracle work."""
    pattern = (r"(?m)^\s*(\d+)\s+maximum resident set size\s*$" if system == "Darwin"
               else r"(?m)^\s*Maximum resident set size \(kbytes\):\s*(\d+)\s*$")
    matches = re.findall(pattern, stderr)
    return int(matches[-1]) * (1 if system == "Darwin" else 1024) if matches else None


def events(text):
    output = []
    for line in text.splitlines():
        try:
            value = json.loads(line)
            if isinstance(value, dict):
                output.append(value)
        except ValueError:
            pass  # A malformed/truncated line cannot establish a completed sample.
    return output


def summarize(records, operation, expected=SAMPLES):
    samples = [v for v in records if v.get("event") == "sample" and v.get("operation") == operation]
    valid = (len(samples) == expected and [v.get("sample") for v in samples] == list(range(expected))
             and all(v.get("oracle_passed") is True and type(v.get("elapsed_ms")) in (int, float)
                     and math.isfinite(v["elapsed_ms"]) and v["elapsed_ms"] >= 0 for v in samples)
             and sum(v.get("event") == "complete" and v.get("operation") == operation
                     and v.get("samples") == expected for v in records) == 1
             and not any(v.get("event") == "failure" for v in records))
    warm = [v["elapsed_ms"] for v in samples[1:] if type(v.get("elapsed_ms")) in (int, float)
            and math.isfinite(v["elapsed_ms"]) and v["elapsed_ms"] >= 0]
    return {"measurement_complete": valid, "samples": samples,
            "first_call_ms": samples[0].get("elapsed_ms") if samples else None,
            "warm_sample_count": len(warm), "warm_max_ms": max(warm) if warm else None,
            "warm_p95_nearest_rank_ms": sorted(warm)[math.ceil(.95*len(warm))-1] if warm else None,
            "p95_qualified": False, "under_two_seconds_gate_passed": False}


def invoke(command, directory, name, timeout):
    stdout_path, stderr_path = directory / f"{name}.stdout.jsonl", directory / f"{name}.stderr.txt"
    system = platform.system()
    measured = command
    memory_method = "unavailable"
    if system in ("Darwin", "Linux") and Path("/usr/bin/time").is_file():
        measured = ["/usr/bin/time", "-l" if system == "Darwin" else "-v", *command]
        memory_method = "time_child_lifetime_maximum_rss_including_restore_and_oracle"
    start = time.perf_counter()
    timed_out = False
    with stdout_path.open("x") as out, stderr_path.open("x") as err:
        child = subprocess.Popen(measured, cwd=ROOT, stdout=out, stderr=err,
                                 env=dict(os.environ, LC_ALL="C"), start_new_session=True)
        try:
            returncode = child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            os.killpg(child.pid, signal.SIGKILL)
            returncode = child.wait(timeout=10)
    elapsed = (time.perf_counter()-start)*1000
    stderr = stderr_path.read_text(errors="replace")
    return {"returncode": returncode, "timed_out": timed_out, "process_elapsed_ms": elapsed,
            "child_lifetime_peak_rss_bytes": peak_rss(stderr, system), "memory_method": memory_method,
            "stdout": stdout_path.name, "stdout_sha256": digest(stdout_path),
            "stderr": stderr_path.name, "stderr_sha256": digest(stderr_path),
            "events": events(stdout_path.read_text(errors="replace"))}


def save(path, report):
    temporary = path.with_suffix(".pending")
    temporary.write_text(json.dumps(report, indent=2, allow_nan=False) + "\n")
    os.replace(temporary, path)


def logical_bytes(directory):
    """Retained synthetic run files only; logical bytes, not allocated blocks or peak disk."""
    return sum(path.stat().st_size for path in directory.rglob("*") if path.is_file())


def run(mode, reuse_build=False):
    directory = ROOT / "artifacts/transaction-performance" / f"{dt.datetime.now(dt.timezone.utc):%Y%m%dT%H%M%S}-{uuid.uuid4().hex}"
    directory.mkdir(parents=True)
    path = directory / "report.json"
    report = {"schema_version": 1, "scope": "development canonical transaction baseline only",
              "observed_at": dt.datetime.now(dt.timezone.utc).isoformat(), "mode": mode,
              "rows": 1000 if mode == "smoke" else 100_000, "host": host(),
              "source_revision": capture(["git", "rev-parse", "HEAD"]),
              "source_dirty": bool(capture(["git", "status", "--porcelain"])),
              "source_sha256": sources(), "runner_sha256": digest(Path(__file__)),
              "phase": "build", "outcome": "incomplete", "complete_release": False,
              "operations": {name: {"outcome": "not_run"} for name in OPERATIONS},
              "limits": {"setup_seconds": 300, "each_operation_process_seconds": 60, "total_samples_per_operation": SAMPLES},
              "unverified": ["OS page-cache cold measurements", "ordinary JavaScript ledger filtering and UI responsiveness",
                             "10,000 pages including 1,000 scans", "concurrent maps/graphs and workers",
                             "OCR/search throughput", "cancel latency", "minimum-host or clean installation qualification",
                             "whole-application peak memory", "statistically qualified p95"]}
    save(path, report)
    try:
        if platform.system() not in ("Darwin", "Linux"):
            raise ValueError("Development runner currently supports macOS/Linux only; Windows remains unmeasured")
        binary = ROOT / "target/release/examples/performance_baseline"
        receipt = ROOT / "artifacts/transaction-performance-build.json"
        if reuse_build:
            built = json.loads(receipt.read_text())
            if built["source_sha256"] != report["source_sha256"] or built["binary_sha256"] != digest(binary):
                raise ValueError("Build reuse refused: binary or Rust sources differ from build receipt")
            report["build"] = dict(built, reused=True)
        else:
            build = invoke(["cargo", "build", "--locked", "--release", "-p", "workbench-core", "--example", "performance_baseline"], directory, "build", 600)
            report["build"] = build
            if build["returncode"] != 0:
                raise ValueError("Release example build failed")
            if sources() != report["source_sha256"]:
                raise ValueError("Rust sources changed during build")
            built = {"source_sha256": report["source_sha256"], "binary_sha256": digest(binary),
                     "profile": "release", "rustc": capture(["rustc", "--version"]),
                     "command": "cargo build --locked --release -p workbench-core --example performance_baseline"}
            receipt.write_text(json.dumps(built, indent=2)+"\n")
            report["build"].update(built)
        report["phase"] = "canonical_setup"
        save(path, report)
        setup = invoke([str(binary), "setup", str(directory), str(report["rows"]), "0"], directory, "setup", 300)
        report["setup"] = setup
        completed = [v for v in setup["events"] if v.get("event") == "setup_complete"]
        if setup["returncode"] != 0 or len(completed) != 1:
            raise ValueError("Canonical setup failed or timed out; queries remain not_run")
        report["fixture"] = completed[0]["fixture"]
        report["retained_logical_bytes_after_setup"] = logical_bytes(directory)
        for operation in OPERATIONS:
            report["phase"] = operation
            report["operations"][operation] = {"outcome": "running"}
            save(path, report)
            observed = invoke([str(binary), "measure", str(directory), operation, str(SAMPLES)], directory, operation, 60)
            observed.update(summarize(observed["events"], operation))
            observed["outcome"] = "measured" if observed["returncode"] == 0 and observed["measurement_complete"] else "failed"
            report["operations"][operation] = observed
            save(path, report)
        report["phase"] = "complete"
        report["outcome"] = "measured" if all(v["outcome"] == "measured" for v in report["operations"].values()) else "failed"
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        report["outcome"] = "failed"
        # Local retained diagnostics may contain paths; they are never automatically committed.
        report["failure"] = str(error)
    report["finished_at"] = dt.datetime.now(dt.timezone.utc).isoformat()
    report["retained_logical_bytes_at_end"] = logical_bytes(directory)
    save(path, report)
    print(json.dumps({"report": str(path.relative_to(ROOT)), "outcome": report["outcome"],
                      "complete_release": False, "operations": {k:v["outcome"] for k,v in report["operations"].items()}}, indent=2))
    return 0 if report["outcome"] == "measured" else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=("smoke", "baseline"), required=True)
    parser.add_argument("--reuse-build", action="store_true", help="Require matching retained source/binary build receipt")
    arguments = parser.parse_args()
    raise SystemExit(run(arguments.mode, arguments.reuse_build))
