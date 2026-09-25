"""Closed single-engine receipt assertions; no candidate import or execution."""
import json
import test_python_compatibility as common

RECIPES = {'networkx': 'python-networkx-v1', 'transactions': 'python-transactions-v1'}
TESTS = {case: 'engines::supervision::python_probe::engine_recipes::native_python_' + case for case in RECIPES}
FIELDS = {'networkx': ('versions', 'graph_path', 'graph_assertions', 'graph_unreachable'),
          'transactions': ('versions', 'transaction_totals', 'transfer_rejected')}
IMPORTS = {'networkx': ['networkx'], 'transactions': ['duckdb', 'pyarrow', 'pyarrow.compute', 'pyarrow.parquet']}
BASE_ASSIGNED = {'code/bootstrap.py': 'workers/python/probe/bootstrap.py',
                 'code/compatibility.py': 'workers/python/probe/compatibility.py',
                 'code/engine_recipes.py': 'workers/python/probe/engine_recipes.py',
                 'input/fixture.json': 'workers/python/probe/fixture.json',
                 'input/expected.json': 'workers/python/probe/expected.json',
                 'input/assignment.json': None}


def assigned(case):
    common.require(case in RECIPES, 'unknown-engine-case')
    return dict(BASE_ASSIGNED, **({'code/transaction_totals.py': 'workers/python/transaction_totals.py'}
                                if case == 'transactions' else {}))


def expected(case):
    common.require(case in RECIPES, 'unknown-engine-case')
    original = common.read_json(common.ROOT / 'workers/python/probe/expected.json', 64 * 1024)
    return {**{key: original[key] for key in FIELDS[case]}, 'imported_modules': IMPORTS[case]}


def summary(native, campaign, case):
    return common.failure_summary(native, campaign, recipe=RECIPES[case], assigned_names=assigned(case))


def accept(native, campaign, interpreter, case):
    summary(native, campaign, case)
    common.require(native['passed'] is True and native['phase'] == 'complete' and native['runtime_verified'] is True
        and native['diagnostics_within_bound'] is True and native['last_worker_checkpoint'] == 'complete'
        and type(native['exit_code']) is int and native['exit_code'] == 0 and native['failure'] is None
        and native['termination_state'] == 'confirmed' and common.sha256(native['profile_sha256'])
        and native['candidate_interpreter'] == interpreter and set(native['assigned_files']) == set(assigned(case))
        and native['last_import_checkpoint'] is None and native['import_diagnostics'] is None
        and native['engine_diagnostics'] is not None and native['engine_diagnostics']['valid'] is True
        and len(native['engine_diagnostics']['checkpoints']) == 4 and native['quota_kind'] is None
        and native['preparation_elapsed_ms'] is not None and native['supervised_elapsed_ms'] is not None,
        'native-engine-assertions-failed')
    for name, source in assigned(case).items():
        if source:
            path = common.ROOT / source
            common.require(native['assigned_files'][name] == {'bytes': path.stat().st_size, 'sha256': common.digest(path)},
                           'native-engine-assignment-mismatch')
    result = native['result']
    common.require(isinstance(result, dict) and set(result) == {'schema_version', 'recipe', 'job_id', 'manifest_sha256',
        'python_version', 'isolated', 'no_site', 'no_bytecode', 'verified_paths', 'checks'}, 'native-engine-result-shape')
    common.require(type(result['schema_version']) is int and result['schema_version'] == 1
        and result['recipe'] == RECIPES[case] and result['manifest_sha256'] == common.MANIFEST
        and result['python_version'] == '3.13.15' and result['job_id'] == native['job_id']
        and all(result[field] is True for field in ('isolated', 'no_site', 'no_bytecode', 'verified_paths')),
        'native-engine-result-identity')
    canonical = lambda value: json.dumps(value, sort_keys=True, allow_nan=False, separators=(',', ':'))
    common.require(canonical(result['checks']) == canonical(expected(case)), 'native-engine-result-assertions')
