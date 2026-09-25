"""Closed test-only canonical graph observations; never imports NetworkX."""
import hashlib
import json
import test_python_compatibility as common

RECIPE = 'python-canonical-graph-v1'
TEST = 'engines::supervision::python_probe::canonical_graph::native_python_canonical_graph'
SOURCES = (
    'crates/core/src/engines/supervision/python_probe/canonical_graph.rs',
    'crates/core/src/store/graph_analysis.rs', 'crates/core/src/store/graph_analysis/capture.rs',
    'crates/core/src/store/graph_analysis/model.rs', 'crates/core/src/store/graph_analysis/probe_fixture.rs',
    'workers/python/graph_path.py', 'workers/python/probe/canonical_graph.py',
    'workers/python/fixtures/canonical-graph-cases.v1.json', 'scripts/python_canonical_graph_receipts.py',
)
ASSIGNED = {
    'code/bootstrap.py': 'workers/python/probe/bootstrap.py',
    'code/runtime_support.py': 'workers/python/runtime_support.py',
    'code/compatibility.py': 'workers/python/probe/compatibility.py',
    'code/engine_recipes.py': 'workers/python/probe/engine_recipes.py',
    'code/canonical_graph.py': 'workers/python/probe/canonical_graph.py',
    'code/graph_path.py': 'workers/python/graph_path.py',
    'input/fixture.json': 'workers/python/probe/fixture.json',
    'input/graph-request.json': None, 'input/assignment.json': None,
}
EXTRA = {'canonical_capture', 'graph_output_identity', 'wrapper_output_identity'}
LIMITATION = ('Undirected connectivity across accepted assertions from all retained time periods. '
              'A path may combine disjoint historical periods and does not establish a contemporaneous, '
              'directed or causal relationship.')
require = common.require
canonical = lambda value: json.dumps(value, allow_nan=False, sort_keys=True, separators=(',', ':'))


def capture_metadata(value):
    require(type(value) is dict and set(value) == {'nonce', 'workspace_revision', 'snapshot_sha256',
        'request_identity', 'source_id', 'target_id', 'canonical_state_sha256'}, 'canonical-capture-shape')
    require(common.canonical_uuid(value['nonce']) and type(value['workspace_revision']) is int
            and value['workspace_revision'] == 2 and common.sha256(value['snapshot_sha256'])
            and common.sha256(value['canonical_state_sha256']) and value['source_id'] == 'a'
            and value['target_id'] == 'c' and common.asset_identity(value['request_identity'], 64*1024),
            'canonical-capture-identity')


def summary(native, campaign):
    require(type(native) is dict and set(native) == common.NATIVE_KEYS | {'engine_diagnostics'} | EXTRA,
            'canonical-observation-shape')
    projected = {key: value for key, value in native.items() if key not in EXTRA}
    result = common.failure_summary(projected, campaign, recipe=RECIPE, assigned_names=ASSIGNED)
    if native['canonical_capture'] is not None:
        capture_metadata(native['canonical_capture'])
    require(native['graph_output_identity'] is None or common.asset_identity(native['graph_output_identity'], 128*1024),
            'canonical-output-identity')
    require(native['wrapper_output_identity'] is None or common.asset_identity(native['wrapper_output_identity'], 64*1024), 'canonical-wrapper-identity')
    result.update({key: native[key] for key in EXTRA})
    return result


def accept(native, campaign, interpreter, artifacts):
    summary(native, campaign)
    require(native['passed'] is True and native['phase'] == 'complete' and native['runtime_verified'] is True
        and native['diagnostics_within_bound'] is True and native['last_worker_checkpoint'] == 'complete'
        and type(native['exit_code']) is int and native['exit_code'] == 0 and native['failure'] is None
        and native['termination_state'] == 'confirmed' and common.sha256(native['profile_sha256'])
        and native['candidate_interpreter'] == interpreter and set(native['assigned_files']) == set(ASSIGNED)
        and native['last_import_checkpoint'] is None and native['import_diagnostics'] is None
        and native['engine_diagnostics'] is not None and native['engine_diagnostics']['valid'] is True
        and len(native['engine_diagnostics']['checkpoints']) == 4 and native['quota_kind'] is None
        and native['preparation_elapsed_ms'] is not None and native['supervised_elapsed_ms'] is not None,
        'native-canonical-assertions-failed')
    capture_metadata(native['canonical_capture'])
    capture = native['canonical_capture']
    require(common.asset_identity(native['graph_output_identity'], 128*1024), 'canonical-output-missing')
    require(common.asset_identity(native['wrapper_output_identity'], 64*1024), 'canonical-wrapper-missing')
    for name, source in ASSIGNED.items():
        if source:
            data = (common.ROOT/source).read_bytes()
            require(native['assigned_files'][name] == {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()},
                    'canonical-assigned-code-mismatch')
    for name, expected, maximum in [('captured-request.json', capture['request_identity'], 64*1024),
                                    ('captured-result.json', native['graph_output_identity'], 128*1024),
                                    ('captured-wrapper.json', native['wrapper_output_identity'], 64*1024)]:
        value = common.read_json(artifacts/name, maximum)
        require((artifacts/name).stat().st_size == expected['bytes'] and common.digest(artifacts/name) == expected['sha256'],
                'canonical-retained-file-mismatch')
        if name == 'captured-request.json': request = value
        elif name == 'captured-result.json': response = value
        else: wrapper = value
    expected_request = common.read_json(common.ROOT/'workers/python/fixtures/canonical-graph-cases.v1.json', 64*1024)['cases'][0]['request']
    expected_request.update(nonce=capture['nonce'], workspace_revision=capture['workspace_revision'],
                            snapshot_sha256=capture['snapshot_sha256'])
    require(canonical(request) == canonical(expected_request)
            and native['assigned_files']['input/graph-request.json'] == capture['request_identity'], 'canonical-request-binding')
    expected_response = {key: value for key, value in request.items() if key not in ('nodes', 'edges', 'source_id', 'target_id')}
    expected_response['outcome'] = {'state': 'path', 'nodes': ['a', 'b', 'c']}
    require(canonical(response) == canonical(expected_response), 'canonical-response-binding')
    result = native['result']
    require(type(result) is dict and set(result) == {'worker', 'canonical'}, 'canonical-result-shape')
    versions = common.read_json(common.ROOT/'workers/python/probe/fixture.json', 64*1024)['versions']
    checks = {'versions': versions, 'imported_modules': ['networkx'], 'backend_metadata_checked': True,
              'campaign_id': campaign, 'capture_nonce': capture['nonce'],
              'request_identity': capture['request_identity'], 'result_identity': native['graph_output_identity']}
    expected_worker = {'schema_version': 1, 'recipe': RECIPE, 'job_id': native['job_id'],
        'manifest_sha256': common.MANIFEST, 'python_version': '3.13.15', 'isolated': True,
        'no_site': True, 'no_bytecode': True, 'verified_paths': True, 'checks': checks}
    expected_canonical = {'validated': True, 'canonical_unchanged': True,
        'workspace_revision': capture['workspace_revision'], 'snapshot_sha256': capture['snapshot_sha256'],
        'nodes': ['a', 'b', 'c'], 'hop_assertion_ids': [['r1', 'r1-parallel'], ['r2']], 'limitation': LIMITATION}
    require(canonical(wrapper) == canonical(expected_worker)
            and canonical(result) == canonical({'worker': expected_worker, 'canonical': expected_canonical}),
            'canonical-owned-validation-binding')
