#!/usr/bin/env python3
"""Explicit test-only Python boundary/relocation campaigns and trusted hash measurement."""
from __future__ import annotations
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import uuid

import test_python_compatibility as common
import relocate_python_prefix as relocation
import python_engine_receipts as engines

ROOT = common.ROOT
HOSTILE_TEST = 'engines::supervision::python_probe::hostile::native_python_hostile'
HASH_TEST = 'engines::supervision::python_probe::native_python_prefix_hash_measurement'
EXTRA_SOURCES = ('Cargo.toml', 'Cargo.lock', 'scripts/test_python_isolation.py', 'scripts/relocate_python_prefix.py', 'scripts/python_engine_receipts.py',
                 'workers/python/probe/hostile.py', 'crates/core/src/engines/supervision/python_probe/hostile.rs',
                 'crates/core/src/engines/supervision/python_probe/listeners.rs')
HOSTILE_ASSIGNED = {'code/bootstrap.py', 'input/sentinel.txt', 'input/assignment.json'}
HOSTILE_EXTRA_KEYS = {'host_controls', 'network_observer', 'post_runtime_verified'}
DENIED = {1, 13}  # Darwin EPERM/EACCES, never ENOENT or a generic exception.
SENTINEL = b'fixed assigned synthetic sentinel\n'
require = common.require


def source_identity():
    identity = common.source_identity()
    identity['files'].update({name: common.digest(ROOT / name) for name in EXTRA_SOURCES})
    return identity


def counts_valid(counts):
    return (isinstance(counts, dict) and set(counts) == {'before', 'after', 'confined', 'unexpected', 'errors'}
            and all(type(value) is int and 0 <= value <= 8 for value in counts.values()))


def hostile_summary(native, campaign):
    require(isinstance(native, dict) and set(native) == common.NATIVE_KEYS | HOSTILE_EXTRA_KEYS, 'hostile-observation-shape')
    result = common.failure_summary({key: value for key, value in native.items() if key not in HOSTILE_EXTRA_KEYS},
                                    campaign, recipe='python-hostile-v1', assigned_names=HOSTILE_ASSIGNED)
    require(isinstance(native['host_controls'], dict) and set(native['host_controls']) == {'before', 'after'}
            and all(type(value) is bool for value in native['host_controls'].values()), 'unsafe-host-control')
    require(isinstance(native['network_observer'], dict) and set(native['network_observer']) == {'tcp', 'udp'}
            and all(value is None or counts_valid(value) for value in native['network_observer'].values()), 'unsafe-network-counts')
    require(type(native['post_runtime_verified']) is bool, 'unsafe-post-runtime-state')
    result.update({key: native[key] for key in HOSTILE_EXTRA_KEYS})
    return result


def accept_hostile(native, campaign, interpreter):
    hostile_summary(native, campaign)
    require(native['passed'] is True and native['phase'] == 'complete' and native['runtime_verified'] is True
            and native['post_runtime_verified'] is True and native['host_controls'] == {'before': True, 'after': True}
            and native['termination_state'] == 'confirmed' and native['exit_code'] == 0 and native['failure'] is None
            and native['quota_kind'] is None and native['last_worker_checkpoint'] == 'complete'
            and native['last_import_checkpoint'] is None and native['diagnostics_within_bound'] is True
            and common.sha256(native['profile_sha256']) and native['candidate_interpreter'] == interpreter
            and native['preparation_elapsed_ms'] is not None and native['supervised_elapsed_ms'] is not None,
            'hostile-boundary-failed')
    require(set(native['assigned_files']) == HOSTILE_ASSIGNED, 'hostile-assignment-shape')
    for name, data in [('code/bootstrap.py', (ROOT / 'workers/python/probe/hostile.py').read_bytes()), ('input/sentinel.txt', SENTINEL)]:
        require(native['assigned_files'][name] == {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}, 'hostile-code-input-mismatch')
    for value in native['network_observer'].values():
        require(value == {'before': 1, 'after': 1, 'confined': 0, 'unexpected': 0, 'errors': 0}, 'network-delivery-control-failed')
    result = native['result']
    require(isinstance(result, dict) and set(result) == {'schema_version', 'recipe', 'job_id', 'manifest_sha256',
        'python_version', 'isolated', 'no_site', 'no_bytecode', 'verified_paths', 'assigned_read', 'scratch_roundtrip',
        'sentinel_sha256', 'files', 'network'}, 'hostile-result-shape')
    require(type(result['schema_version']) is int and result['schema_version'] == 1 and result['recipe'] == 'python-hostile-v1'
            and result['job_id'] == native['job_id'] and result['manifest_sha256'] == common.MANIFEST
            and result['python_version'] == '3.13.15' and result['sentinel_sha256'] == hashlib.sha256(SENTINEL).hexdigest()
            and all(result[key] is True for key in ('isolated', 'no_site', 'no_bytecode', 'verified_paths', 'assigned_read', 'scratch_roundtrip')),
            'hostile-result-identity')
    require(isinstance(result['files'], dict) and set(result['files']) == {'sibling_read', 'original_write_open', 'prefix_write_open'},
            'hostile-file-shape')
    for value in result['files'].values():
        require(isinstance(value, dict) and set(value) == {'opened', 'read_completed', 'errno'}
                and value['opened'] is False and value['read_completed'] is False
                and type(value['errno']) is int and value['errno'] in DENIED, 'hostile-file-denial-failed')
    require(isinstance(result['network'], dict) and set(result['network']) == {'tcp', 'udp'}, 'hostile-network-shape')
    for value in result['network'].values():
        require(isinstance(value, dict) and set(value) == {'attempted', 'socket_created', 'connected', 'send_accepted', 'received_echo', 'errno'}
                and all(type(value[key]) is bool for key in ('attempted', 'socket_created', 'connected', 'send_accepted', 'received_echo'))
                and type(value['errno']) is int and value['attempted'] is True and value['received_echo'] is False, 'hostile-network-attempt-invalid')
    tcp, udp = result['network']['tcp'], result['network']['udp']
    require(not tcp['connected'] and not tcp['send_accepted'] and tcp['errno'] in DENIED and not udp['connected']
            and ((udp['socket_created'] and udp['send_accepted'] and udp['errno'] == 60)
                 or (not udp['send_accepted'] and udp['errno'] in DENIED)), 'hostile-network-denial-failed')


