"""Fixed test-only bootstrap. Never used by an application worker command."""
import json
from pathlib import Path
import sys

MAX_JSON = 64 * 1024
PHASES = ('bootstrap', 'versions', 'imports', 'mentions', 'graph', 'transactions', 'plugins', 'complete')


def require(condition):
    if not condition:
        raise ValueError('fixed-probe-contract')


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


def main(recipe='python-compatibility-v1'):
    require(recipe in ('python-compatibility-v1', 'python-networkx-v1', 'python-transactions-v1', 'python-canonical-graph-v1'))
    job = Path(__file__).resolve().parent.parent
    scratch = job / 'scratch'
    phase = 'bootstrap'
    try:
        assignment = read_json(job / 'input/assignment.json')
        assignment_fields = {'schema_version', 'job_id', 'prefix', 'manifest_sha256'}
        if recipe == 'python-canonical-graph-v1':
            assignment_fields |= {'campaign_id', 'capture_nonce', 'request_identity'}
        require(set(assignment) == assignment_fields)
        require(assignment['schema_version'] == 1)
        prefix = Path(assignment['prefix'])
        require(prefix.is_absolute() and prefix.resolve() == prefix)
        require(sys.flags.isolated == 1 and sys.flags.no_site == 1 and sys.dont_write_bytecode is True)
        require(Path(sys.executable) == prefix / 'install/bin/python3.13')
        require(Path(sys.prefix) == prefix / 'install' and Path(sys.base_prefix) == prefix / 'install')
        sys.path[:] = bootstrap_paths(prefix, job / 'code', list(sys.path))
        if recipe == 'python-compatibility-v1':
            from import_diagnostics import ImportDiagnostics
            diagnostics = ImportDiagnostics(scratch)
            phases = PHASES
        elif recipe == 'python-canonical-graph-v1':
            from canonical_graph import PHASES as phases
        else:
            from engine_recipes import PHASES as phases

        def checkpoint(value):
            nonlocal phase
            require(value in phases)
            phase = value
            write_json(scratch / ('checkpoint-' + str(phases.index(value)) + '.json'), {'phase': value}, 512)
            if recipe == 'python-compatibility-v1' and value == 'imports':
                diagnostics.install()
            elif recipe == 'python-compatibility-v1' and value == 'mentions':
                diagnostics.finish()

        checkpoint('bootstrap')
        if recipe == 'python-compatibility-v1':
            from compatibility import execute
            checks = execute(read_json(job / 'input/fixture.json'), prefix, job, checkpoint, diagnostics.checkpoint)
        elif recipe == 'python-canonical-graph-v1':
            from canonical_graph import execute
            checks = execute(assignment, read_json(job / 'input/fixture.json'), prefix, job, checkpoint)
        else:
            from engine_recipes import execute
            checks = execute(recipe, read_json(job / 'input/fixture.json'), read_json(job / 'input/expected.json'),
                             prefix, job, checkpoint)
        result = {'checks': checks}
        result.update(schema_version=1, recipe=recipe, job_id=assignment['job_id'],
                      manifest_sha256=assignment['manifest_sha256'], python_version=sys.version.split()[0],
                      isolated=True, no_site=True, no_bytecode=True, verified_paths=True)
        write_json(scratch / 'result.json', result, 1024 * 1024)
        checkpoint('complete')
        return 0
    except Exception:
        # No traceback, exception text, environment or local path enters the receipt.
        write_json(scratch / 'failure.json', {'phase': phase, 'failure': 'compatibility-contract-failed'}, 512)
        return 1


if __name__ == '__main__':
    require(len(sys.argv) in (1, 2))
    raise SystemExit(main(sys.argv[1] if len(sys.argv) == 2 else 'python-compatibility-v1'))
