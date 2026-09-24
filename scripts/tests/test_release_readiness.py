"""Synthetic consistency checks do not confirm real credentials, hardware or people."""
import copy
from datetime import date
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import subprocess
import sys
from types import SimpleNamespace
from unittest.mock import patch
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('readiness', ROOT / 'scripts/check_release_readiness.py')
readiness = importlib.util.module_from_spec(spec)
spec.loader.exec_module(readiness)


class ReadinessTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.record = {
            'schema_version': 1, 'baseline_revision': 'a' * 40,
            'programme_start': '2026-09-23', 'programme_end': '2026-12-23',
            'assessed_on': '2026-09-24', 'evidence': {}, 'requirements': [],
        }
        evidence_path = self.root / 'docs/delivery/readiness/initial-audit.md'
        evidence_path.parent.mkdir(parents=True)
        self.add_evidence('initial-audit', 'inspection')
        for key in sorted(readiness.REQUIREMENTS):
            self.record['requirements'].append({
                'id': key, 'requirement': 'Synthetic requirement',
                'responsible_role': 'Synthetic role', 'assignment_status': 'unassigned',
                'assignment_evidence': [], 'status': 'unknown',
                'assessed_on': '2026-09-24',
                'needed_by': '2026-10-20' if key.endswith('_rehearsal') else '2026-10-06',
                'evidence': ['initial-audit'], 'confirmation_evidence': [],
                'required_evidence': 'Synthetic proof', 'next_action': 'Confirm access',
                'later_checkpoint': {'due_on': '2026-12-15', 'purpose': 'Retest candidate'},
            })
        self.as_of = date(2026, 9, 24)

    def check(self):
        return readiness.validate(self.record, root=self.root, as_of=self.as_of)

    def add_evidence(self, key, kind, recorded_on='2026-09-24', valid_until='2026-12-15'):
        path = self.root / f'docs/delivery/readiness/{key}.md'
        path.write_text('Synthetic test attestation; no real resource is confirmed.\n')
        self.record['evidence'][key] = {
            'kind': kind, 'record': path.relative_to(self.root).as_posix(),
            'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
            'recorded_on': recorded_on,
            'valid_until': valid_until if kind == 'confirmation' else None,
        }

    def test_unresolved_record_is_consistent_but_not_ready(self):
        result = self.check()
        self.assertFalse(result['ready'])
        self.assertEqual(len(result['unresolved']), 11)
        self.assertFalse(result['overdue'])
        self.assertEqual(result['counts']['confirmed'], 0)
        self.assertEqual(result['counts']['missing'], 0)

    def test_repository_record_is_consistent(self):
        record = json.loads((ROOT / readiness.REGISTER).read_text())
        result = readiness.validate(record, root=ROOT,
                                    as_of=date.fromisoformat(record['assessed_on']))
        self.assertTrue(result['record_valid'])

    def test_explicit_confirmation_can_establish_access_readiness(self):
        self.add_evidence('availability', 'confirmation')
        self.add_evidence('custodian', 'role_assignment')
        for item in self.record['requirements']:
            item['status'] = 'confirmed'
            item['confirmation_evidence'] = ['availability']
            item['assignment_status'] = 'confirmed'
            item['assignment_evidence'] = ['custodian']
        self.assertTrue(self.check()['ready'])
        self.record['requirements'][0]['assignment_evidence'] = ['availability']
        with self.assertRaises(readiness.InvalidRecord):
            self.check()

    def test_confirmation_requires_evidence_and_responsible_role(self):
        item = self.record['requirements'][0]
        item['status'] = 'confirmed'
        with self.assertRaises(readiness.InvalidRecord):
            self.check()
        item['confirmation_evidence'] = ['initial-audit']
        with self.assertRaises(readiness.InvalidRecord):
            self.check()
        self.add_evidence('availability', 'confirmation')
        item['confirmation_evidence'] = ['availability']
        with self.assertRaises(readiness.InvalidRecord):
            self.check()
        self.add_evidence('custodian', 'role_assignment')
        item['assignment_status'] = 'confirmed'
        item['assignment_evidence'] = ['custodian']
        self.assertEqual(self.check()['counts']['confirmed'], 1)

    def test_missing_requires_explicit_unavailability(self):
        item = self.record['requirements'][0]
        item['status'] = 'missing'
        with self.assertRaises(readiness.InvalidRecord):
            self.check()
        self.add_evidence('unavailable', 'unavailability')
        item['confirmation_evidence'] = ['unavailable']
        self.assertEqual(self.check()['counts']['missing'], 1)

    def test_unresolved_cannot_claim_confirmation(self):
        self.record['requirements'][0]['confirmation_evidence'] = ['initial-audit']
        with self.assertRaises(readiness.InvalidRecord):
            self.check()

    def test_requirement_cannot_be_deleted_or_duplicated(self):
        original = copy.deepcopy(self.record)
        self.record['requirements'].pop()
        with self.assertRaises(readiness.InvalidRecord):
            self.check()
        self.record = original
        self.record['requirements'][-1] = self.record['requirements'][0]
        with self.assertRaises(readiness.InvalidRecord):
            self.check()

    def test_changed_evidence_is_rejected(self):
        path = self.root / self.record['evidence']['initial-audit']['record']
        path.write_text('changed')
        with self.assertRaises(readiness.InvalidRecord):
            self.check()

    def test_unsafe_and_absent_evidence_is_rejected(self):
        for value in ('../secret', 'docs/delivery/readiness/../../../secret',
                      'docs/delivery/readiness/missing.md'):
            with self.subTest(value=value):
                self.record['evidence']['initial-audit']['record'] = value
                with self.assertRaises((readiness.InvalidRecord, OSError)):
                    self.check()

    def test_invalid_dates_and_late_access_checkpoints_rejected(self):
        for value in ('2026-02-30', '2026-9-25', '2026-10-07'):
            with self.subTest(value=value):
                self.record['requirements'][0]['needed_by'] = value
                with self.assertRaises(ValueError):
                    self.check()

    def test_future_assessment_and_future_evidence_rejected(self):
        self.as_of = date(2026, 9, 23)
        with self.assertRaises(readiness.InvalidRecord):
            self.check()
        self.as_of = date(2026, 9, 24)
        self.record['evidence']['initial-audit']['recorded_on'] = '2026-09-25'
        with self.assertRaises(readiness.InvalidRecord):
            self.check()

    def test_overdue_is_distinct_from_missing(self):
        self.as_of = date(2026, 10, 7)
        result = self.check()
        self.assertEqual(len(result['overdue']), 9)
        self.assertEqual(result['counts']['missing'], 0)

    def confirm_all(self):
        self.add_evidence('availability', 'confirmation', valid_until='2026-12-23')
        self.add_evidence('custodian', 'role_assignment')
        for item in self.record['requirements']:
            item.update(status='confirmed', confirmation_evidence=['availability'],
                        assignment_status='confirmed', assignment_evidence=['custodian'])

    def run_cli(self, as_of, optimized=False):
        path = self.root / readiness.REGISTER
        path.write_text(json.dumps(self.record))
        code = ("import importlib.util, pathlib, sys; "
                "spec=importlib.util.spec_from_file_location('readiness',sys.argv[1]); "
                "module=importlib.util.module_from_spec(spec); spec.loader.exec_module(module); "
                "module.ROOT=pathlib.Path(sys.argv[2]); "
                "sys.argv=['check_release_readiness.py','--require-ready','--as-of',sys.argv[3]]; "
                "raise SystemExit(module.main())")
        return subprocess.run([sys.executable, *(['-O'] if optimized else []), '-c', code,
                               str(ROOT / 'scripts/check_release_readiness.py'),
                               str(self.root), as_of], capture_output=True, text=True)

    def test_confirmation_expires_at_checkpoint_despite_later_declared_validity(self):
        self.confirm_all()
        self.as_of = date(2026, 12, 15)
        self.assertTrue(self.check()['ready'])
        self.as_of = date(2026, 12, 16)
        result = self.check()
        self.assertFalse(result['ready'])
        self.assertEqual(11, len(result['stale_confirmations']))
        self.assertEqual(11, len(result['overdue']))
        self.assertEqual(11, result['counts']['confirmed'])

    def test_explicit_validity_can_expire_before_checkpoint(self):
        self.confirm_all()
        self.record['evidence']['availability']['valid_until'] = '2026-10-01'
        self.as_of = date(2026, 10, 2)
        self.assertEqual(11, len(self.check()['stale_confirmations']))

    def test_fresh_reconfirmation_renews_only_through_programme_end(self):
        self.confirm_all()
        self.record['assessed_on'] = '2026-12-16'
        self.add_evidence('renewal', 'confirmation', recorded_on='2026-12-16', valid_until='2026-12-23')
        for item in self.record['requirements']:
            item['assessed_on'] = '2026-12-16'
            item['confirmation_evidence'] = ['renewal']
        self.as_of = date(2026, 12, 16)
        self.assertTrue(self.check()['ready'])
        self.as_of = date(2026, 12, 24)
        result = self.check()
        self.assertTrue(result['programme_expired'])
        self.assertFalse(result['ready'])

    def test_invalid_confirmation_validity_is_rejected(self):
        self.confirm_all()
        for value in (None, '2026-09-01', '2027-01-01'):
            with self.subTest(value=value):
                self.record['evidence']['availability']['valid_until'] = value
                with self.assertRaises(ValueError):
                    self.check()

    def test_stale_cli_fails_under_python_optimization(self):
        self.confirm_all()
        for optimized in (False, True):
            run = self.run_cli('2027-01-01', optimized)
            self.assertEqual(1, run.returncode, run.stderr)
            result = json.loads(run.stdout)
            self.assertTrue(result['record_valid'])
            self.assertFalse(result['ready'])
            self.assertEqual(11, len(result['overdue']))

    def test_cli_oserror_omits_private_paths(self):
        (self.root / self.record['evidence']['initial-audit']['record']).unlink()
        run = self.run_cli('2026-09-24')
        self.assertEqual(1, run.returncode)
        self.assertNotIn(str(self.root), run.stdout + run.stderr)
        self.assertEqual('Unreadable or malformed readiness record', json.loads(run.stdout)['error'])

    def test_root_parent_and_file_reparse_flags_are_rejected(self):
        original = Path.lstat
        paths = (self.root, self.root / 'docs',
                 self.root / self.record['evidence']['initial-audit']['record'])
        for target in paths:
            def metadata(path):
                actual = original(path)
                if path == target:
                    return SimpleNamespace(st_mode=actual.st_mode,
                                           st_file_attributes=0x400)
                return actual
            with self.subTest(target=target.name), patch.object(Path, 'lstat', metadata):
                with self.assertRaisesRegex(readiness.InvalidRecord, 'reparse'):
                    self.check()

    @unittest.skipUnless(os.name == 'nt', 'Windows junction-specific check')
    def test_native_windows_junction_root_is_rejected(self):
        junction = self.root / 'junction'
        run = subprocess.run(['cmd', '/c', 'mklink', '/J', str(junction), str(self.root)],
                             capture_output=True, text=True)
        if run.returncode:
            self.skipTest('Junction creation unavailable')
        try:
            with self.assertRaisesRegex(readiness.InvalidRecord, 'reparse'):
                readiness.validate(self.record, root=junction, as_of=self.as_of)
        finally:
            junction.rmdir()

    def test_duplicate_json_keys_rejected(self):
        with self.assertRaises(readiness.InvalidRecord):
            json.loads('{"status":"unknown","status":"confirmed"}',
                       object_pairs_hook=readiness.reject_duplicate_keys)


if __name__ == '__main__':
    unittest.main()
