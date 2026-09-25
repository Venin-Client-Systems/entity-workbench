#!/usr/bin/env python3
"""Explicitly invoked development probe; never a production worker entry point."""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import stat
import subprocess
import uuid

import verify_python_install as installed
import verify_runtime_bundle as inventory
import install_python_offline as assembler

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = '4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822'
NATIVE_TEST = 'engines::supervision::python_probe::native_python_compatibility'
SOURCES = ('crates/core/src/engines/supervision.rs', 'crates/core/src/engines/supervision/python_probe.rs',
           'workers/python/probe/bootstrap.py', 'workers/python/probe/compatibility.py',
           'workers/python/probe/fixture.json', 'workers/python/probe/expected.json',
           'workers/python/transaction_totals.py', 'scripts/test_python_compatibility.py',
           'scripts/verify_python_install.py', 'scripts/install_python_offline.py',
           'scripts/stage_python_runtime.py', 'scripts/verify_runtime_bundle.py')


class ProbeFailure(Exception):
    """Only fixed failure categories may be placed in the public report."""


def require(ok, failure):
    if not ok:
        raise ProbeFailure(failure)


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def source_identity():
    def git(*args):
        return subprocess.check_output(['git', *args], cwd=ROOT, timeout=10, text=True).strip()
    require(not git('status', '--porcelain'), 'source-is-not-clean')
    return {'commit': git('rev-parse', 'HEAD'), 'tree': git('rev-parse', 'HEAD^{tree}'),
            'parents': git('show', '-s', '--format=%P', 'HEAD').split(),
            'files': {name: digest(ROOT / name) for name in SOURCES}}


def read_json(path, maximum):
    require(path.is_file() and not path.is_symlink() and path.stat().st_nlink == 1
            and path.stat().st_size <= maximum, 'invalid-observation-file')
    raw = path.read_bytes()
    require(len(raw) <= maximum, 'observation-exceeds-limit')
    return json.loads(raw, object_pairs_hook=inventory.unique_object)


NATIVE_KEYS = {'schema_version', 'recipe', 'runtime_manifest_sha256', 'passed', 'complete_release',
               'phase', 'diagnostics_within_bound', 'last_worker_checkpoint', 'exit_code', 'failure', 'result',
               'campaign_id', 'job_id', 'architecture', 'runtime_verified', 'candidate_interpreter',
               'profile_sha256', 'assigned_files', 'termination_state', 'last_import_checkpoint', 'quota_kind',
               'preparation_elapsed_ms', 'supervised_elapsed_ms'}
IMPORTS = ('duckdb', 'networkx', 'spacy', 'click', 'splink', 'pyarrow')
ASSIGNED = {'code/bootstrap.py': 'workers/python/probe/bootstrap.py',
            'code/compatibility.py': 'workers/python/probe/compatibility.py',
            'code/transaction_totals.py': 'workers/python/transaction_totals.py',
            'input/fixture.json': 'workers/python/probe/fixture.json',
            'input/reader.json': None, 'input/assignment.json': None}


def canonical_uuid(value):
    try:
        return isinstance(value, str) and str(uuid.UUID(value)) == value
    except ValueError:
        return False


def sha256(value):
    return isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value) is not None


def asset_identity(value, maximum, path=None):
    return (isinstance(value, dict) and set(value) == ({'bytes', 'sha256'} if path is None else {'path', 'bytes', 'sha256'})
            and type(value['bytes']) is int and 0 < value['bytes'] <= maximum and sha256(value['sha256'])
            and (path is None or value['path'] == path))


