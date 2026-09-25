"""Closed test-only operation for a real Rust-owned graph capture; no app entry."""
import hashlib
import importlib
import importlib.metadata
import os
from pathlib import Path
import sys
import uuid

PHASES = ('bootstrap', 'versions', 'metadata', 'imports', 'init-ready', 'operation', 'complete')
RECIPE = 'python-canonical-graph-v1'
LOOPBACK = ('nx_loopback', 'networkx.classes.tests.dispatch_interface:backend_interface')


def require(condition):
    if not condition:
        raise ValueError('fixed-canonical-graph-contract')


def identity(data):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def canonical_uuid(value):
    require(type(value) is str and len(value) == 36)
    parsed = uuid.UUID(value)
    require(str(parsed) == value and parsed.version == 4 and parsed.variant == uuid.RFC_4122)


def preimport_backends():
    # Never call EntryPoint.load(). NetworkX would execute backend_info on import.
    require('networkx' not in sys.modules)
    require(not any(key.startswith('NETWORKX_') for key in os.environ))
    require(list(importlib.metadata.entry_points(group='networkx.backend_info')) == [])
    backends = list(importlib.metadata.entry_points(group='networkx.backends'))
    require(len(backends) == 1)
    entry = backends[0]
    require((entry.name, entry.value) == LOOPBACK and entry.dist is not None
            and entry.dist.metadata['Name'].lower() == 'networkx' and entry.dist.version == '3.6.1')


def execute(assignment, fixture, prefix, job, checkpoint):
    from compatibility import distribution_versions
    from engine_recipes import Timings
    import graph_path
    require(Path(graph_path.__file__).resolve() == job / 'code/graph_path.py')
    require(set(assignment) == {'schema_version', 'job_id', 'prefix', 'manifest_sha256',
                                'campaign_id', 'capture_nonce', 'request_identity'})
    require(type(assignment['schema_version']) is int and assignment['schema_version'] == 1)
    for field in ('job_id', 'campaign_id', 'capture_nonce'):
        canonical_uuid(assignment[field])
    timings = Timings(job / 'scratch')
    timings.checkpoint('initialization', 'before')
    with (job / 'input/graph-request.json').open('rb') as stream:
        raw = stream.read(64 * 1024 + 1)
    require(len(raw) <= 64 * 1024)
    request = graph_path.parse_request(raw)  # Full request validation before any NetworkX import.
    require(request['nonce'] == assignment['capture_nonce']
            and request['runtime_manifest_sha256'] == assignment['manifest_sha256'])
    expected_identity = assignment['request_identity']
    require(type(expected_identity) is dict and set(expected_identity) == {'bytes', 'sha256'}
            and type(expected_identity['bytes']) is int and type(expected_identity['sha256']) is str
            and expected_identity == identity(raw))
    require(fixture['schema_version'] == 1 and len(fixture['versions']) == 58)
    site = prefix / 'install/lib/python3.13/site-packages'
    checkpoint('versions')
    versions, _ = distribution_versions(site, fixture['versions'])
    checkpoint('metadata')
    preimport_backends()
    checkpoint('imports')
    nx = importlib.import_module('networkx')
    require(nx.__version__ == '3.6.1' and Path(nx.__file__).resolve().is_relative_to(site))
    require(nx.utils.backends.backends == {} and nx.utils.backends._loaded_backends == {}
            and set(nx.utils.backends.backend_info) == {'networkx'})
    timings.checkpoint('initialization', 'ready')
    checkpoint('init-ready')
    timings.checkpoint('operation', 'before')
    checkpoint('operation')
    result = graph_path.execute(raw)
    require(0 < len(result) <= graph_path.MAX_RESULT_BYTES)
    with (job / 'scratch/graph-result.json').open('xb') as stream:
        require(stream.write(result) == len(result))
        stream.flush()
    # Worker operation/serialization completed. Rust validation occurs only after termination.
    timings.checkpoint('operation', 'after')
    return {'versions': versions, 'imported_modules': ['networkx'], 'backend_metadata_checked': True,
            'campaign_id': assignment['campaign_id'], 'capture_nonce': assignment['capture_nonce'],
            'request_identity': identity(raw), 'result_identity': identity(result)}
