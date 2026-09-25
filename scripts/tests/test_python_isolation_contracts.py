"""Synthetic source contracts; no downloaded interpreter or package is executed."""
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
import test_python_isolation as runner
import relocate_python_prefix as relocation
import test_python_compatibility as common

spec = importlib.util.spec_from_file_location('fixed_hostile_source', common.ROOT / 'workers/python/probe/hostile.py')
hostile = importlib.util.module_from_spec(spec); spec.loader.exec_module(hostile)


def asset(data): return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def native_receipt(campaign):
    job = str(uuid.uuid4())
    executable = {'path': 'install/bin/python3.13', **asset(b'NEVER EXECUTED')}
    denied = {'opened': False, 'read_completed': False, 'errno': 1}
    attempt = {'attempted': True, 'socket_created': True, 'connected': False, 'send_accepted': False, 'received_echo': False, 'errno': 1}
    result = {'schema_version': 1, 'recipe': 'python-hostile-v1', 'job_id': job, 'manifest_sha256': common.MANIFEST,
        'python_version': '3.13.15', 'isolated': True, 'no_site': True, 'no_bytecode': True, 'verified_paths': True,
        'assigned_read': True, 'scratch_roundtrip': True, 'sentinel_sha256': hashlib.sha256(runner.SENTINEL).hexdigest(),
        'files': {name: copy.deepcopy(denied) for name in ['sibling_read', 'original_write_open', 'prefix_write_open']},
        'network': {'tcp': copy.deepcopy(attempt), 'udp': copy.deepcopy(attempt)}}
    native = {'schema_version': 1, 'recipe': 'python-hostile-v1', 'runtime_manifest_sha256': common.MANIFEST,
        'campaign_id': campaign, 'job_id': job, 'architecture': 'aarch64', 'runtime_verified': True,
        'candidate_interpreter': executable, 'profile_sha256': 'a' * 64,
        'assigned_files': {'code/bootstrap.py': asset((common.ROOT / 'workers/python/probe/hostile.py').read_bytes()),
                          'input/sentinel.txt': asset(runner.SENTINEL), 'input/assignment.json': asset(b'fixed assignment')},
        'termination_state': 'confirmed', 'passed': True, 'complete_release': False, 'phase': 'complete',
        'diagnostics_within_bound': True, 'last_worker_checkpoint': 'complete', 'last_import_checkpoint': None,
        'quota_kind': None, 'preparation_elapsed_ms': 10, 'supervised_elapsed_ms': 20, 'exit_code': 0, 'failure': None,
        'result': result, 'host_controls': {'before': True, 'after': True}, 'post_runtime_verified': True,
        'network_observer': {protocol: {'before': 1, 'after': 1, 'confined': 0, 'unexpected': 0, 'errors': 0}for protocol in ['tcp', 'udp']}}
    return native, executable


