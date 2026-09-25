#!/usr/bin/env python3
"""Explicit reviewed four-case native coordinator campaign. Never normal runtime discovery."""
import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import stat
import subprocess
import uuid

import test_python_compatibility as common
import relocate_python_prefix as relocation
import python_graph_coordinator_receipts as receipts

ROOT = Path(__file__).resolve().parents[1]
require = common.require


def source_identity():
    def git(*args):
        return subprocess.check_output(['git', *args], cwd=ROOT, timeout=10)
    require(not git('status', '--porcelain').strip(), 'source-is-not-clean')
    names = git('ls-files', '-z').decode().strip('\0').split('\0')
    require(len(names) <= 5000 and all(name and not (ROOT/name).is_symlink() for name in names), 'source-file-set')
    # Bind the complete tracked source tree, including test-only helpers, fixed
    # Python assets, Cargo locks/manifests and the runner/validator themselves.
    return {'commit':git('rev-parse', 'HEAD').decode().strip(),
            'tree':git('rev-parse', 'HEAD^{tree}').decode().strip(),
            'files':{name:common.digest(ROOT/name) for name in names}}


def build(artifacts):
    log = artifacts/'build.log'
    completed = common.run_logged(['cargo','test','--offline','--locked','--release','-p','workbench-core',
        '--lib','--no-run','--message-format=json'], log, 600)
    require(completed.returncode == 0, 'source-build-failed')
    binaries = set()
    for line in log.read_text().splitlines():
        require(len(line) <= 4*1024**2, 'compiler-message-bound')
        if not line.startswith('{'): continue
        item = json.loads(line)
        if item.get('reason') == 'compiler-artifact' and item.get('target', {}).get('name') == 'workbench_core' and item.get('executable'):
            profile = item['profile']
            require(profile['test'] is True and profile['opt_level'] == '3' and profile['debug_assertions'] is False,
                    'native-build-profile')
            binaries.add(Path(item['executable']))
    require(len(binaries) == 1, 'native-test-binary-ambiguous')
    binary = binaries.pop()
    metadata = binary.lstat()
    require(stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1, 'native-test-binary-invalid')
    return binary


def run(prefix, artifacts):
    report = {'schema_version':1, 'campaign_id':str(uuid.uuid4()), 'passed':False, 'complete_release':False,
              'candidate_started':False, 'phase':'initial', 'termination':'not_started', 'failure':None,
              'observed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(), 'cases':[],
              'limits':{'offline_build_seconds':600,'native_outer_seconds':900,'coordinator_wait_seconds':180,
                        'child_wall_seconds':30,'child_cpu_seconds':30,'live_handshake_seconds':5},
              'unverified':['normal-app-activation','signed-helper','Windows-and-Linux-confinement',
                            'hard-RSS-ceiling','NetworkX-computation-phase-cancellation','release-readiness']}
    common.save(artifacts, report)
    try:
        report['phase'] = 'source'; report['source'] = source_identity()
        require(platform.system() == 'Darwin' and platform.machine() == 'arm64', 'native-platform-unsupported')
        report['host'] = {'system':platform.system(),'version':platform.mac_ver()[0],'architecture':platform.machine()}
        report['phase'] = 'runtime-before'; common.save(artifacts, report)
        report['original_inventory'] = common.installed.verify(prefix, common.MANIFEST)
        require(report['original_inventory'].get('verified') is True, 'original-runtime-unverified')
        engines = artifacts/'app-engines'; engines.mkdir(mode=0o700)
        report['relocation'] = relocation.relocate(prefix, engines/'python')
        require(report['relocation'].get('copied') is True, 'app-local-copy-unverified')
        report['phase'] = 'compile'; common.save(artifacts, report)
        binary = build(artifacts); report['binary_sha256'] = common.digest(binary)
        require(source_identity() == report['source'], 'source-changed-during-build')
        report.update(phase='native',candidate_started=True,termination='unconfirmed')
        common.save(artifacts, report)
        environment = dict(os.environ, WORKBENCH_TEST_GRAPH_ARTIFACTS=str(artifacts),
            WORKBENCH_TEST_GRAPH_ENGINES=str(engines), WORKBENCH_TEST_GRAPH_CAMPAIGN=report['campaign_id'])
        completed = common.run_logged([str(binary),'--ignored','--exact',receipts.TEST,'--test-threads=1','--nocapture'],
            artifacts/'native-test.log',900,environment)
        # Parent-written bounded receipts only, before any worker output/canonical/runtime read.
        for case in receipts.CASES:
            path = artifacts/case/'receipt.json'
            value = common.read_json(path, 2*1024**2)
            safe = receipts.summary(value, report['campaign_id'], case)
            report['cases'].append({'case':case,'receipt_sha256':common.digest(path),'receipt':value})
            require(safe, 'native-termination-or-cleanup-unconfirmed')
            require(value['passed'] is True, 'native-case-failed')
        report['termination'] = 'confirmed'
        end = common.read_json(artifacts/'native-complete.json', 4096)
        require(end == {'schema_version':1,'campaign_id':report['campaign_id'],'passed':True,'complete_release':False}
                and completed.returncode == 0, 'native-campaign-incomplete')
        for entry in report['cases']:
            receipts.accept(entry['receipt'], report['campaign_id'], entry['case'], artifacts/entry['case'])
        report['phase'] = 'runtime-after'
        report['post_original_inventory'] = common.installed.verify(prefix, common.MANIFEST)
        report['post_app_inventory'] = common.installed.verify(engines/'python', common.MANIFEST)
        require(report['post_original_inventory'].get('verified') is True
                and report['post_app_inventory'].get('verified') is True, 'post-runtime-unverified')
        require(source_identity() == report['source'] and common.digest(binary) == report['binary_sha256'], 'source-or-binary-changed')
        report.update(passed=True, phase='complete')
    except common.ProbeFailure as error:
        report['failure'] = str(error)
    except subprocess.TimeoutExpired:
        report['failure'] = 'native-termination-unverified' if report['candidate_started'] else 'trusted-build-timeout'
        if report['candidate_started']: report['termination'] = 'unverified'
        # No child/canonical/runtime reads, no cleanup and no retry after outer timeout.
    except (OSError,ValueError,TypeError,KeyError,subprocess.SubprocessError):
        report['failure'] = 'campaign-input-or-receipt-unavailable'
    finally:
        common.save(artifacts, report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prefix',type=Path,required=True)
    parser.add_argument('--artifacts',type=Path,required=True)
    parser.add_argument('--execute-reviewed-campaign',action='store_true')
    args = parser.parse_args()
    if not args.execute_reviewed_campaign: parser.error('Explicit reviewed candidate invocation is required')
    common.prepare_artifacts(args.prefix,args.artifacts)
    report = run(args.prefix.resolve(strict=True),args.artifacts.resolve(strict=True))
    print(json.dumps({'passed':report['passed'],'failure':report['failure'],'complete_release':False}))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