def failure_summary(native, campaign):
    # A malformed receipt cannot carry arbitrary strings, file names or identities into public evidence.
    require(isinstance(native, dict) and set(native) == NATIVE_KEYS, 'native-observation-shape')
    require(type(native['schema_version']) is int and native['schema_version'] == 1
            and native['recipe'] == 'python-compatibility-v1' and native['runtime_manifest_sha256'] == MANIFEST
            and native['complete_release'] is False and native['campaign_id'] == campaign
            and canonical_uuid(campaign) and canonical_uuid(native['job_id']) and native['architecture'] == 'aarch64'
            and type(native['runtime_verified']) is bool and type(native['diagnostics_within_bound']) is bool,
            'native-observation-identity')
    require(native['phase'] in ('not-started', 'runtime-inventory', 'confined-compatibility', 'complete')
            and (native['last_worker_checkpoint'] is None or native['last_worker_checkpoint'] in
                 ('bootstrap', 'versions', 'imports', 'mentions', 'graph', 'transactions', 'plugins', 'complete'))
            and (native['failure'] is None or native['failure'] in
                 ('termination-unverified', 'cleanup-failed', 'quota-exhausted', 'compatibility-failed'))
            and (native['exit_code'] is None or type(native['exit_code']) is int and -128 <= native['exit_code'] <= 2**31 - 1)
            and type(native['passed']) is bool and native['termination_state'] in
                 ('not-started', 'unconfirmed', 'unverified', 'confirmed'), 'unsafe-native-failure-observation')
    require(native['candidate_interpreter'] is None or asset_identity(native['candidate_interpreter'], 80 * 1024**2,
                                                                    'install/bin/python3.13'), 'unsafe-interpreter-identity')
    require(native['profile_sha256'] is None or sha256(native['profile_sha256']), 'unsafe-profile-identity')
    checkpoint = native['last_import_checkpoint']
    require(checkpoint is None or isinstance(checkpoint, dict) and set(checkpoint) == {'module', 'boundary'}
            and checkpoint['module'] in IMPORTS and checkpoint['boundary'] in ('before', 'after'),
            'unsafe-import-checkpoint')
    require(native['quota_kind'] is None or native['quota_kind'] in
            ('wall-time', 'tree-depth', 'tree-entry-count', 'tree-or-file-bytes', 'tree-size-overflow', 'other'),
            'unsafe-quota-kind')
    require(all(native[key] is None or type(native[key]) is int and 0 <= native[key] <= 86_400_000
                for key in ('preparation_elapsed_ms', 'supervised_elapsed_ms')), 'unsafe-elapsed-observation')
    assigned = native['assigned_files']
    require(isinstance(assigned, dict) and set(assigned) <= set(ASSIGNED)
            and all(asset_identity(value, 64 * 1024) for value in assigned.values()), 'unsafe-assigned-identity')
    return {key: native[key] for key in NATIVE_KEYS - {'result'}}


def accept_native(native, campaign, interpreter):
    failure_summary(native, campaign)
    require(native['passed'] is True and native['phase'] == 'complete' and native['runtime_verified'] is True
            and native['diagnostics_within_bound'] is True and native['last_worker_checkpoint'] == 'complete'
            and type(native['exit_code']) is int and native['exit_code'] == 0 and native['failure'] is None
            and native['termination_state'] == 'confirmed' and sha256(native['profile_sha256'])
            and native['candidate_interpreter'] == interpreter and set(native['assigned_files']) == set(ASSIGNED)
            and native['last_import_checkpoint'] == {'module': 'pyarrow', 'boundary': 'after'}
            and native['quota_kind'] is None and native['preparation_elapsed_ms'] is not None
            and native['supervised_elapsed_ms'] is not None,
            'native-compatibility-failed')
    for relative, source in ASSIGNED.items():
        if relative == 'input/assignment.json':
            continue  # Contains private assigned-prefix spelling; job identity is checked separately below.
        data = (ROOT / source).read_bytes() if source else b'{"synthetic":true,"reference":"000123"}'
        require(native['assigned_files'][relative] == {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()},
                'native-assignment-identity')
    result = native['result']
    require(isinstance(result, dict) and set(result) == {'schema_version', 'recipe', 'job_id', 'manifest_sha256',
                'python_version', 'isolated', 'no_site', 'no_bytecode', 'verified_paths', 'checks'}, 'native-result-shape')
    require(type(result['schema_version']) is int and result['schema_version'] == 1
            and result['recipe'] == 'python-compatibility-v1' and result['manifest_sha256'] == MANIFEST
            and result['python_version'] == '3.13.15' and result['job_id'] == native['job_id']
            and all(result[field] is True for field in ('isolated', 'no_site', 'no_bytecode', 'verified_paths')),
            'native-result-identity')
    expected = read_json(ROOT / 'workers/python/probe/expected.json', 64 * 1024)
    # JSON comparison preserves the difference between booleans and integers.
    canonical = lambda value: json.dumps(value, sort_keys=True, allow_nan=False, separators=(',', ':'))
    require(canonical(result['checks']) == canonical(expected), 'native-result-assertions')


