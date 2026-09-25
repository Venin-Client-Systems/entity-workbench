"""Closed test-only single-engine recipes; importing this module uses stdlib only."""
import importlib
import json
from pathlib import Path
import time

RECIPES = {
    'python-networkx-v1': ('networkx',),
    'python-transactions-v1': ('duckdb', 'pyarrow', 'pyarrow.compute', 'pyarrow.parquet'),
}
FIELDS = {
    'python-networkx-v1': ('versions', 'graph_path', 'graph_assertions', 'graph_unreachable'),
    'python-transactions-v1': ('versions', 'transaction_totals', 'transfer_rejected'),
}
PHASES = ('bootstrap', 'versions', 'imports', 'init-ready', 'operation', 'complete')
TIMES = (('initialization', 'before'), ('initialization', 'ready'), ('operation', 'before'), ('operation', 'after'))


def require(condition):
    if not condition:
        raise ValueError('fixed-engine-contract')


def expected_checks(recipe, expected):
    require(recipe in RECIPES)
    return {**{key: expected[key] for key in FIELDS[recipe]}, 'imported_modules': list(RECIPES[recipe])}


class Timings:
    def __init__(self, scratch):
        self.scratch = scratch
        self.started = (time.monotonic_ns(), time.process_time_ns())
        self.previous = (0, 0)
        self.count = 0

    def checkpoint(self, stage, boundary):
        require(self.count < len(TIMES) and (stage, boundary) == TIMES[self.count])
        current = tuple((now - start) // 1_000_000 for now, start in
                        zip((time.monotonic_ns(), time.process_time_ns()), self.started))
        require(all(type(now) is int and old <= now <= 120_000 for old, now in zip(self.previous, current)))
        data = json.dumps({'stage': stage, 'boundary': boundary, 'elapsed_ms': current[0],
                           'process_cpu_ms': current[1]}, allow_nan=False, sort_keys=True).encode()
        require(len(data) <= 512)
        with (self.scratch / f'engine-time-{self.count}.json').open('xb') as stream:
            stream.write(data)
        self.previous = current
        self.count += 1


def execute(recipe, fixture, expected, prefix, job, checkpoint):
    # Import reviewed adapter helpers only after bootstrap validates the path set.
    from compatibility import distribution_versions, graph_checks, transaction_checks
    require(recipe in RECIPES and fixture['schema_version'] == 1 and len(fixture['versions']) == 58)
    timings = Timings(job / 'scratch')
    timings.checkpoint('initialization', 'before')
    checkpoint('versions')
    site = prefix / 'install/lib/python3.13/site-packages'
    versions, _ = distribution_versions(site, fixture['versions'])
    checkpoint('imports')
    modules = {name: importlib.import_module(name) for name in RECIPES[recipe]}
    for module in modules.values():
        require(Path(module.__file__).resolve().is_relative_to(site))
    if recipe == 'python-transactions-v1':
        import transaction_totals  # Reviewed adapter; all its top-level imports are stdlib.
        require(Path(transaction_totals.__file__).resolve() == job / 'code/transaction_totals.py')
        modules['pyarrow'].set_cpu_count(1)
        modules['pyarrow'].set_io_thread_count(1)
    timings.checkpoint('initialization', 'ready')
    checkpoint('init-ready')
    timings.checkpoint('operation', 'before')
    checkpoint('operation')
    selected = (graph_checks(modules['networkx'], fixture) if recipe == 'python-networkx-v1'
                else transaction_checks(modules['pyarrow'], fixture, job))
    result = {'versions': versions, 'imported_modules': list(RECIPES[recipe]), **selected}
    canonical = lambda value: json.dumps(value, allow_nan=False, sort_keys=True, separators=(',', ':'))
    require(canonical(result) == canonical(expected_checks(recipe, expected)))
    # No after record or result is produced until the original exact assertions agree.
    timings.checkpoint('operation', 'after')
    return result
