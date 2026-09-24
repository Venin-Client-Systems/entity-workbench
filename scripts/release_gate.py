"""Check release policy or verify a complete local candidate, without running evidence."""
import argparse
import json
from pathlib import Path

from release_evidence import ROOT, evaluate


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check-policy', action='store_true', help='Validate declarations/receipts only; never report a verified complete release')
    parser.add_argument('--ledger', type=Path, default=ROOT / 'docs/release-gates.json')
    parser.add_argument('--evidence-root', type=Path, default=ROOT / 'docs/release-evidence')
    parser.add_argument('--artifact-root', type=Path)
    args = parser.parse_args()
    report = evaluate(args.ledger, args.evidence_root, args.artifact_root, args.check_policy)
    print(json.dumps(report, indent=2, sort_keys=True))
    if args.check_policy:
        return 0 if report['policy_valid'] and not report['errors'] else 1
    return 0 if report['complete_release'] and not report['errors'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