class IsolationContracts(unittest.TestCase):
    def test_hostile_receipt_requires_denial_controls_nonce_and_zero_delivery(self):
        campaign = str(uuid.uuid4()); native, executable = native_receipt(campaign)
        runner.accept_hostile(native, campaign, executable)
        changes = [lambda n: n.update(campaign_id=str(uuid.uuid4())), lambda n: n.update(termination_state='unverified'),
            lambda n: n.update(post_runtime_verified=False), lambda n: n['host_controls'].update(after=False),
            lambda n: n['network_observer']['udp'].update(confined=1), lambda n: n['network_observer']['tcp'].update(after=0),
            lambda n: n['network_observer']['udp'].update(errors=1), lambda n: n['network_observer']['udp'].update(unexpected=1),
            lambda n: n['result']['files']['sibling_read'].update(errno=2), lambda n: n['result']['files']['prefix_write_open'].update(opened=True),
            lambda n: n['result']['network']['tcp'].update(connected=True), lambda n: n['result']['network']['udp'].update(received_echo=True),
            lambda n: n['result'].update(assigned_read=False), lambda n: n['result'].update(scratch_roundtrip=False)]
        for change in changes:
            value = copy.deepcopy(native); change(value)
            with self.assertRaises(common.ProbeFailure): runner.accept_hostile(value, campaign, executable)

    def test_udp_send_acceptance_never_substitutes_for_parent_delivery_observation(self):
        campaign = str(uuid.uuid4()); native, executable = native_receipt(campaign)
        native['result']['network']['udp'].update(send_accepted=True, errno=60)
        runner.accept_hostile(native, campaign, executable)
        native['network_observer']['udp']['confined'] = 1
        with self.assertRaises(common.ProbeFailure): runner.accept_hostile(native, campaign, executable)

    def test_closed_failures_reject_untrusted_paths_counts_and_arbitrary_fields(self):
        campaign = str(uuid.uuid4()); native, _ = native_receipt(campaign)
        for key, value in [('post_runtime_verified', 1), ('host_controls', {'path': '/private/synthetic'}),
                           ('network_observer', {'tcp': {'private': 'x'}, 'udp': None})]:
            bad = copy.deepcopy(native); bad[key] = value
            with self.assertRaises(common.ProbeFailure): runner.hostile_summary(bad, campaign)
        bad = copy.deepcopy(native); bad['network_observer']['udp']['confined'] = False
        with self.assertRaises(common.ProbeFailure): runner.hostile_summary(bad, campaign)
        native['result'] = {'untrusted': '/private/synthetic'}
        self.assertNotIn('/private/synthetic', json.dumps(runner.hostile_summary(native, campaign)))

    def test_file_probe_opens_write_only_without_truncation_or_write_calls(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'original'; path.write_bytes(b'unchanged sentinel')
            with patch.object(hostile.os, 'write', side_effect=AssertionError('No write permitted')):
                result = hostile.file_attempt(path, write=True)
            self.assertTrue(result['opened']); self.assertFalse(result['read_completed'])
            self.assertEqual(path.read_bytes(), b'unchanged sentinel')
        with patch.object(hostile.os, 'open', side_effect=PermissionError(13, 'private message')):
            self.assertEqual(hostile.file_attempt('unused'), {'opened': False, 'read_completed': False, 'errno': 13})

    def test_network_attempt_reports_send_and_echo_independently_without_real_socket(self):
        channel = SimpleNamespace(settimeout=lambda _value: None, sendto=lambda data, _address: len(data),
            recvfrom=lambda _size: (_ for _ in ()).throw(TimeoutError()), close=lambda: None)
        with patch.object(hostile.socket, 'socket', return_value=channel):
            result = hostile.network_attempt('udp', 1234, b'a' * 32)
        self.assertTrue(result['send_accepted']); self.assertFalse(result['received_echo'])
        self.assertTrue(result['attempted']); self.assertTrue(result['socket_created'])

    def test_measurement_rejects_debug_profile_candidate_execution_stale_nonce_and_missing_duration(self):
        campaign = str(uuid.uuid4())
        native = {'schema_version': 1, 'recipe': 'trusted-prefix-hash-v1', 'campaign_id': campaign,
            'measurement_id': str(uuid.uuid4()), 'manifest_sha256': common.MANIFEST, 'passed': True,
            'complete_release': False, 'candidate_executed': False, 'build_profile': 'release',
            'debug_assertions': False, 'elapsed_ms': 100, 'failure': None}
        runner.accept_measurement(native, campaign)
        for key, value in [('candidate_executed', True), ('debug_assertions', True), ('build_profile', 'debug'),
                           ('campaign_id', str(uuid.uuid4())), ('elapsed_ms', None), ('elapsed_ms', True)]:
            bad = dict(native); bad[key] = value
            with self.assertRaises(common.ProbeFailure): runner.accept_measurement(bad, campaign)

    def test_release_build_requires_optimized_non_debug_artifact(self):
        with tempfile.TemporaryDirectory() as directory:
            artifacts = Path(directory); binary = artifacts / 'test'; binary.write_bytes(b'SYNTHETIC')
            def build(command, path, timeout):
                self.assertIn('--release', command); self.assertEqual(timeout, 300)
                path.write_text(json.dumps({'reason': 'compiler-artifact', 'profile': {'test': True, 'opt_level': '0', 'debug_assertions': True},
                    'target': {'name': 'workbench_core'}, 'executable': str(binary)}) + '\n')
                return SimpleNamespace(returncode=0)
            with patch.object(common, 'run_logged', side_effect=build):
                with self.assertRaisesRegex(common.ProbeFailure, 'wrong-native-build-profile'): common.build_binary(artifacts, release=True)


@unittest.skipUnless(os.name == 'posix', 'POSIX no-follow relocation')
class RelocationContracts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve(); self.source = self.base / 'source'; self.source.mkdir()
        self.destination = self.base / 'moved'
        self.entries = {'install/bin/python3.13': {**asset(b'SYNTHETIC NEVER EXECUTED'), 'executable': True},
                        'licenses/notice.txt': {**asset(b'SYNTHETIC NOTICE'), 'executable': False}}
        for path, data in [('install/bin/python3.13', b'SYNTHETIC NEVER EXECUTED'), ('licenses/notice.txt', b'SYNTHETIC NOTICE')]:
            target = self.source / path; target.parent.mkdir(parents=True, exist_ok=True); target.write_bytes(data)
            target.chmod(0o755 if self.entries[path]['executable'] else 0o644)
        raw = json.dumps({'files': self.entries}, sort_keys=True).encode()
        (self.source / 'manifest.json').write_bytes(raw); (self.source / 'manifest.json').chmod(0o644)
        self.pin = hashlib.sha256(raw).hexdigest()
        def verify_root(root, pin):
            raw = relocation.verifier.read_bytes(root, 'manifest.json')
            if hashlib.sha256(raw).hexdigest() != pin: raise ValueError('Synthetic manifest changed')
            entries = json.loads(raw)['files'] | {'manifest.json': {**asset(raw), 'executable': False}}
            relocation.files.verify_output(root, entries)
            return {'verified': True, 'files': len(entries), 'bytes': sum(v['bytes'] for v in entries.values())}
        self.verify_patch = patch.object(relocation.verifier, 'verify_root', side_effect=verify_root)
        self.verify_patch.start(); self.addCleanup(self.verify_patch.stop)

    def test_fresh_copy_preserves_all_bytes_modes_and_has_no_shared_inode(self):
        report = relocation.relocate(self.source, self.destination, self.pin)
        self.assertTrue(report['copied']); self.assertFalse(report['candidate_executed'])
        for name in list(self.entries) + ['manifest.json']:
            original, moved = self.source / name, self.destination / name
            self.assertEqual(original.read_bytes(), moved.read_bytes())
            self.assertEqual(original.stat().st_mode, moved.stat().st_mode)
            self.assertNotEqual(original.stat().st_ino, moved.stat().st_ino)

    def test_existing_nested_or_linked_destinations_fail_without_source_mutation(self):
        before = (self.source / 'manifest.json').read_bytes()
        self.destination.mkdir(); (self.destination / 'keep').write_text('unchanged')
        self.assertFalse(relocation.relocate(self.source, self.destination, self.pin)['copied'])
        self.assertEqual((self.destination / 'keep').read_text(), 'unchanged')
        self.assertFalse(relocation.relocate(self.source, self.source / 'nested', self.pin)['copied'])
        alias = self.base / 'alias'; alias.symlink_to(self.base, target_is_directory=True)
        self.assertFalse(relocation.relocate(self.source, alias / 'output', self.pin)['copied'])
        self.assertEqual((self.source / 'manifest.json').read_bytes(), before)
        self.assertFalse((self.source / 'nested').exists())

    def test_link_hardlink_unlisted_and_hash_corruption_are_rejected(self):
        sentinel = self.source / 'extra'
        for kind in ['link', 'hardlink', 'extra', 'changed']:
            if kind == 'link': sentinel.symlink_to(self.source / 'manifest.json')
            elif kind == 'hardlink': os.link(self.source / 'manifest.json', sentinel)
            elif kind == 'extra': sentinel.write_bytes(b'unlisted')
            else: (self.source / 'licenses/notice.txt').write_bytes(b'changed')
            self.assertFalse(relocation.relocate(self.source, self.destination, self.pin)['copied'])
            self.assertFalse(self.destination.exists())
            if sentinel.exists() or sentinel.is_symlink(): sentinel.unlink()

    def test_midcopy_source_change_removes_only_owned_partial_destination(self):
        original = relocation.assembler.FileInventory.copy
        calls = 0
        def corrupt(book, stream, root, path, size, executable=False):
            nonlocal calls
            result = original(book, stream, root, path, size, executable); calls += 1
            if calls == 1: (self.source / 'licenses/notice.txt').write_bytes(b'changed')
            return result
        with patch.object(relocation.assembler.FileInventory, 'copy', new=corrupt):
            report = relocation.relocate(self.source, self.destination, self.pin)
        self.assertFalse(report['copied']); self.assertFalse(self.destination.exists()); self.assertTrue(self.source.exists())

    def test_cleanup_failure_is_explicit_and_partial_output_is_retained(self):
        with patch.object(relocation.assembler.FileInventory, 'copy', side_effect=OSError('private message')), \
                patch.object(relocation.shutil, 'rmtree', side_effect=OSError('private cleanup')) as cleanup:
            cleanup.avoids_symlink_attacks = True
            report = relocation.relocate(self.source, self.destination, self.pin)
        self.assertFalse(report['copied']); self.assertIsNotNone(report['preceding_failure'])
        self.assertIn('retained for recovery', report['failure']); self.assertTrue(self.destination.exists())
        self.assertNotIn('private', json.dumps(report))



@unittest.skipUnless(os.name == 'posix', 'POSIX-only runner preparation')
class IsolationRunnerContracts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve(); self.prefix = self.base / 'prefix'; self.prefix.mkdir()
        self.artifacts = self.base / 'artifacts'; self.artifacts.mkdir()
        self.binary = self.base / 'compiled-test'; self.binary.write_bytes(b'NEVER EXECUTED')
        _, interpreter = native_receipt(str(uuid.uuid4()))
        (self.prefix / 'manifest.json').write_text(json.dumps({'files': {'install/bin/python3.13': interpreter}}))
        self.stack = ExitStack(); self.addCleanup(self.stack.close)
        self.stack.enter_context(patch.object(runner, 'source_identity', return_value={'commit': 'synthetic'}))
        for name, value in [('system', 'Darwin'), ('machine', 'arm64'), ('mac_ver', ('26.6.2', (), ''))]:
            self.stack.enter_context(patch.object(runner.platform, name, return_value=value))
        self.verifier = self.stack.enter_context(patch.object(common.installed, 'verify', return_value={'verified': True}))
        self.build = self.stack.enter_context(patch.object(common, 'build_binary', return_value=self.binary))

    def test_measurement_runs_only_fixed_trusted_rust_test_without_candidate_flag(self):
        def launch(command, _log, timeout, environment):
            self.assertIn(runner.HASH_TEST, command); self.assertEqual(timeout, 120)
            initial = json.loads((self.artifacts / 'observation.json').read_bytes())
            self.assertFalse(initial['candidate_started']); self.assertFalse(initial['passed'])
            native = {'schema_version': 1, 'recipe': 'trusted-prefix-hash-v1', 'campaign_id': environment['WORKBENCH_TEST_PYTHON_CAMPAIGN'],
                'measurement_id': str(uuid.uuid4()), 'manifest_sha256': common.MANIFEST, 'passed': True, 'complete_release': False,
                'candidate_executed': False, 'build_profile': 'release', 'debug_assertions': False, 'elapsed_ms': 1, 'failure': None}
            (self.artifacts / 'native-report.json').write_text(json.dumps(native))
            return SimpleNamespace(returncode=0)
        with patch.object(common, 'run_logged', side_effect=launch): report = runner.observe(self.prefix, self.artifacts, 'measure')
        self.assertTrue(report['passed']); self.assertFalse(report['candidate_started'])
        self.build.assert_called_once_with(self.artifacts, release=True)

    def test_hostile_failure_still_checks_both_prefixes_after_confirmed_termination(self):
        def launch(_command, _log, _timeout, environment):
            native, _ = native_receipt(environment['WORKBENCH_TEST_PYTHON_CAMPAIGN'])
            native.update(passed=False, phase='confined-hostile', failure='compatibility-failed')
            (self.artifacts / 'native-report.json').write_text(json.dumps(native))
            return SimpleNamespace(returncode=1)
        with patch.object(common, 'run_logged', side_effect=launch): report = runner.observe(self.prefix, self.artifacts, 'hostile')
        self.assertFalse(report['passed']); self.assertEqual(report['termination_state'], 'confirmed')
        self.assertEqual(self.verifier.call_count, 3)
        self.assertTrue(report['post_original_inventory']['verified'])

    def test_unverified_worker_termination_does_not_hash_prefixes_or_retry(self):
        def launch(_command, _log, _timeout, environment):
            native, _ = native_receipt(environment['WORKBENCH_TEST_PYTHON_CAMPAIGN'])
            native.update(passed=False, phase='confined-hostile', failure='termination-unverified', termination_state='unverified')
            (self.artifacts / 'native-report.json').write_text(json.dumps(native))
            return SimpleNamespace(returncode=1)
        with patch.object(common, 'run_logged', side_effect=launch) as run:
            report = runner.observe(self.prefix, self.artifacts, 'hostile')
        run.assert_called_once(); self.assertEqual(self.verifier.call_count, 1)
        self.assertEqual(report['failure'], 'native-termination-unverified'); self.assertFalse(report['passed'])

    def test_outer_timeout_retains_parent_identity_without_prefix_verification_or_retry(self):
        def launch(_command, _log, _timeout, environment):
            native, _ = native_receipt(environment['WORKBENCH_TEST_PYTHON_CAMPAIGN'])
            native.update(passed=False, phase='confined-hostile', termination_state='unconfirmed', result=None)
            (self.artifacts / 'native-report.json').write_text(json.dumps(native))
            raise subprocess.TimeoutExpired('fixed-test', 120)
        with patch.object(common, 'run_logged', side_effect=launch) as run:
            report = runner.observe(self.prefix, self.artifacts, 'hostile')
        run.assert_called_once(); self.assertEqual(self.verifier.call_count, 1)
        self.assertFalse(report['passed']); self.assertEqual(report['termination_state'], 'unverified')
        self.assertTrue(common.canonical_uuid(report['native']['job_id']))

    def test_failed_relocation_or_build_cannot_launch_candidate(self):
        with patch.object(runner.relocation, 'relocate', return_value={'copied': False}), patch.object(common, 'run_logged') as launch:
            report = runner.observe(self.prefix, self.artifacts, 'relocated')
        launch.assert_not_called(); self.assertFalse(report['candidate_started']); self.assertFalse(report['passed'])
        with patch.object(common, 'build_binary', side_effect=common.ProbeFailure('source-compilation-failed')), \
                patch.object(common, 'run_logged') as launch:
            report = runner.observe(self.prefix, self.artifacts, 'hostile')
        launch.assert_not_called(); self.assertFalse(report['candidate_started']); self.assertFalse(report['passed'])

if __name__ == '__main__':
    unittest.main()