def measurement_summary(native, campaign):
    require(isinstance(native, dict) and set(native) == {'schema_version', 'recipe', 'campaign_id', 'measurement_id',
        'manifest_sha256', 'passed', 'complete_release', 'candidate_executed', 'build_profile', 'debug_assertions', 'elapsed_ms', 'failure'},
        'measurement-shape')
    require(type(native['schema_version']) is int and native['schema_version'] == 1 and native['recipe'] == 'trusted-prefix-hash-v1'
            and native['campaign_id'] == campaign and common.canonical_uuid(native['measurement_id'])
            and native['manifest_sha256'] == common.MANIFEST and type(native['passed']) is bool and native['complete_release'] is False
            and native['candidate_executed'] is False and native['build_profile'] == 'release' and native['debug_assertions'] is False
            and (native['elapsed_ms'] is None or type(native['elapsed_ms']) is int and 0 <= native['elapsed_ms'] <= 120_000)
            and native['failure'] in (None, 'prefix-verification-failed'), 'unsafe-prefix-measurement')
    return dict(native)


def accept_measurement(native, campaign):
    measurement_summary(native, campaign)
    require(native['passed'] is True and native['elapsed_ms'] is not None and native['failure'] is None,
            'trusted-prefix-measurement-failed')


def native_summary(native, campaign, case):
    if case == 'measure': return measurement_summary(native, campaign)
    if case == 'hostile': return hostile_summary(native, campaign)
    if case in engines.RECIPES: return engines.summary(native, campaign, case)
    return common.failure_summary(native, campaign)


