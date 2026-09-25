"""Safe harness contracts only: no candidate interpreter or third-party imports."""
from contextlib import ExitStack
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import test_python_compatibility as runner


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, runner.ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)  # These reviewed modules import stdlib only; main/execute are never invoked.
    return module


sys.modules['runtime_support'] = load('runtime_support', 'workers/python/runtime_support.py')
bootstrap = load('probe_bootstrap_contract', 'workers/python/probe/bootstrap.py')
compatibility = load('probe_compatibility_contract', 'workers/python/probe/compatibility.py')


def identity(data):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def successful_receipt(campaign):
    job = str(uuid.uuid4())
    assigned = {}
    for relative, source in runner.ASSIGNED.items():
        data = (runner.ROOT / source).read_bytes() if source else b'{"synthetic":true,"reference":"000123"}'
        assigned[relative] = identity(data)
    interpreter = dict(identity(b'SYNTHETIC interpreter never executed'), path='install/bin/python3.13')
    native = {'schema_version': 1, 'recipe': 'python-compatibility-v1', 'runtime_manifest_sha256': runner.MANIFEST,
              'campaign_id': campaign, 'job_id': job, 'architecture': 'aarch64', 'runtime_verified': True,
              'candidate_interpreter': interpreter, 'profile_sha256': 'a' * 64, 'assigned_files': assigned,
              'termination_state': 'confirmed', 'passed': True, 'complete_release': False, 'phase': 'complete',
              'diagnostics_within_bound': True, 'last_worker_checkpoint': 'complete',
              'last_import_checkpoint': {'module': 'pyarrow', 'boundary': 'after'}, 'quota_kind': None,
              'import_diagnostics': {'valid': True, 'attempts': [], 'checkpoints': [
                  {'module': module, 'boundary': boundary, 'elapsed_ms': index * 2 + offset,
                   'process_cpu_ms': index * 2 + offset}
                  for index, module in enumerate(runner.IMPORTS) for offset, boundary in enumerate(('before', 'after'))]},
              'preparation_elapsed_ms': 50, 'supervised_elapsed_ms': 100, 'exit_code': 0, 'failure': None,
              'result': {'schema_version': 1, 'recipe': 'python-compatibility-v1', 'job_id': job,
                         'manifest_sha256': runner.MANIFEST, 'python_version': '3.13.15', 'isolated': True,
                         'no_site': True, 'no_bytecode': True, 'verified_paths': True,
                         'checks': runner.read_json(runner.ROOT / 'workers/python/probe/expected.json', 64 * 1024)}}
    return native, interpreter


