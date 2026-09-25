"""Offline host-campaign acceptance tests. Never load or execute the candidate interpreter."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
import python_graph_host_receipts as receipt
spec = importlib.util.spec_from_file_location('host_campaign_runner', SCRIPTS/'run_graph_host_native.py')
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)
CAMPAIGN = 'ed4882d9-2aa0-4c52-9efc-e389a3289996'


def specimen():
    q = {'schema_version': 5, 'id': CAMPAIGN, 'request_key': '836db95e-0c8b-4ad0-b975-c46cdbca8bd7', 'state': 'queued', 'attempt': 1,
         'input': {'operation': 'shortest_connection_path', 'source_id': 'a', 'target_id': 'c',
                   'requested_revision': 2, 'queued_revision': 3}}
    f = dict(q, state='completed', failure=None, lease='bf28cb14-4c3c-4cac-940f-dbbf8b73a29d', result_ids=['a'*64])
    request = {'workspace_revision': 4, 'source_id': 'a', 'target_id': 'c', 'nodes': list('abcdef'),
               'edges': [['a', 'b'], ['a', 'd'], ['b', 'c'], ['c', 'e'], ['d', 'e']]}
    graph = {'workspace_revision': 4, 'outcome': {'state': 'path', 'nodes': ['a', 'b', 'c']}}
    record = {'schema_version': 1, 'id': 'a'*64, 'job_id': q['id'], 'request_key': q['request_key'], 'host_attempt_lease': f['lease'], 'attempt': 1,
              'requested_revision': 2, 'queued_revision': 3, 'captured_revision': 4, 'published_revision': 5,
              'request_json': json.dumps(request), 'result_json': json.dumps(graph),
              'frozen': {'assertion_reviews': {'accepted': 6, 'pending': 1, 'rejected': 1, 'deferred': 1}},
              'outcome': {'state': 'path', 'nodes': ['a', 'b', 'c'], 'hops': [
                  {'source_id': 'a', 'target_id': 'b', 'assertion_ids': ['r1', 'r1-parallel']},
                  {'source_id': 'b', 'target_id': 'c', 'assertion_ids': ['r2']}]}}
    record['request_sha256'] = receipt.identity(record['request_json'].encode())['sha256']
    record['result_sha256'] = receipt.identity(record['result_json'].encode())['sha256']
    ref = {k: record[k] for k in ('id', 'request_sha256', 'result_sha256', 'captured_revision', 'published_revision')}
    finished = {'schema_version': 1, 'job': f, 'availability': 'ready', 'workspace_revision': 5, 'results': [ref], 'execution': None,
                'controls': {'can_cancel': False, 'can_retry_publication': False}}
    result = {'queued': {'schema_version': 1, 'job': q, 'availability': 'ready', 'workspace_revision': 3},
              'page': {'schema_version': 1, 'availability': 'ready', 'workspace_revision': 5, 'rows': [{'sequence': 3, 'job': f}],
                       'total_count': 1, 'next_cursor': None}, 'finished': finished, 'replay': copy.deepcopy(finished),
              'inspection': {'record': record, 'compared_revision': 5, 'freshness': 'current_at_publication', 'original_integrity': 'verified'},
              'delta': {}, 'replay_unchanged': True, 'joined_shutdown': True, 'reopen_unchanged': True, 'scratch_empty': True}
    before = {'tables': {'schema': [], 'version': [[5]], 'meta': [[1, 2]], 'records': [[1, 'entity', 'a', '{"id":"a"}']],
                        'history': [], 'events': [[1, 1, 'import', 'time'], [2, 2, 'synthetic.graph', 'time']],
                        'derivative_objects': [], 'sqlite_sequence': [['events', 2], ['records', 1]]}, 'originals': {'a'*64: [1, 'b'*64]}}
    after = copy.deepcopy(before)
    a = after['tables']
    a['records'] += [[2, 'processing_request', q['request_key'], json.dumps(q['id'])],
                     [3, 'processing_job', q['id'], json.dumps(f)], [5, 'graph_analysis', record['id'], json.dumps(record)]]
    a['history'] = [[1, 'processing_job', q['id'], json.dumps(q), 4],
                    [2, 'processing_job', q['id'], json.dumps(dict(f, state='running')), 5]]
    a['events'] += [[3, 3, 'processing.graph_queue', 'time'], [4, 4, 'processing.graph_claim', 'time'], [5, 5, 'processing.graph_finish', 'time']]
    a['meta'] = [[1, 5]]
    a['sqlite_sequence'] = [['events', 5], ['history', 2], ['records', 6]]
    result['delta'] = {'before': receipt.prior.snapshot_identity(before), 'after': receipt.prior.snapshot_identity(after),
                       'all_tables_checked': True, 'new_events': a['events'][2:]}
    return result, request, graph, record, before, after


def envelope():
    o = {'execution_calls': 1, 'launch_count': 1, 'pid': 123, 'launch_ms': 100, 'live_observed_ms': None,
         'cancellation_seen_ms': None, 'stop_ms': 200, 'termination': 'confirmed', 'cleanup': 'confirmed',
         'profile_sha256': None, 'assigned_files': None, 'pre_inventory_verified': True, 'post_inventory_verified': True,
         'assignment_id': CAMPAIGN, 'capture_nonce': CAMPAIGN, 'request_identity': receipt.identity(b'{}'),
         'wrapper_identity': receipt.identity(b'{}'), 'result_identity': receipt.identity(b'{}')}
    return {'schema_version': 1, 'campaign_id': CAMPAIGN, 'passed': True, 'failure': None, 'observation': o,
            'result': {}, 'complete_release': False}


class HostReceiptTests(unittest.TestCase):
    def test_uncertainty_gates_every_post_read(self):
        for field, value in [('termination', 'unverified'), ('termination', 'unconfirmed'), ('cleanup', 'failed')]:
            e = envelope(); e['observation'][field] = value
            with patch.object(receipt.common, 'read_json', side_effect=AssertionError('read')), patch.object(receipt.prior, 'raw', side_effect=AssertionError('raw')):
                with self.assertRaises(receipt.common.ProbeFailure): receipt.accept(e, CAMPAIGN, Path('/unused'))

    def test_exact_envelope_and_raw_bytes(self):
        self.assertTrue(receipt.summary(envelope(), CAMPAIGN))
        for field, value in [('campaign_id', 'other'), ('schema_version', True), ('complete_release', True)]:
            e = envelope(); e[field] = value
            with self.assertRaises(receipt.common.ProbeFailure): receipt.summary(e, CAMPAIGN)
        with tempfile.TemporaryDirectory() as root:
            p = Path(root)/'result'; p.write_bytes(b'{ "x": 1}')
            with self.assertRaises(receipt.common.ProbeFailure): receipt.prior.raw(p, 100, receipt.identity(b'{"x":1}'))

    def test_public_reference_replay_and_revision_bindings(self):
        result, request, graph, record, _, _ = specimen()
        receipt.public_results(result, request, graph, record)
        for field, replacement in [('availability', 'synthetic_fixture'), ('workspace_revision', 4)]:
            changed = copy.deepcopy(result); changed['finished'][field] = replacement
            with self.assertRaises(receipt.common.ProbeFailure): receipt.public_results(changed, request, graph, record)
        for field, replacement in [('id', 'b'*64), ('request_sha256', 'b'*64), ('result_sha256', 'b'*64), ('captured_revision', 3)]:
            changed = copy.deepcopy(result); changed['finished']['results'][0][field] = replacement
            with self.assertRaises(receipt.common.ProbeFailure): receipt.public_results(changed, request, graph, record)
        changed = copy.deepcopy(result); changed['replay']['job']['request_key'] = CAMPAIGN
        with self.assertRaises(receipt.common.ProbeFailure): receipt.public_results(changed, request, graph, record)

    def test_complete_delta_rejects_every_table_and_detached_body(self):
        result, _, _, _, before, after = specimen()
        receipt.canonical_delta(before, after, result)
        for table in receipt.prior.TABLES:
            changed = copy.deepcopy(after); changed['tables'][table].append(['unexpected'])
            with self.assertRaises((receipt.common.ProbeFailure, ValueError, IndexError)): receipt.canonical_delta(before, changed, result)
        for index in (1, 2, 3):
            changed = copy.deepcopy(after); changed['tables']['records'][index][3] = '"substitution"'
            with self.assertRaises(receipt.common.ProbeFailure): receipt.canonical_delta(before, changed, result)
        changed = copy.deepcopy(after); changed['originals'] = {}
        with self.assertRaises(receipt.common.ProbeFailure): receipt.canonical_delta(before, changed, result)
        changed = copy.deepcopy(result); changed['delta']['before']['sha256'] = 'f'*64
        with self.assertRaises(receipt.common.ProbeFailure): receipt.canonical_delta(before, after, changed)


@unittest.skipUnless(os.name == 'posix', 'Native runner private file modes require POSIX')
class HostRunnerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        base = Path(self.temporary.name).resolve()
        self.prefix = base/'original'; self.prefix.mkdir(mode=0o700)
        self.artifacts = base/CAMPAIGN
        for path in [self.artifacts, self.artifacts/'resources', self.artifacts/'resources/engines', self.artifacts/'resources/engines/python']:
            path.mkdir(mode=0o700)

    def test_admission_accepts_only_prestaged_subtree_and_never_adopts_old_results(self):
        self.assertEqual(runner.admit(self.prefix, self.artifacts), self.artifacts/'resources/engines/python')
        for name in ('receipt.json', 'observation.json', 'workspace', 'native-test.log'):
            path = self.artifacts/name; path.write_text('retained')
            with self.assertRaises(receipt.common.ProbeFailure): runner.admit(self.prefix, self.artifacts)
            self.assertEqual(path.read_text(), 'retained'); path.unlink()
        misplaced = self.artifacts/'resources/extra'; misplaced.mkdir()
        with self.assertRaises(receipt.common.ProbeFailure): runner.admit(self.prefix, self.artifacts)

    def test_admission_refuses_linked_or_public_ancestors(self):
        resources = self.artifacts/'resources'
        resources.chmod(0o755)
        with self.assertRaises(receipt.common.ProbeFailure): runner.admit(self.prefix, self.artifacts)
        resources.chmod(0o700)
        python = resources/'engines/python'; python.rmdir(); python.symlink_to(self.prefix, target_is_directory=True)
        with self.assertRaises(receipt.common.ProbeFailure): runner.admit(self.prefix, self.artifacts)

    def test_source_failure_is_retained_before_any_build_or_inventory(self):
        with patch.object(runner, 'source_identity', side_effect=OSError('fixture')), patch.object(runner, 'build', side_effect=AssertionError('build')), patch.object(runner.common.installed, 'verify', side_effect=AssertionError('inventory')):
            report = runner.run(self.prefix, self.artifacts)
        self.assertFalse(report['passed']); self.assertFalse(report['candidate_started'])
        saved = json.loads((self.artifacts/'observation.json').read_text())
        self.assertEqual(saved, report)

    def test_outer_timeout_never_reads_receipts_database_originals_or_runtime_after(self):
        binary = self.prefix.parent/'fake-test'; binary.write_bytes(b'never executed')
        with patch.object(runner, 'source_identity', return_value={'commit': 'source-only'}), patch.object(runner.platform, 'system', return_value='Darwin'), patch.object(runner.platform, 'machine', return_value='arm64'), patch.object(runner.common.installed, 'verify', return_value={'verified': True}) as inventory, patch.object(runner, 'build', return_value=binary), patch.object(runner.common, 'run_logged', side_effect=subprocess.TimeoutExpired('fixed', 300)) as launch, patch.object(runner.common, 'read_json', side_effect=AssertionError('post read')), patch.object(runner.receipts, 'accept', side_effect=AssertionError('accept')):
            report = runner.run(self.prefix, self.artifacts)
            self.assertEqual(inventory.call_count, 2); self.assertEqual(launch.call_count, 1)
        self.assertEqual(report['termination'], 'unverified'); self.assertFalse(report['passed'])
        self.assertTrue((self.artifacts/'native-test-binary').is_file())

    def test_unconfirmed_receipt_prevents_acceptance_and_all_post_inventory(self):
        binary = self.prefix.parent/'fake-test'; binary.write_bytes(b'never executed')
        value = envelope(); value['observation']['termination'] = 'unverified'
        with patch.object(runner, 'source_identity', return_value={'commit': 'source-only'}), patch.object(runner.platform, 'system', return_value='Darwin'), patch.object(runner.platform, 'machine', return_value='arm64'), patch.object(runner.common.installed, 'verify', return_value={'verified': True}) as inventory, patch.object(runner, 'build', return_value=binary), patch.object(runner.common, 'run_logged', return_value=subprocess.CompletedProcess([], 0)) as launch, patch.object(runner.common, 'read_json', return_value=value) as read, patch.object(runner.receipts, 'accept', side_effect=AssertionError('accept')):
            report = runner.run(self.prefix, self.artifacts)
            self.assertEqual(inventory.call_count, 2); self.assertEqual(launch.call_count, 1); self.assertEqual(read.call_count, 1)
        self.assertFalse(report['passed'])
        self.assertEqual(report['failure'], 'host-termination-or-cleanup-unconfirmed')
        self.assertEqual(report['receipt']['observation']['termination'], 'unverified')


if __name__ == '__main__':
    unittest.main()
