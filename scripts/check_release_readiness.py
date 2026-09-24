"""Validate the offline, sanitized EW-08 readiness register; never query accounts."""
import argparse
from datetime import date, timedelta
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
REGISTER = Path('docs/delivery/readiness/register.v1.json')
REQUIREMENTS = {
    'macos_signing_access', 'windows_signing_access',
    'macos_signing_rehearsal', 'windows_signing_rehearsal',
    'windows11_clean_environment', 'macos_arm64_clean_environment',
    'macos_intel_clean_environment', 'benchmark_16gb_environment',
    'supported_os_matrix', 'maintainer_review', 'security_review',
}
STATUSES = {'unknown', 'unverified', 'confirmed', 'missing'}
KINDS = {'inspection', 'confirmation', 'unavailability', 'role_assignment'}


class InvalidRecord(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise InvalidRecord(message)


def text(value):
    return isinstance(value, str) and bool(value.strip())


def day(value):
    require(isinstance(value, str) and re.fullmatch(r'\d{4}-\d{2}-\d{2}', value),
            'dates must use YYYY-MM-DD')
    return date.fromisoformat(value)


def validate(record, root=ROOT, as_of=None):
    """Check consistency, evidence bytes and dates; human attestations remain reviewed claims."""
    as_of = as_of or date.today()
    require(type(record['schema_version']) is int and record['schema_version'] == 1,
            'unsupported register version')
    require(re.fullmatch(r'[0-9a-f]{40}', record['baseline_revision']),
            'baseline must be a full source revision')
    assessed = day(record['assessed_on'])
    start, end = day(record['programme_start']), day(record['programme_end'])
    require(start < end and start <= assessed <= as_of, 'invalid assessment window')
    evidence = record['evidence']
    require(isinstance(evidence, dict) and evidence, 'evidence records are required')
    for key, item in evidence.items():
        require(text(key) and item['kind'] in KINDS, 'invalid evidence kind')
        require(day(item['recorded_on']) <= assessed, 'invalid evidence date')
        relative = item['record']
        require(isinstance(relative, str) and relative.startswith('docs/delivery/readiness/'),
                'evidence must be a public readiness document')
        require('\\' not in relative and all(part not in {'', '.', '..'}
                for part in relative.split('/')), 'unsafe evidence path')
        path = root
        for part in relative.split('/'):
            path = path / part
            require(not path.is_symlink(), 'evidence links are not accepted')
        require(path.is_file() and path.stat().st_size <= 1024 * 1024,
                'evidence must be a bounded local file')
        require(re.fullmatch(r'[0-9a-f]{64}', item['sha256']), 'invalid evidence checksum')
        require(hashlib.sha256(path.read_bytes()).hexdigest() == item['sha256'],
                'evidence checksum mismatch')
    items = record['requirements']
    require(isinstance(items, list) and all(isinstance(item, dict) for item in items),
            'requirements must be records')
    ids = [item['id'] for item in items]
    require(len(ids) == len(REQUIREMENTS) and set(ids) == REQUIREMENTS,
            'requirements missing, duplicated or unknown')
    counts = dict.fromkeys(sorted(STATUSES), 0)
    unresolved, overdue = [], []
    for item in items:
        key, status = item['id'], item['status']
        require(status in STATUSES, f'{key}: invalid status')
        require(all(text(item[field]) for field in (
            'requirement', 'responsible_role', 'required_evidence', 'next_action')),
            f'{key}: descriptive fields must be nonempty')
        require(day(item['assessed_on']) == assessed, f'{key}: inconsistent assessment date')
        deadline = start + timedelta(days=27 if key.endswith('_rehearsal') else 13)
        due = day(item['needed_by'])
        later = day(item['later_checkpoint']['due_on'])
        require(start <= due <= deadline and due <= later <= min(end, start + timedelta(days=83)),
                f'{key}: checkpoint falls outside its sprint deadline')
        require(text(item['later_checkpoint']['purpose']), f'{key}: missing later checkpoint')
        for field in ('evidence', 'confirmation_evidence', 'assignment_evidence'):
            refs = item[field]
            require(isinstance(refs, list) and all(isinstance(ref, str) for ref in refs),
                    f'{key}: invalid evidence references')
            require(len(refs) == len(set(refs)) and all(ref in evidence for ref in refs),
                    f'{key}: duplicate or unknown evidence reference')
        require(item['evidence'], f'{key}: assessment evidence required')
        assigned = item['assignment_status']
        require(assigned in {'unassigned', 'confirmed'}, f'{key}: invalid role assignment')
        require(bool(item['assignment_evidence']) == (assigned == 'confirmed'),
                f'{key}: assignment needs an explicit record')
        require(all(evidence[ref]['kind'] == 'role_assignment'
                    for ref in item['assignment_evidence']), f'{key}: wrong assignment evidence')
        confirmation = item['confirmation_evidence']
        if status in {'confirmed', 'missing'}:
            kind = 'confirmation' if status == 'confirmed' else 'unavailability'
            require(confirmation and all(evidence[ref]['kind'] == kind for ref in confirmation),
                    f'{key}: status requires appropriate confirmation evidence')
            if status == 'confirmed':
                require(assigned == 'confirmed', f'{key}: confirmed access needs a responsible role')
        else:
            require(not confirmation, f'{key}: unresolved status cannot claim confirmation')
        counts[status] += 1
        if status != 'confirmed':
            unresolved.append(key)
            if due < as_of:
                overdue.append(key)
    return {'schema_version': 1, 'record_valid': True, 'assessed_on': str(assessed),
            'checked_as_of': str(as_of), 'ready': not unresolved, 'counts': counts,
            'unresolved': unresolved, 'overdue': overdue}


def reject_duplicate_keys(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'duplicate JSON key')
        result[key] = value
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--require-ready', action='store_true',
                        help='also fail while any access or decision remains unconfirmed')
    parser.add_argument('--as-of', type=day, default=date.today(),
                        help='evaluate overdue items at YYYY-MM-DD (default: local date)')
    args = parser.parse_args()
    try:
        path = ROOT / REGISTER
        require(path.stat().st_size <= 1024 * 1024, 'register exceeds size bound')
        record = json.loads(path.read_text(), object_pairs_hook=reject_duplicate_keys)
        result = validate(record, as_of=args.as_of)
    except (ValueError, TypeError, KeyError, OSError) as exc:
        print(json.dumps({'record_valid': False, 'ready': False, 'error': str(exc)}))
        return 1
    print(json.dumps(result, indent=2))
    return 1 if args.require_ready and not result['ready'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
