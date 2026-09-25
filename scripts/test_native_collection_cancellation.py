"""Opt-in fixed Mac owned-response cancellation proof; no queries without reviewed source approval."""
import argparse
import json
from pathlib import Path
import uuid

from test_native_durable_https import ROOT, observe


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-fixed-cancellation", action="store_true")
    parser.add_argument("--allowed-signers", type=Path, required=True)
    args = parser.parse_args()
    artifact = ROOT / "artifacts/native-collection-cancellation" / str(uuid.uuid4())
    artifact.mkdir(parents=True, mode=0o700)
    report = observe(artifact, args.allow_fixed_cancellation, args.allowed_signers.resolve(), cancellation=True)
    print(json.dumps({"artifact_id": artifact.name, **{k: report[k] for k in
                     ("passed", "outcome", "phase", "failure") if k in report}}))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
