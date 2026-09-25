#!/usr/bin/env python3
"""One explicit native host/API experiment against pre-staged resources. Never stages or retries."""
import argparse
import datetime
import importlib.util
import json
import os
from pathlib import Path
import platform
import stat
import subprocess

import python_graph_host_receipts as receipts

# Load the prior runner by its exact source path: unittest discovery also has a
# test module with that basename. Never resolve a test module as campaign code.
_spec = importlib.util.spec_from_file_location('graph_host_prior_runner', Path(__file__).with_name('test_graph_coordinator_native.py'))
previous = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(previous)

common = previous.common
require = common.require
source_identity = previous.source_identity
build = previous.build


def admit(prefix, artifacts):
    require(os.name == 'posix' and artifacts.is_absolute() and prefix.is_absolute()
            and common.canonical_uuid(artifacts.name), 'host-explicit-uuid-directory')
    require(artifacts.resolve(strict=True) == artifacts and prefix.resolve(strict=True) == prefix,
            'host-linked-resource-ancestry')
    require(not prefix.is_relative_to(artifacts) and not artifacts.is_relative_to(prefix), 'host-original-resource-overlap')
    for path, expected in [(artifacts, {'resources'}), (artifacts/'resources', {'engines'}),
                           (artifacts/'resources/engines', {'python'})]:
        metadata = path.lstat()
        require(stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == os.getuid()
                and metadata.st_mode & 0o077 == 0, 'host-private-resource-ancestors')
        require(set(p.name for p in path.iterdir()) == expected, 'host-artifact-adoption-refused')
    python = artifacts/'resources/engines/python'
    require(python.is_dir() and not python.is_symlink(), 'host-prestaged-prefix-required')
    return python


def retain_binary(binary, artifacts):
    target = artifacts/'native-test-binary'
    with binary.open('rb') as source, target.open('xb') as destination:
        os.fchmod(destination.fileno(), 0o700)
        copied = 0
        while chunk := source.read(64*1024):
            copied += len(chunk)
            require(copied <= 64*1024**2, 'host-test-binary-bound')
            destination.write(chunk)
        destination.flush()
        os.fsync(destination.fileno())
    require(common.digest(binary) == common.digest(target), 'host-test-binary-copy')
    return target


def run(prefix, artifacts):
    # Refuse any previous receipt/workspace before writing anything to this directory.
    installed = admit(prefix, artifacts)
    report = {'schema_version': 1, 'campaign_id': artifacts.name, 'passed': False, 'complete_release': False,
              'candidate_started': False, 'phase': 'initial', 'termination': 'not_started', 'failure': None,
              'observed_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'limits': {'offline_build_seconds': 600, 'native_outer_seconds': 300,
                         'coordinator_wait_seconds': 180, 'child_wall_seconds': 30, 'child_cpu_seconds': 30},
              'unverified': ['packaged-Tauri-UI', 'normal-startup-activation', 'signed-helper',
                             'unsupported-platforms', 'preparation-inclusive-deadline', 'release-readiness']}
    common.save(artifacts, report)
    try:
        report['phase'] = 'source'; report['source'] = source_identity()
        require(platform.system() == 'Darwin' and platform.machine() == 'arm64', 'host-native-platform-unsupported')
        report['host'] = {'system': platform.system(), 'version': platform.mac_ver()[0], 'architecture': platform.machine()}
        report['phase'] = 'runtime-before'; common.save(artifacts, report)
        report['original_inventory'] = common.installed.verify(prefix, common.MANIFEST)
        report['resource_inventory'] = common.installed.verify(installed, common.MANIFEST)
        require(report['original_inventory'].get('verified') is True and report['resource_inventory'].get('verified') is True,
                'host-runtime-unverified')
        report['phase'] = 'compile'; common.save(artifacts, report)
        built = build(artifacts); binary = retain_binary(built, artifacts)
        report['binary_sha256'] = common.digest(binary)
        require(source_identity() == report['source'], 'host-source-changed-during-build')
        report.update(phase='native', candidate_started=True, termination='unconfirmed')
        common.save(artifacts, report)
        environment = dict(os.environ, WORKBENCH_TEST_GRAPH_HOST_ARTIFACTS=str(artifacts))
        completed = common.run_logged([str(binary), '--ignored', '--exact', receipts.TEST, '--test-threads=1', '--nocapture'],
                                     artifacts/'native-test.log', 300, environment)
        # Only the bounded parent-written receipt is read until confirmed stop/cleanup.
        value = common.read_json(artifacts/'receipt.json', 2*1024**2)
        report['receipt'] = value
        require(receipts.summary(value, report['campaign_id']), 'host-termination-or-cleanup-unconfirmed')
        report['termination'] = 'confirmed'
        require(completed.returncode == 0 and value['passed'] is True, 'host-native-case-failed')
        receipts.accept(value, report['campaign_id'], artifacts)
        report['receipt_sha256'] = common.digest(artifacts/'receipt.json')
        report['phase'] = 'runtime-after'
        report['post_original_inventory'] = common.installed.verify(prefix, common.MANIFEST)
        report['post_resource_inventory'] = common.installed.verify(installed, common.MANIFEST)
        require(report['post_original_inventory'].get('verified') is True and report['post_resource_inventory'].get('verified') is True,
                'host-post-runtime-unverified')
        require(source_identity() == report['source'] and common.digest(binary) == report['binary_sha256'], 'host-source-or-binary-changed')
        report.update(passed=True, phase='complete')
    except common.ProbeFailure as error:
        report['failure'] = str(error)
    except subprocess.TimeoutExpired:
        report['failure'] = 'host-native-termination-unverified' if report['candidate_started'] else 'trusted-build-timeout'
        if report['candidate_started']: report['termination'] = 'unverified'
        # No receipt/output/canonical/runtime traversal, cleanup or automatic retry.
    except (OSError, ValueError, TypeError, KeyError, IndexError, subprocess.SubprocessError):
        report['failure'] = 'host-campaign-input-or-receipt-unavailable'
    finally:
        common.save(artifacts, report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prefix', type=Path, required=True, help='Explicit true original reviewed prefix')
    parser.add_argument('--artifacts', type=Path, required=True, help='Fresh UUID directory containing only resources/engines/python')
    parser.add_argument('--execute-reviewed-campaign', action='store_true')
    args = parser.parse_args()
    if not args.execute_reviewed_campaign: parser.error('Explicit reviewed invocation required')
    try:
        report = run(args.prefix, args.artifacts)
    except (common.ProbeFailure, OSError, ValueError):
        # Do not overwrite/adopt an unadmitted directory even to retain failure.
        print(json.dumps({'passed': False, 'failure': 'host-artifact-preflight-refused', 'complete_release': False}))
        return 1
    print(json.dumps({'passed': report['passed'], 'failure': report['failure'], 'complete_release': False}))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