class ProbeContractTests(unittest.TestCase):
    def test_bootstrap_paths_reject_injected_cwd_user_site_duplicates_and_missing_stdlib(self):
        prefix = Path('/synthetic/prefix') if os.name == 'posix' else Path('C:/synthetic/prefix')
        code = prefix / 'assigned/code'
        stdlib = [str(prefix / name) for name in ('install/lib/python313.zip', 'install/lib/python3.13',
                                                 'install/lib/python3.13/lib-dynload')]
        self.assertEqual(bootstrap.bootstrap_paths(prefix, code, stdlib),
                         stdlib + [str(prefix / 'install/lib/python3.13/site-packages'), str(code)])
        for initial in (stdlib + [''], stdlib + [str(code)], stdlib + [stdlib[0]], stdlib[:-1], []):
            with self.subTest(initial=initial), self.assertRaises(ValueError):
                bootstrap.bootstrap_paths(prefix, code, initial)

    def test_bootstrap_json_rejects_duplicate_oversize_and_clobber(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'fixture.json'
            path.write_text('{"a":1,"a":2}')
            with self.assertRaises(ValueError): bootstrap.read_json(path)
            path.write_bytes(b' ' * (bootstrap.MAX_JSON + 1))
            with self.assertRaises(ValueError): bootstrap.read_json(path)
            path.write_bytes(b'original')
            with self.assertRaises(FileExistsError): bootstrap.write_json(path, {'a': 1})
            self.assertEqual(path.read_bytes(), b'original')
            with self.assertRaises(ValueError): bootstrap.write_json(path.with_name('large'), {'a': 'x' * 1024}, 512)
            self.assertFalse(path.with_name('large').exists())

    def test_metadata_normalizes_names_but_rejects_duplicate_wrong_or_missing_versions(self):
        def dist(name, version): return SimpleNamespace(metadata={'Name': name}, version=version)
        wanted = {'one-package': '1.2'}
        with patch.object(compatibility.importlib.metadata, 'distributions', return_value=[dist('One_Package', '1.2')]):
            self.assertEqual(compatibility.distribution_versions(Path('site'), wanted)[0], wanted)
        for distributions in ([], [dist('one-package', '2')], [dist('One.Package', '1.2'), dist('one-package', '1.2')],
                              [dist('unselected', '1')] * 129):
            with patch.object(compatibility.importlib.metadata, 'distributions', return_value=distributions):
                with self.assertRaises(ValueError): compatibility.distribution_versions(Path('site'), wanted)

    def test_import_diagnostics_retain_exact_order_and_failed_import_boundary(self):
        class StopProbe(Exception): pass
        fixture = {'schema_version': 1, 'versions': {}}
        for fail_at in range(6):
            events = []; count = 0
            def fake_import(name):
                nonlocal count
                current = count; count += 1
                if current == fail_at: raise StopProbe()
                return SimpleNamespace()
            with patch.object(compatibility, 'distribution_versions', return_value=({}, [])), \
                    patch.object(compatibility.importlib, 'import_module', side_effect=fake_import) as imported:
                with self.assertRaises(StopProbe):
                    compatibility.execute(fixture, Path('prefix'), Path('job'), lambda _phase: None,
                                          lambda module, boundary: events.append((module, boundary)))
            self.assertEqual([call.args[0] for call in imported.call_args_list], list(runner.IMPORTS[:fail_at + 1]))
            self.assertEqual(events, [(module, boundary) for module in runner.IMPORTS[:fail_at]
                                      for boundary in ('before', 'after')] + [(runner.IMPORTS[fail_at], 'before')])

    def test_native_receipt_binds_campaign_job_source_interpreter_and_termination(self):
        campaign = str(uuid.uuid4())
        native, interpreter = successful_receipt(campaign)
        runner.accept_native(native, campaign, interpreter)
        mutations = [lambda n: n.update(campaign_id=str(uuid.uuid4())), lambda n: n.update(job_id=str(uuid.uuid4())),
                     lambda n: n.update(runtime_manifest_sha256='b' * 64), lambda n: n.update(termination_state='unverified'),
                     lambda n: n.update(runtime_verified=False), lambda n: n.update(profile_sha256=None),
                     lambda n: n['candidate_interpreter'].update(sha256='b' * 64),
                     lambda n: n['assigned_files']['code/bootstrap.py'].update(sha256='b' * 64),
                     lambda n: n['assigned_files']['code/import_diagnostics.py'].update(sha256='b' * 64),
                     lambda n: n['assigned_files'].pop('input/assignment.json'),
                     lambda n: n['result']['checks'].update(transfer_rejected=1), lambda n: n.update(exit_code=False),
                     lambda n: n.update(extra='unreviewed'), lambda n: n.update(quota_kind='wall-time'),
                     lambda n: n.update(last_import_checkpoint={'module': 'spacy', 'boundary': 'before'}),
                     lambda n: n.update(supervised_elapsed_ms=None)]
        for mutate in mutations:
            changed = copy.deepcopy(native); mutate(changed)
            with self.assertRaises(runner.ProbeFailure): runner.accept_native(changed, campaign, interpreter)

    def test_failure_receipt_keeps_safe_identity_but_never_arbitrary_paths_or_results(self):
        campaign = str(uuid.uuid4()); native, _ = successful_receipt(campaign)
        native.update(passed=False, phase='confined-compatibility', last_worker_checkpoint='imports',
                      failure='compatibility-failed', exit_code=1, result={'untrusted': '/private/sentinel'})
        summary = runner.failure_summary(native, campaign)
        self.assertNotIn('result', summary)
        self.assertEqual(summary['job_id'], native['job_id'])
        for key, value in [('failure', '/private/sentinel'), ('phase', '/private/sentinel'),
                           ('profile_sha256', '/private/sentinel'), ('job_id', '/private/sentinel'),
                           ('last_import_checkpoint', {'module': '/private/sentinel', 'boundary': 'before'}),
                           ('last_import_checkpoint', {'module': 'duckdb', 'boundary': '/private/sentinel'}),
                           ('last_import_checkpoint', {'module': 'duckdb', 'boundary': 'before', 'private': 'x'}),
                           ('quota_kind', '/private/sentinel'), ('supervised_elapsed_ms', True),
                           ('preparation_elapsed_ms', -1), ('supervised_elapsed_ms', 86_400_001),
                           ('candidate_interpreter', {'path': '/private/sentinel', **identity(b'x')})]:
            changed = copy.deepcopy(native); changed[key] = value
            with self.assertRaises(runner.ProbeFailure): runner.failure_summary(changed, campaign)
        changed = copy.deepcopy(native); changed['assigned_files']['/private/sentinel'] = identity(b'x')
        with self.assertRaises(runner.ProbeFailure): runner.failure_summary(changed, campaign)

    def test_initial_failure_receipt_permits_only_unassigned_null_identities(self):
        campaign = str(uuid.uuid4()); native, _ = successful_receipt(campaign)
        native.update(passed=False, phase='runtime-inventory', runtime_verified=False, candidate_interpreter=None,
                      profile_sha256=None, assigned_files={}, termination_state='not-started',
                      last_worker_checkpoint=None, last_import_checkpoint=None, quota_kind=None, preparation_elapsed_ms=None,
                      import_diagnostics=None, supervised_elapsed_ms=None, exit_code=None, failure='compatibility-failed', result=None)
        self.assertEqual(runner.failure_summary(native, campaign)['assigned_files'], {})
        with self.assertRaises(runner.ProbeFailure): runner.accept_native(native, campaign, None)

    def test_source_preflight_rejects_nonignored_untracked_files(self):
        calls = []
        def git(command, **_):
            calls.append(command)
            return '?? injected.py\n'
        with patch.object(runner.subprocess, 'check_output', side_effect=git):
            with self.assertRaisesRegex(runner.ProbeFailure, 'source-is-not-clean'): runner.source_identity()
        self.assertEqual(calls, [['git', 'status', '--porcelain']])


@unittest.skipUnless(os.name == 'posix', 'POSIX-only campaign preparation')
class ProbeRunnerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.prefix = self.base / 'prefix'; self.prefix.mkdir()
        self.artifacts = self.base / 'artifacts'; self.artifacts.mkdir()
        self.binary = self.base / 'compiled-test'; self.binary.write_bytes(b'SYNTHETIC never executed')
        _, self.interpreter = successful_receipt(str(uuid.uuid4()))
        (self.prefix / 'manifest.json').write_text(json.dumps({'files': {'install/bin/python3.13': self.interpreter}}))

    def patches(self):
        stack = ExitStack(); self.addCleanup(stack.close)
        for function, value in [('source_identity', {'commit': 'synthetic', 'tree': 'synthetic'}),
                                ('build_binary', self.binary)]:
            stack.enter_context(patch.object(runner, function, return_value=value))
        stack.enter_context(patch.object(runner.platform, 'system', return_value='Darwin'))
        stack.enter_context(patch.object(runner.platform, 'machine', return_value='arm64'))
        stack.enter_context(patch.object(runner.platform, 'mac_ver', return_value=('26.0.1', (), '')))
        stack.enter_context(patch.object(runner.installed, 'verify', return_value={'verified': True}))
        return stack

    def test_initial_failure_saved_before_compilation_and_contains_fresh_campaign(self):
        self.patches()
        def fail(_):
            initial = json.loads((self.artifacts / 'observation.json').read_text())
            self.assertFalse(initial['passed']); self.assertTrue(runner.canonical_uuid(initial['campaign_id']))
            self.assertFalse(initial['native_started'])
            raise runner.ProbeFailure('source-compilation-failed')
        with patch.object(runner, 'build_binary', side_effect=fail), patch.object(runner, 'run_logged') as launch:
            report = runner.observe(self.prefix, self.artifacts)
        launch.assert_not_called()
        self.assertFalse(report['passed']); self.assertEqual(report['failure'], 'source-compilation-failed')
        self.assertEqual(report['platform'], {'os': 'Darwin', 'version': '26.0.1', 'architecture': 'arm64'})

    def test_mocked_native_success_requires_bound_campaign_not_previous_report(self):
        self.patches()
        def run(_command, _path, _timeout, environment):
            initial = json.loads((self.artifacts / 'observation.json').read_text())
            self.assertEqual(environment['WORKBENCH_TEST_PYTHON_CAMPAIGN'], initial['campaign_id'])
            native, _ = successful_receipt(initial['campaign_id'])
            (self.artifacts / 'native-report.json').write_text(json.dumps(native))
            return SimpleNamespace(returncode=0)
        with patch.object(runner, 'run_logged', side_effect=run):
            report = runner.observe(self.prefix, self.artifacts)
        self.assertTrue(report['passed']); self.assertFalse(report['complete_release'])
        self.assertEqual(report['termination_state'], 'confirmed')
        old = copy.deepcopy(report['native']); old['campaign_id'] = str(uuid.uuid4())
        with patch.object(runner, 'run_logged', return_value=SimpleNamespace(returncode=0)):
            (self.artifacts / 'native-report.json').write_text(json.dumps(old))
            report = runner.observe(self.prefix, self.artifacts)
        self.assertFalse(report['passed']); self.assertEqual(report['failure'], 'native-observation-identity')

    def test_outer_timeout_retains_initial_native_identity_without_retry_or_scratch_read(self):
        self.patches()
        def timeout(_command, _path, _timeout, environment):
            native, _ = successful_receipt(environment['WORKBENCH_TEST_PYTHON_CAMPAIGN'])
            native.update(passed=False, phase='confined-compatibility', termination_state='unconfirmed',
                          last_worker_checkpoint=None, last_import_checkpoint=None, import_diagnostics=None,
                          exit_code=None, failure=None, result=None)
            (self.artifacts / 'native-report.json').write_text(json.dumps(native))
            raise subprocess.TimeoutExpired('synthetic-test', 120)
        with patch.object(runner, 'run_logged', side_effect=timeout) as launch:
            report = runner.observe(self.prefix, self.artifacts)
        launch.assert_called_once()
        self.assertFalse(report['passed']); self.assertEqual(report['failure'], 'native-termination-unverified')
        self.assertEqual(report['termination_state'], 'unverified')
        self.assertTrue(runner.canonical_uuid(report['native']['job_id']))
        self.assertEqual(report['native']['profile_sha256'], 'a' * 64)

    def test_failed_native_keeps_checked_identities_and_fixed_failure(self):
        self.patches()
        def fail(_command, _path, _timeout, environment):
            native, _ = successful_receipt(environment['WORKBENCH_TEST_PYTHON_CAMPAIGN'])
            native.update(passed=False, phase='confined-compatibility', last_worker_checkpoint='imports',
                          failure='compatibility-failed', exit_code=1, result={'path': '/private/sentinel'})
            (self.artifacts / 'native-report.json').write_text(json.dumps(native))
            return SimpleNamespace(returncode=1)
        with patch.object(runner, 'run_logged', side_effect=fail): report = runner.observe(self.prefix, self.artifacts)
        self.assertFalse(report['passed']); self.assertEqual(report['native']['last_worker_checkpoint'], 'imports')
        self.assertNotIn('/private/sentinel', json.dumps(report))
        self.assertEqual(report['failure'], 'native-test-process-failed')

    def test_quota_failure_retains_partial_timing_and_invalid_diagnostic_without_replacing_cause(self):
        self.patches()
        def fail(_command, _path, _timeout, environment):
            native, _ = successful_receipt(environment['WORKBENCH_TEST_PYTHON_CAMPAIGN'])
            native.update(passed=False, phase='confined-compatibility', last_worker_checkpoint='imports',
                          failure='quota-exhausted', quota_kind='wall-time', exit_code=None, result=None,
                          last_import_checkpoint={'module': 'spacy', 'boundary': 'before'})
            native['import_diagnostics']['checkpoints'] = native['import_diagnostics']['checkpoints'][:5]
            native['import_diagnostics'].update(valid=False, attempts=[{
                'module': 'numpy', 'ordinal': 0, 'elapsed_ms': 6, 'process_cpu_ms': 3}])
            (self.artifacts / 'native-report.json').write_text(json.dumps(native))
            return SimpleNamespace(returncode=1)
        with patch.object(runner, 'run_logged', side_effect=fail) as launched:
            report = runner.observe(self.prefix, self.artifacts)
        launched.assert_called_once()
        self.assertFalse(report['passed'])
        self.assertEqual(report['native']['failure'], 'quota-exhausted')
        self.assertEqual(report['native']['quota_kind'], 'wall-time')
        self.assertEqual(len(report['native']['import_diagnostics']['checkpoints']), 5)
        self.assertFalse(report['native']['import_diagnostics']['valid'])
        self.assertEqual(report['native']['import_diagnostics']['attempts'][0]['module'], 'numpy')
        self.assertEqual(report['termination_state'], 'confirmed')

    def test_unsupported_platform_never_verifies_or_spawns_candidate(self):
        self.patches()
        with patch.object(runner.platform, 'system', return_value='Windows'), \
                patch.object(runner.installed, 'verify') as verify, patch.object(runner, 'run_logged') as launch:
            report = runner.observe(self.prefix, self.artifacts)
        verify.assert_not_called(); launch.assert_not_called()
        self.assertFalse(report['passed']); self.assertEqual(report['failure'], 'unsupported-native-platform')

    def test_artifact_output_refuses_existing_nested_and_linked_ancestors(self):
        with self.assertRaises(FileExistsError): runner.prepare_artifacts(self.prefix, self.artifacts)
        with self.assertRaises(ValueError): runner.prepare_artifacts(self.prefix, self.prefix / 'nested')
        alias = self.base / 'alias'; alias.symlink_to(self.base, target_is_directory=True)
        with self.assertRaises((OSError, ValueError)): runner.prepare_artifacts(self.prefix, alias / 'output')
        self.assertFalse((self.base / 'output').exists())
        self.assertFalse((self.prefix / 'nested').exists())


if __name__ == '__main__':
    unittest.main()
