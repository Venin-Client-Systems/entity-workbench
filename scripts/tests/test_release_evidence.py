"""Synthetic contracts only: passing these fixtures does not approve any release."""
import copy
from datetime import datetime, timedelta, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
import release_evidence as evidence


class ReleaseEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.receipts = self.root / 'receipts'
        self.artifacts = self.root / 'artifacts'
        self.receipts.mkdir()
        self.artifacts.mkdir()
        self.policy = json.loads(evidence.POLICY.read_text())
        for target, support in self.policy['support_matrix'].items():
            support['minimum_version'] = '11.0.22000' if target.startswith('windows') else '15.0.0'
            support['tested_versions'] = [support['minimum_version']]
            support['decision_reference'] = 'synthetic-test-decision'
        self.policy_path = self.root / 'policy.json'
        self.policy_path.write_text(json.dumps(self.policy))
        self.ledger_path = self.root / 'ledger.json'
        self.now = datetime.now(timezone.utc).replace(microsecond=0)
        self.observed = (self.now - timedelta(hours=1)).strftime('%Y-%m-%dT%H:%M:%SZ')
        self.policy_digest = hashlib.sha256(self.policy_path.read_bytes()).hexdigest()
        self.fixture_digest = hashlib.sha256(evidence.CATALOGUE.read_bytes()).hexdigest()
        self.ledger = {'schema_version': 2, 'complete_release': True,
                       'candidate': {'id': 'synthetic-candidate', 'source_revision': 'a' * 40,
                                     'policy_sha256': self.policy_digest, 'fixture_catalogue_sha256': self.fixture_digest,
                                     'artifacts': {}},
                       'gates': {g: True for g in self.policy['gates']}, 'evidence': []}
        for target in evidence.TARGETS:
            self.ledger['candidate']['artifacts'][target] = self.file(self.artifacts, target + '.bin', ('SYNTHETIC ' + target).encode())
        attachment = self.file(self.receipts, 'result.json', b'{"synthetic":true,"result":"fixture-only"}\n')
        for gate, rule in self.policy['gates'].items():
            for target in rule['targets']:
                family, arch = target.split('-')
                record = {'schema_version': 1, 'id': gate.replace('_', '-') + '-' + target,
                          'gate': gate, 'target': target, 'source_revision': 'a' * 40,
                          'policy_sha256': self.policy_digest, 'fixture_catalogue_sha256': self.fixture_digest,
                          'artifact_sha256': self.ledger['candidate']['artifacts'][target]['sha256'],
                          'observed_at': self.observed,
                          'environment': {'os_family': family, 'architecture': arch,
                                          'os_version': self.policy['support_matrix'][target]['minimum_version'],
                                          'memory_gib': 16, 'context': 'clean_install'},
                          'kind': rule['evidence_kind'], 'procedure': {'id': rule['procedure_id'], 'version': rule['procedure_version'], 'invocation': 'Synthetic fixture procedure; never execute this field'},
                          'result': 'passed', 'attachments': [attachment],
                          'review': {'status': 'accepted', 'role': rule['reviewer_role'], 'reference': 'synthetic-review-' + str(len(self.ledger['evidence'])), 'receipt': None}}
                self.review(record)
                self.ledger['evidence'].append(record)

    def file(self, root, name, raw):
        (root / name).write_bytes(raw)
        return {'path': name, 'size': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}

    def review(self, record):
        review = record['review']
        receipt = {'schema_version': 1, 'reference': review['reference'], 'evidence_id': record['id'],
                   'observation_sha256': evidence.observation_digest(record), 'decision': review['status'],
                   'reviewer_role': review['role'], 'reviewed_at': self.now.strftime('%Y-%m-%dT%H:%M:%SZ'),
                   'reason': 'Synthetic validation fixture, not real acceptance.'}
        review['receipt'] = self.file(self.receipts, record['id'] + '-review.json', json.dumps(receipt).encode())

    def run_check(self, policy_only=False, artifacts=True):
        self.ledger_path.write_text(json.dumps(self.ledger))
        with patch.object(evidence, 'POLICY', self.policy_path):
            return evidence.evaluate(self.ledger_path, self.receipts, self.artifacts if artifacts else None, policy_only)

    def fails(self, contains=None, policy_only=False):
        report = self.run_check(policy_only)
        self.assertFalse(report['complete_release'], report)
        self.assertTrue(report['errors'], report)
        if contains:
            self.assertIn(contains, ' '.join(report['errors']))
        return report

    def test_current_repository_policy_has_no_passed_gates(self):
        report = evidence.evaluate(evidence.ROOT / 'docs/release-gates.json', evidence.ROOT / 'docs/release-evidence', policy_only=True)
        self.assertTrue(report['policy_valid'], report)
        self.assertFalse(report['complete_release'])
        self.assertEqual(12, len(report['unpassed']))
        self.assertEqual(10, report['fixture_count'])

    def test_complete_synthetic_ledger_requires_actual_artifact_bytes(self):
        result = self.run_check()
        self.assertTrue(result['complete_release'], result)
        self.assertTrue(result['candidate_bytes_verified'])
        self.assertEqual(12, len(result['passed_gates']))
        self.assertFalse(self.run_check(artifacts=False)['complete_release'])

    def test_policy_check_never_reports_a_complete_release(self):
        result = self.run_check(policy_only=True, artifacts=False)
        self.assertTrue(result['policy_valid'], result)
        self.assertFalse(result['complete_release'])
        self.assertFalse(result['candidate_bytes_verified'])

    def test_missing_gate_platform_evidence_blocks_claim(self):
        self.ledger['evidence'].pop(0)
        self.fails('Gate claim lacks')

    def test_unsupported_ledger_and_evidence_schemas_and_duplicate_ids(self):
        original = copy.deepcopy(self.ledger)
        for schema in (1, 999, True):
            self.ledger = copy.deepcopy(original)
            self.ledger['schema_version'] = schema
            self.fails('Unsupported ledger')
        self.ledger = copy.deepcopy(original)
        self.ledger['evidence'][0]['schema_version'] = 999
        self.fails('Unsupported evidence')
        self.ledger = original
        self.ledger['evidence'].append(copy.deepcopy(self.ledger['evidence'][0]))
        self.fails('Duplicate evidence id')

    def test_candidate_policy_and_fixture_bindings_cannot_be_stale(self):
        for key in ('policy_sha256', 'fixture_catalogue_sha256'):
            old = self.ledger['candidate'][key]
            self.ledger['candidate'][key] = '0' * 64
            self.fails('binding is stale')
            self.ledger['candidate'][key] = old

    def test_stale_observations_are_retained_but_cannot_pass(self):
        for key, value in [('source_revision', 'b' * 40), ('artifact_sha256', 'b' * 64), ('policy_sha256', 'b' * 64), ('fixture_catalogue_sha256', 'b' * 64)]:
            record = self.ledger['evidence'][0]
            old = record[key]
            record[key] = value
            self.review(record)
            report = self.fails('Gate claim lacks')
            self.assertIn(record['id'], report['noncurrent_records'])
            record[key] = old
            self.review(record)

    def test_artifact_and_attachment_tampering(self):
        target = next(iter(self.ledger['candidate']['artifacts'].values()))
        (self.artifacts / target['path']).write_bytes(b'X' * target['size'])
        self.fails('checksum mismatch')
        self.ledger['candidate']['artifacts'][evidence.TARGETS[0]] = self.file(self.artifacts, target['path'], b'SYNTHETIC ' + evidence.TARGETS[0].encode())
        (self.receipts / 'result.json').write_bytes(b'corrupt')
        self.fails('size mismatch')

    def test_review_must_exist_and_bind_to_the_observation(self):
        record = self.ledger['evidence'][0]
        (self.receipts / record['review']['receipt']['path']).unlink()
        self.fails()
        self.review(record)
        record['procedure']['invocation'] = 'Changed after review'
        self.fails('not bound')

    def test_wrong_review_identity_decision_and_role(self):
        record = self.ledger['evidence'][0]
        original = copy.deepcopy(record)
        for field, value in [('evidence_id', 'unrelated'), ('decision', 'rejected'), ('reviewer_role', 'maintainer')]:
            self.ledger['evidence'][0] = copy.deepcopy(original)
            record = self.ledger['evidence'][0]
            self.review(record)
            path = self.receipts / record['review']['receipt']['path']
            receipt = json.loads(path.read_text())
            receipt[field] = value
            record['review']['receipt'] = self.file(self.receipts, path.name, json.dumps(receipt).encode())
            self.fails('Review')

    def test_pending_or_rejected_review_never_passes(self):
        record = self.ledger['evidence'][0]
        for status in ('pending', 'rejected'):
            record['review'] = {'status': status, 'role': None, 'reference': None, 'receipt': None}
            if status == 'rejected':
                record['review'].update(role='release-verifier', reference='reject-synthetic')
                self.review(record)
            self.fails('Gate claim lacks')

    def test_later_failure_cannot_be_hidden_by_rejecting_it(self):
        record = copy.deepcopy(self.ledger['evidence'][0])
        record['id'] += '-later'
        record['observed_at'] = (self.now - timedelta(minutes=30)).strftime('%Y-%m-%dT%H:%M:%SZ')
        record['result'] = 'failed'
        record['review']['status'] = 'rejected'
        self.review(record)
        self.ledger['evidence'].append(record)
        self.fails('hides a later outcome')
        self.ledger['gates'][record['gate']] = False
        self.ledger['complete_release'] = False
        report = self.run_check()
        self.assertTrue(report['policy_valid'], report)
        self.assertEqual(len(self.ledger['evidence']), report['retained_records'])

    def test_wrong_procedure_source_ci_and_old_observations_do_not_qualify(self):
        original = copy.deepcopy(self.ledger['evidence'][0])
        for change in ('procedure', 'source_ci', 'old'):
            record = copy.deepcopy(original)
            if change == 'procedure':
                record['procedure']['version'] = 'obsolete'
            elif change == 'source_ci':
                record['kind'] = 'source_check'
                record['environment']['context'] = 'hosted_ci'
            else:
                record['observed_at'] = (self.now - timedelta(days=31)).strftime('%Y-%m-%dT%H:%M:%SZ')
            self.review(record)
            self.ledger['evidence'][0] = record
            self.fails('Gate claim lacks')

    def test_invalid_or_unsupported_os_and_wrong_architecture(self):
        original = copy.deepcopy(self.ledger['evidence'][0])
        for version in ('11.garbage', 'banana', '11.00.22000', '10.0.20348', '11.0.10000'):
            record = copy.deepcopy(original)
            record['environment']['os_version'] = version
            self.review(record)
            self.ledger['evidence'][0] = record
            self.fails()
        record = copy.deepcopy(original)
        record['environment']['architecture'] = 'aarch64'
        self.review(record)
        self.ledger['evidence'][0] = record
        self.fails('mismatch')

    def test_pass_on_another_os_cannot_hide_a_retained_failure(self):
        for failed_version, passed_version in [('11.0.22000', '11.0.26100'), ('11.0.26100', '11.0.22000')]:
            with self.subTest(failed_version=failed_version):
                original = copy.deepcopy(self.ledger)
                record = self.ledger['evidence'][0]
                record['environment']['os_version'] = passed_version
                self.review(record)
                failed = copy.deepcopy(record)
                failed['id'] += '-other-os'
                failed['environment']['os_version'] = failed_version
                failed['result'] = 'failed'
                failed['observed_at'] = (self.now - timedelta(hours=2)).strftime('%Y-%m-%dT%H:%M:%SZ')
                self.review(failed)
                self.ledger['evidence'].append(failed)
                self.fails('Gate claim lacks')
                self.ledger = original

    def test_policy_change_does_not_erase_observed_os_test_obligations(self):
        record = copy.deepcopy(self.ledger['evidence'][0])
        record['id'] += '-old-policy'
        record['environment']['os_version'] = '11.0.26100'
        record['policy_sha256'] = 'b' * 64
        record['result'] = 'failed'
        self.review(record)
        self.ledger['evidence'].append(record)
        self.fails('Gate claim lacks')
        fresh = copy.deepcopy(record)
        fresh['id'] += '-rerun'
        fresh['policy_sha256'] = self.policy_digest
        fresh['result'] = 'passed'
        fresh['observed_at'] = (self.now - timedelta(minutes=20)).strftime('%Y-%m-%dT%H:%M:%SZ')
        self.review(fresh)
        self.ledger['evidence'].append(fresh)
        self.assertTrue(self.run_check()['complete_release'])

    def test_later_failure_using_old_policy_cannot_be_hidden(self):
        failed = copy.deepcopy(self.ledger['evidence'][0])
        failed['id'] += '-later-old-policy'
        failed['policy_sha256'] = 'b' * 64
        failed['result'] = 'failed'
        failed['observed_at'] = (self.now - timedelta(minutes=15)).strftime('%Y-%m-%dT%H:%M:%SZ')
        self.review(failed)
        self.ledger['evidence'].append(failed)
        self.fails('Gate claim lacks')

    def test_minimum_os_must_be_observed(self):
        record = self.ledger['evidence'][0]
        record['environment']['os_version'] = '11.0.26100'
        self.review(record)
        self.fails('Gate claim lacks')

    def test_windows_specific_gate_cannot_use_mac_evidence(self):
        record = next(r for r in self.ledger['evidence'] if r['gate'] == 'windows_appcontainer')
        record['target'] = 'macos-aarch64'
        self.fails('Wrong platform')

    def test_security_gate_requires_security_reviewer(self):
        record = next(r for r in self.ledger['evidence'] if r['gate'] == 'windows_appcontainer')
        record['review']['role'] = 'maintainer'
        self.review(record)
        self.fails('Gate claim lacks')

    def test_future_observation_and_review_before_observation(self):
        record = self.ledger['evidence'][0]
        record['observed_at'] = (self.now + timedelta(days=1)).strftime('%Y-%m-%dT%H:%M:%SZ')
        self.review(record)
        self.fails('future')
        record['observed_at'] = self.observed
        self.review(record)
        ref = record['review']['receipt']
        receipt = json.loads((self.receipts / ref['path']).read_text())
        receipt['reviewed_at'] = (self.now - timedelta(days=1)).strftime('%Y-%m-%dT%H:%M:%SZ')
        record['review']['receipt'] = self.file(self.receipts, ref['path'], json.dumps(receipt).encode())
        self.fails('predates')

    def test_artifact_paths_and_receipt_links_are_rejected(self):
        ref = next(iter(self.ledger['candidate']['artifacts'].values()))
        ref['path'] = '../outside'
        self.fails('Unsafe path')
        ref['path'] = evidence.TARGETS[0] + '.bin'
        outside = self.root / 'outside'
        (self.receipts / 'result.json').rename(outside)
        try:
            (self.receipts / 'result.json').symlink_to(outside)
        except OSError as exc:
            self.skipTest(f'Symlink creation unavailable: {exc.errno}')
        self.fails('links')

    def test_policy_cannot_remove_or_vacuously_pass_gates(self):
        original = copy.deepcopy(self.policy)
        mutations = [lambda p: p.update(schema_version=999),
                     lambda p: p['gates'].pop('integrated_workflows'),
                     lambda p: p['gates']['integrated_workflows'].update(targets=[]),
                     lambda p: p['gates']['integrated_workflows'].update(targets=['windows-x86_64'] * 3),
                     lambda p: p['gates']['integrated_workflows'].update(issues=[]),
                     lambda p: p['gates']['integrated_workflows'].update(evidence_kind='source_check'),
                     lambda p: p['scenarios'].pop('identity_ambiguity')]
        for mutate in mutations:
            policy = copy.deepcopy(original)
            mutate(policy)
            self.policy_path.write_text(json.dumps(policy))
            self.fails()

    def test_unknown_support_boundary_never_qualifies(self):
        self.policy['support_matrix']['windows-x86_64'] = {'minimum_version': None, 'tested_versions': [], 'decision_reference': None}
        self.policy_path.write_text(json.dumps(self.policy))
        digest = hashlib.sha256(self.policy_path.read_bytes()).hexdigest()
        self.ledger['candidate']['policy_sha256'] = digest
        for record in self.ledger['evidence']:
            record['policy_sha256'] = digest
            self.review(record)
        self.fails('Gate claim lacks')

    def test_malformed_json_and_naked_boolean_claims_fail_under_optimization(self):
        self.ledger_path.write_text('{"schema_version":2,"schema_version":2}')
        with patch.object(evidence, 'POLICY', self.policy_path):
            self.assertFalse(evidence.evaluate(self.ledger_path, self.receipts, policy_only=True)['policy_valid'])
        ledger = json.loads((evidence.ROOT / 'docs/release-gates.json').read_text())
        ledger['complete_release'] = True
        ledger['gates'] = {g: True for g in ledger['gates']}
        self.ledger_path.write_text(json.dumps(ledger))
        run = subprocess.run([sys.executable, '-O', str(SCRIPTS / 'release_gate.py'), '--check-policy', '--ledger', str(self.ledger_path)], capture_output=True, text=True)
        self.assertEqual(1, run.returncode)
        self.assertFalse(json.loads(run.stdout)['complete_release'])

    def test_fixture_integrity_and_scenario_coverage(self):
        catalogue = json.loads(evidence.CATALOGUE.read_text())
        catalogue['scenarios'].pop('identity_ambiguity')
        path = self.root / 'catalogue.json'
        path.write_text(json.dumps(catalogue))
        with self.assertRaises(evidence.InvalidEvidence):
            evidence.validate_catalogue(path, self.policy_path)
        catalogue = json.loads(evidence.CATALOGUE.read_text())
        catalogue['fixtures'][0]['file']['sha256'] = '0' * 64
        # Retain real referenced fixture files while substituting only catalogue content.
        original_read = evidence.read_json
        def read(selected):
            return (catalogue, '0' * 64) if selected == evidence.CATALOGUE else original_read(selected)
        with patch.object(evidence, 'read_json', side_effect=read), self.assertRaises(evidence.InvalidEvidence):
            evidence.validate_catalogue(evidence.CATALOGUE, self.policy_path)


if __name__ == '__main__':
    unittest.main()
