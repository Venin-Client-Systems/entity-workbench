"""Closed app-owned graph recipe. No campaign, fixture, plugins or recipe selector."""
import hashlib
import importlib
import importlib.util
from pathlib import Path
import sys
import uuid

RECIPE = 'python-graph-job-v1'
MANIFEST = '4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822'
MAX_REQUEST = 1024 * 1024


def support():
    name = 'runtime_support'
    if name not in sys.modules:
        spec = importlib.util.spec_from_file_location(name, Path(__file__).parent / 'runtime_support.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        sys.modules[name] = module
    return sys.modules[name]


def identity(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def canonical_uuid(value):
    check = support().require
    check(type(value) is str and len(value) == 36)
    parsed = uuid.UUID(value)
    check(str(parsed) == value and parsed.version == 4 and parsed.variant == uuid.RFC_4122)


def execute(assignment, prefix, job):
    runtime = support()
    check = runtime.require
    check(type(assignment) is dict and set(assignment) == {
        'schema_version', 'recipe', 'job_id', 'prefix', 'manifest_sha256',
        'capture_nonce', 'request_identity'})
    check(type(assignment['schema_version']) is int and assignment['schema_version'] == 1)
    check(assignment['recipe'] == RECIPE and assignment['manifest_sha256'] == MANIFEST)
    canonical_uuid(assignment['job_id'])
    canonical_uuid(assignment['capture_nonce'])
    with (job / 'input/graph-request.json').open('rb') as stream:
        raw = stream.read(MAX_REQUEST + 1)
    check(0 < len(raw) <= MAX_REQUEST)
    expected = assignment['request_identity']
    check(type(expected) is dict and set(expected) == {'bytes', 'sha256'}
          and type(expected['bytes']) is int and type(expected['sha256']) is str
          and expected == identity(raw))
    import graph_path
    check(Path(graph_path.__file__).resolve() == job / 'code/graph_path.py')
    request = graph_path.parse_request(raw)
    check(request['nonce'] == assignment['capture_nonce']
          and request['runtime_manifest_sha256'] == MANIFEST)
    site = prefix / 'install/lib/python3.13/site-packages'
    wanted = runtime.read_json(job / 'input/runtime-versions.json')
    check(type(wanted) is dict and len(wanted) == 58)
    versions, _ = runtime.distribution_versions(site, wanted)
    runtime.preimport_backends()
    nx = importlib.import_module('networkx')
    check(nx.__version__ == '3.6.1' and Path(nx.__file__).resolve().is_relative_to(site))
    check(nx.utils.backends.backends == {} and nx.utils.backends._loaded_backends == {}
          and set(nx.utils.backends.backend_info) == {'networkx'})
    result = graph_path.execute(raw)
    check(0 < len(result) <= graph_path.MAX_RESULT_BYTES)
    with (job / 'scratch/graph-result.json').open('xb') as stream:
        check(stream.write(result) == len(result))
        stream.flush()
    return {
        'versions': versions, 'imported_modules': ['networkx'], 'backend_metadata_checked': True,
        'capture_nonce': assignment['capture_nonce'], 'request_identity': identity(raw),
        'result_identity': identity(result),
    }


def main():
    runtime = support()
    check = runtime.require
    check(len(sys.argv) == 1)
    job = Path(__file__).resolve().parent.parent
    try:
        assignment = runtime.read_json(job / 'input/assignment.json')
        prefix = Path(assignment['prefix'])
        check(prefix.is_absolute() and prefix.resolve() == prefix)
        check(sys.flags.isolated == 1 and sys.flags.no_site == 1 and sys.dont_write_bytecode is True)
        check(Path(sys.executable) == prefix / 'install/bin/python3.13')
        check(Path(sys.prefix) == prefix / 'install' and Path(sys.base_prefix) == prefix / 'install')
        sys.path[:] = runtime.bootstrap_paths(prefix, job / 'code', list(sys.path))
        checks = execute(assignment, prefix, job)
        runtime.write_json(job / 'scratch/result.json', {
            'schema_version': 1, 'recipe': RECIPE, 'job_id': assignment['job_id'],
            'manifest_sha256': MANIFEST, 'python_version': sys.version.split()[0],
            'isolated': True, 'no_site': True, 'no_bytecode': True, 'verified_paths': True,
            'checks': checks,
        })
        return 0
    except Exception:
        runtime.write_json(job / 'scratch/failure.json', {'failure': 'fixed-graph-contract-failed'}, 512)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