def prepare_artifacts(prefix, artifacts):
    require(os.name == 'posix', 'unsupported-artifact-host')
    parent = source = None
    try:
        parent, name = assembler.files.parent_at(artifacts)
        if prefix.exists():
            source = assembler.open_directory(prefix)
            assembler.reject_nested_output(parent, (source,))
        os.mkdir(name, 0o700, dir_fd=parent)
    finally:
        for fd in (parent, source):
            if fd is not None:
                os.close(fd)


def run_logged(command, path, timeout, environment=None):
    with path.open('xb') as output:
        os.fchmod(output.fileno(), 0o600)
        result = subprocess.run(command, cwd=ROOT, env=environment, stdin=subprocess.DEVNULL,
                                stdout=output, stderr=subprocess.STDOUT, timeout=timeout, check=False)
        require(output.tell() <= 32 * 1024**2, 'tool-log-exceeds-limit')
    return result


def build_binary(artifacts):
    command = ['cargo', 'test', '--offline', '--locked', '-p', 'workbench-core', '--lib', '--no-run', '--message-format=json']
    log = artifacts / 'build.log'
    result = run_logged(command, log, 300)
    require(result.returncode == 0, 'source-compilation-failed')
    binaries = set()
    with log.open(encoding='utf-8') as stream:
        for line in stream:
            require(len(line) <= 4 * 1024**2, 'compiler-message-exceeds-limit')
            if not line.startswith('{'):
                continue
            item = json.loads(line)
            if (item.get('reason') == 'compiler-artifact' and item.get('profile', {}).get('test') is True
                    and item.get('target', {}).get('name') == 'workbench_core' and item.get('executable')):
                binaries.add(Path(item['executable']))
    require(len(binaries) == 1, 'ambiguous-compiled-native-test')
    binary = binaries.pop()
    metadata = binary.lstat()
    require(stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1, 'invalid-compiled-native-test')
    return binary


def save(artifacts, report):
    # This is an exclusively owned fresh development-artifact directory.
    encoded = (json.dumps(report, indent=2, sort_keys=True, allow_nan=False) + '\n').encode()
    require(len(encoded) <= 2 * 1024**2, 'campaign-observation-exceeds-limit')
    temporary = artifacts / 'observation.pending'
    with temporary.open('xb') as stream:
        os.fchmod(stream.fileno(), 0o600)
        stream.write(encoded)
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(artifacts / 'observation.json')