def observe(prefix, artifacts, case):
    require(case in ('measure', 'hostile', 'relocated', 'networkx', 'transactions'), 'unknown-isolation-case')
    report = {'schema_version': 1, 'observed_at': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'case': case,
              'campaign_id': str(uuid.uuid4()), 'passed': False, 'complete_release': False, 'failure': None,
              'build_profile': 'release', 'native_test_started': False, 'candidate_started': False,
              'termination_state': 'not-started', 'phase': 'not-started', 'native': None,
              'runtime_manifest_sha256': common.MANIFEST,
              'unverified': ['Internet-and-DNS-and-IPv6-denial', 'hard-RSS-ceiling', 'supervisor-crash-termination',
                             'canonical-protocol', 'signing-clean-install-platform-matrix', 'complete-notices']}
    common.save(artifacts, report)
    try:
        report['phase'] = 'source-identity'; identity = source_identity(); report['source'] = identity
        host = {'os': platform.system(), 'version': platform.mac_ver()[0], 'architecture': platform.machine()}
        require(host['os'] == 'Darwin' and host['architecture'] == 'arm64' and common.re.fullmatch(r'[0-9]{1,3}\.[0-9]{1,3}(?:\.[0-9]{1,3})?', host['version']), 'unsupported-native-platform')
        report['platform'] = host; report['phase'] = 'runtime-inventory'
        report['original_inventory'] = common.installed.verify(prefix, common.MANIFEST)
        require(report['original_inventory'].get('verified') is True, 'runtime-inventory-failed')
        selected = prefix
        if case == 'relocated':
            report['phase'] = 'relocation'; common.save(artifacts, report)
            selected = artifacts / 'moved-prefix'
            report['relocation'] = relocation.relocate(prefix, selected)
            require(report['relocation'].get('copied') is True, 'relocation-copy-failed')
        manifest = common.read_json(selected / 'manifest.json', 16 * 1024**2)
        asset = manifest['files']['install/bin/python3.13']
        interpreter = {'path': 'install/bin/python3.13', 'bytes': asset['bytes'], 'sha256': asset['sha256']}
        require(common.asset_identity(interpreter, 80 * 1024**2, 'install/bin/python3.13'), 'invalid-interpreter-inventory')
        report['phase'] = 'compile-release-native-test'; common.save(artifacts, report)
        binary = common.build_binary(artifacts, release=True); report['native_binary_sha256'] = common.digest(binary)
        require(source_identity() == identity, 'source-changed-during-build')
        test = HASH_TEST if case == 'measure' else HOSTILE_TEST if case == 'hostile' else engines.TESTS[case] if case in engines.RECIPES else common.NATIVE_TEST
        report['native_test'] = test; report['native_test_started'] = True; report['candidate_started'] = case != 'measure'
        report['phase'] = 'trusted-prefix-hash' if case == 'measure' else 'confined-' + case
        report['termination_state'] = 'not-applicable' if case == 'measure' else 'unconfirmed'; common.save(artifacts, report)
        environment = dict(os.environ, WORKBENCH_TEST_PYTHON_PREFIX=str(selected.absolute()),
                           WORKBENCH_TEST_PYTHON_ARTIFACTS=str(artifacts.absolute()), WORKBENCH_TEST_PYTHON_CAMPAIGN=report['campaign_id'])
        completed = common.run_logged([str(binary), '--ignored', '--exact', test, '--test-threads=1', '--nocapture'],
                                      artifacts / 'native-test.log', 120, environment)
        native = common.read_json(artifacts / 'native-report.json', 1024**2)
        report['native_report_sha256'] = common.digest(artifacts / 'native-report.json')
        if case == 'measure':
            report['native'] = measurement_summary(native, report['campaign_id'])
            accept_measurement(native, report['campaign_id'])
        else:
            report['native'] = native_summary(native, report['campaign_id'], case)
            report['termination_state'] = native['termination_state']
            if native['termination_state'] not in ('confirmed', 'not-started'):
                report['termination_state'] = 'unverified'
                raise common.ProbeFailure('native-termination-unverified')
            # Verify after both successful and confirmed failed executions, without changing source bytes.
            report['post_original_inventory'] = common.installed.verify(prefix, common.MANIFEST)
            report['post_selected_inventory'] = common.installed.verify(selected, common.MANIFEST)
            require(report['post_original_inventory'].get('verified') is True and report['post_selected_inventory'].get('verified') is True,
                    'post-campaign-inventory-failed')
            if case == 'hostile': accept_hostile(native, report['campaign_id'], interpreter)
            elif case in engines.RECIPES: engines.accept(native, report['campaign_id'], interpreter, case)
            else: common.accept_native(native, report['campaign_id'], interpreter)
        require(completed.returncode == 0, 'native-test-process-failed')
        require(source_identity() == identity and common.digest(binary) == report['native_binary_sha256'], 'source-or-binary-changed')
        report.update(passed=True, phase='complete', native=native)
    except common.ProbeFailure as error:
        report['failure'] = str(error)
    except subprocess.TimeoutExpired:
        report['failure'] = 'native-termination-unverified' if report['candidate_started'] else 'trusted-tool-timeout'
        if report['candidate_started']: report['termination_state'] = 'unverified'
        # Read only the bounded parent receipt, never candidate scratch, after an outer timeout.
        try:
            native = common.read_json(artifacts / 'native-report.json', 1024**2)
            report['native'] = native_summary(native, report['campaign_id'], case)
            report['native_report_sha256'] = common.digest(artifacts / 'native-report.json')
        except (OSError, ValueError, TypeError, KeyError, common.ProbeFailure):
            pass
        # No automatic retry, candidate output read, post-verification or cleanup on unknown termination.
    except (OSError, ValueError, TypeError, KeyError, subprocess.SubprocessError):
        report['failure'] = 'invalid-or-unavailable-isolation-input'
    finally:
        common.save(artifacts, report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prefix', type=Path, required=True)
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--case', choices=('measure', 'hostile', 'relocated', 'networkx', 'transactions'), required=True)
    parser.add_argument('--execute-reviewed-probe', action='store_true')
    args = parser.parse_args()
    if args.case != 'measure' and not args.execute_reviewed_probe:
        parser.error('Candidate execution requires explicit reviewed invocation')
    try:
        common.prepare_artifacts(args.prefix, args.artifacts)
        report = observe(args.prefix, args.artifacts, args.case)
    except (OSError, ValueError, common.ProbeFailure):
        report = {'passed': False, 'complete_release': False, 'failure': 'artifact-or-observation-failed'}
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
