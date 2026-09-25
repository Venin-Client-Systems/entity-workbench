"""Pinned Python runtime checks shared by fixed application and probe entries.

No engine is imported and no entry point is loaded by this module.
"""
import importlib.metadata
import json
import os
from pathlib import Path
import re
import sys

MAX_JSON = 64 * 1024
LOOPBACK = ('nx_loopback', 'networkx.classes.tests.dispatch_interface:backend_interface')


def require(condition):
    if not condition:
        raise ValueError('fixed-runtime-contract')


def unique(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result)
        result[key] = value
    return result


def read_json(path):
    with path.open('rb') as stream:
        data = stream.read(MAX_JSON + 1)
    require(len(data) <= MAX_JSON)
    return json.loads(data, object_pairs_hook=unique)


def write_json(path, value, maximum=MAX_JSON):
    data = json.dumps(value, allow_nan=False, sort_keys=True).encode()
    require(len(data) <= maximum)
    with path.open('xb') as stream:
        stream.write(data)


def bootstrap_paths(prefix, code, initial):
    stdlib = prefix / 'install/lib/python3.13'
    expected = {prefix / 'install/lib/python313.zip', stdlib, stdlib / 'lib-dynload'}
    actual = [Path(value) for value in initial]
    require(actual and len(actual) == len(set(actual)) and set(actual) == expected)
    require(all(value.is_absolute() for value in actual))
    return [*initial, str(prefix / 'install/lib/python3.13/site-packages'), str(code)]


def distribution_versions(site, wanted):
    observed = {}
    distributions = list(importlib.metadata.distributions(path=[str(site)]))
    require(len(distributions) <= 128)
    for distribution in distributions:
        name = re.sub(r'[-_.]+', '-', distribution.metadata['Name']).lower()
        if name in wanted:
            require(name not in observed)
            observed[name] = distribution.version
    require(observed == wanted)
    return observed, distributions


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