def observe(prefix, artifacts):
    report = {'schema_version': 1, 'observed_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'scope': 'test-only macOS arm64 confined Python compatibility', 'passed': False, 'complete_release': False,
              'phase': 'not-started', 'failure': None, 'native_started': False, 'native': None,
              'campaign_id': str(uuid.uuid4()), 'platform': None, 'termination_state': 'not-started',
              'runtime_manifest_sha256': MANIFEST, 'unverified': ['hostile-file-network-controls', 'fresh-relocation',
                 'hard-resident-memory-ceiling', 'supervisor-crash-termination', 'canonical-worker-protocol',
                 'signing-clean-install-minimum-os', 'Intel-Mac-and-Windows', 'complete-notices']}
    save(artifacts, report)  # A failure/timeout cannot leave a previous successful run.
    try:
        report['phase'] = 'source-identity'
        identity = source_identity()
        report['source'] = identity
        report['phase'] = 'platform'
        host = {'os': platform.system(), 'version': platform.mac_ver()[0], 'architecture': platform.machine()}
        require(host['os'] == 'Darwin' and host['architecture'] == 'arm64'
                and re.fullmatch(r'[0-9]{1,3}\.[0-9]{1,3}(?:\.[0-9]{1,3})?', host['version']), 'unsupported-native-platform')
        report['platform'] = host
        report['phase'] = 'runtime-inventory'
        verification = installed.verify(prefix, MANIFEST)
        report['inventory'] = verification
        require(verification.get('verified') is True, 'runtime-inventory-rejected')
        manifest = read_json(prefix / 'manifest.json', 16 * 1024**2)
        executable = manifest['files']['install/bin/python3.13']
        interpreter = {'path': 'install/bin/python3.13', 'bytes': executable['bytes'], 'sha256': executable['sha256']}
        require(asset_identity(interpreter, 80 * 1024**2, 'install/bin/python3.13'), 'invalid-interpreter-inventory')
        report['phase'] = 'compile-native-test'; save(artifacts, report)
        binary = build_binary(artifacts)
        report['native_binary_sha256'] = digest(binary)
        require(source_identity() == identity, 'source-changed-during-build')
        report['phase'] = 'confined-compatibility'
        report['native_started'] = True
        report['termination_state'] = 'unconfirmed'
        save(artifacts, report)
        environment = dict(os.environ, WORKBENCH_TEST_PYTHON_PREFIX=str(prefix.absolute()),
                           WORKBENCH_TEST_PYTHON_ARTIFACTS=str(artifacts.absolute()),
                           WORKBENCH_TEST_PYTHON_CAMPAIGN=report['campaign_id'])
        completed = run_logged([str(binary), '--ignored', '--exact', NATIVE_TEST, '--test-threads=1', '--nocapture'],
                               artifacts / 'native-test.log', 120, environment)
        native = read_json(artifacts / 'native-report.json', 1024 * 1024)
        # Failed native observations have closed fields; never copy arbitrary raw logs into the public report.
        report['native_report_sha256'] = digest(artifacts / 'native-report.json')
        report['native'] = failure_summary(native, report['campaign_id'])
        report['termination_state'] = native['termination_state']
        require(completed.returncode == 0, 'native-test-process-failed')
        accept_native(native, report['campaign_id'], interpreter)
        require(source_identity() == identity and digest(binary) == report['native_binary_sha256'], 'source-or-binary-changed')
        report['native'] = native
        report.update(passed=True, phase='complete')
    except ProbeFailure as error:
        report['failure'] = str(error)
    except subprocess.TimeoutExpired:
        report['failure'] = 'native-termination-unverified' if report['native_started'] else 'source-tool-timeout'
        if report['native_started']:
            report['termination_state'] = 'unverified'
            # Read only the parent's initial receipt; no job scratch or worker diagnostics after this timeout.
            try:
                native = read_json(artifacts / 'native-report.json', 1024 * 1024)
                report['native'] = failure_summary(native, report['campaign_id'])
                report['native_report_sha256'] = digest(artifacts / 'native-report.json')
            except (OSError, ValueError, TypeError, KeyError, ProbeFailure):
                pass
    except (OSError, ValueError, TypeError, KeyError, subprocess.SubprocessError):
        report['failure'] = 'invalid-or-unavailable-probe-input'
    finally:
        save(artifacts, report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prefix', type=Path, required=True)
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--execute-reviewed-probe', action='store_true')
    args = parser.parse_args()
    if not args.execute_reviewed_probe:
        parser.error('This command executes the candidate; explicit reviewed invocation is required')
    try:
        prepare_artifacts(args.prefix, args.artifacts)  # Fresh only; no immutable-prefix writes.
        report = observe(args.prefix, args.artifacts)
    except (OSError, ValueError, ProbeFailure):
        report = {'passed': False, 'complete_release': False, 'failure': 'artifact-or-observation-failed'}
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
