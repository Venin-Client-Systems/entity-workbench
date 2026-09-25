"""Fixed app source regressions using synthetic stubs; never invokes the candidate."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
import uuid

ROOT = Path(__file__).resolve().parents[2]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


runtime = load('app_runtime_support', 'workers/python/runtime_support.py')
worker = load('app_graph_worker', 'workers/python/graph_worker.py')
graph = load('app_graph_path', 'workers/python/graph_path.py')


class GraphWorkerTests(unittest.TestCase):
    def execute(self, *, padding=0, mutation=None, malformed=None, metadata_error=False,
                existing=False, bad_result=False):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        job = Path(temp.name).resolve()
        for name in ('input', 'code', 'scratch'):
            (job / name).mkdir()
        prefix = job / 'prefix'
        site = prefix / 'install/lib/python3.13/site-packages'
        value = json.loads((ROOT / 'workers/python/fixtures/canonical-graph-cases.v1.json').read_bytes())['cases'][0]['request']
        value['nonce'] = str(uuid.uuid4())
        raw = json.dumps(value).encode() + b' ' * padding
        if malformed is not None:
            raw = malformed
        (job / 'input/graph-request.json').write_bytes(raw)
        versions = json.loads((ROOT / 'workers/python/runtime_versions.json').read_bytes())
        (job / 'input/runtime-versions.json').write_text(json.dumps(versions))
        assignment = dict(schema_version=1, recipe=worker.RECIPE, job_id=str(uuid.uuid4()),
            prefix=str(prefix), manifest_sha256=worker.MANIFEST, capture_nonce=value['nonce'],
            request_identity=worker.identity(raw))
        if mutation:
            mutation(assignment)
        expected = b'{"exact":"raw bytes"}\n'
        adapter = SimpleNamespace(__file__=str(job / 'code/graph_path.py'),
            parse_request=graph.parse_request, MAX_RESULT_BYTES=graph.MAX_RESULT_BYTES,
            execute=Mock(return_value=b'x' * (graph.MAX_RESULT_BYTES + 1) if bad_result else expected))
        nx = SimpleNamespace(__version__='3.6.1', __file__=str(site / 'networkx/__init__.py'),
            utils=SimpleNamespace(backends=SimpleNamespace(backends={}, _loaded_backends={}, backend_info={'networkx': {}})))
        if existing:
            (job / 'scratch/graph-result.json').write_bytes(b'preserve')
        with patch.dict(sys.modules, {'runtime_support': runtime, 'graph_path': adapter}), \
             patch.object(runtime, 'distribution_versions', return_value=(versions, [])) as distributions, \
             patch.object(runtime, 'preimport_backends', side_effect=ValueError('fixed') if metadata_error else None) as guard, \
             patch.object(worker.importlib, 'import_module', return_value=nx) as imported:
            try:
                result = worker.execute(assignment, prefix, job)
            except Exception as error:
                result = error
        return result, job, imported, guard, distributions, adapter

    def test_fixed_app_accepts_request_over_legacy_probe_bound_and_preserves_raw_bytes(self):
        result, job, imported, guard, versions, adapter = self.execute(padding=65536)
        self.assertIsInstance(result, dict)
        raw = (job / 'scratch/graph-result.json').read_bytes()
        self.assertEqual(raw, b'{"exact":"raw bytes"}\n')
        self.assertEqual(result['result_identity'], worker.identity(raw))
        self.assertGreater(result['request_identity']['bytes'], 65536)
        self.assertNotIn('campaign_id', result)
        imported.assert_called_once_with('networkx')
        guard.assert_called_once()
        versions.assert_called_once()
        adapter.execute.assert_called_once()

    def test_invalid_assignment_or_request_never_imports_networkx(self):
        mutations = [lambda a: a.update(campaign_id=str(uuid.uuid4())),
            lambda a: a.update(recipe='python-canonical-graph-v1'),
            lambda a: a.update(schema_version=True),
            lambda a: a.update(capture_nonce=str(uuid.uuid4())),
            lambda a: a.update(job_id=uuid.uuid4().hex),
            lambda a: a['request_identity'].update(bytes=True),
            lambda a: a['request_identity'].update(sha256='0' * 64)]
        for mutate in mutations:
            result, job, imported, guard, versions, adapter = self.execute(mutation=mutate)
            self.assertIsInstance(result, ValueError)
            imported.assert_not_called()
            guard.assert_not_called()
            versions.assert_not_called()
            adapter.execute.assert_not_called()
            self.assertFalse((job / 'scratch/graph-result.json').exists())
        for raw in (b'{}', b' ' * (1024 * 1024 + 1), b'{"nonce":"a","nonce":"b"}'):
            result, _, imported, guard, _, _ = self.execute(malformed=raw)
            self.assertIsInstance(result, ValueError)
            imported.assert_not_called()
            guard.assert_not_called()

    def test_metadata_denial_occurs_before_import_and_output_never_clobbers(self):
        result, job, imported, _, _, adapter = self.execute(metadata_error=True)
        self.assertIsInstance(result, ValueError)
        imported.assert_not_called()
        adapter.execute.assert_not_called()
        self.assertFalse((job / 'scratch/graph-result.json').exists())
        result, job, _, _, _, _ = self.execute(existing=True)
        self.assertIsInstance(result, FileExistsError)
        self.assertEqual((job / 'scratch/graph-result.json').read_bytes(), b'preserve')
        result, job, _, _, _, _ = self.execute(bad_result=True)
        self.assertIsInstance(result, ValueError)
        self.assertFalse((job / 'scratch/graph-result.json').exists())

    def test_no_recipe_selector_and_shared_asset_versions_are_exact(self):
        with patch.dict(sys.modules, {'runtime_support': runtime}), patch.object(sys, 'argv', ['worker', 'other']):
            with self.assertRaises(ValueError):
                worker.main()
        pinned = json.loads((ROOT / 'workers/python/runtime_versions.json').read_bytes())
        historical = json.loads((ROOT / 'workers/python/probe/fixture.json').read_bytes())['versions']
        self.assertEqual(pinned, historical)
        self.assertEqual(len(pinned), 58)
        self.assertNotIn('NETWORKX_', (ROOT / 'workers/python/graph_worker.py').read_text())

    def test_future_probe_source_binding_includes_shared_extraction_and_fixed_assets(self):
        sys.path.insert(0, str(ROOT / 'scripts'))
        self.addCleanup(sys.path.remove, str(ROOT / 'scripts'))
        import test_python_compatibility as common
        import python_canonical_graph_receipts as canonical
        import python_engine_receipts as engines
        for path in ('crates/core/src/engines/python_graph.rs',
                     'crates/core/src/engines/supervision/python.rs',
                     'crates/core/src/store/file_identity.rs',
                     'workers/python/runtime_support.py',
                     'workers/python/graph_worker.py', 'workers/python/runtime_versions.json'):
            self.assertIn(path, common.SOURCES)
            self.assertTrue((ROOT / path).is_file())
        for assigned in (common.ASSIGNED, canonical.ASSIGNED, engines.assigned('networkx'), engines.assigned('transactions')):
            self.assertEqual(assigned['code/runtime_support.py'], 'workers/python/runtime_support.py')


if __name__ == '__main__':
    unittest.main()
