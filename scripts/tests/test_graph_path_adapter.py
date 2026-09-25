"""Stdlib-only contract/IPC tests. No packaged interpreter or NetworkX executes."""
import importlib.util
import io
import json
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('fixed_graph_path', ROOT / 'workers/python/graph_path.py')
adapter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adapter)


def request():
    return {
        'schema_version': 1, 'recipe': adapter.RECIPE, 'policy': adapter.POLICY,
        'nonce': '63472a3b-7d95-4516-b5b8-4ead64afef86', 'workspace_revision': 2,
        'snapshot_sha256': 'a' * 64, 'engine': 'networkx', 'engine_version': '3.6.1',
        'runtime_manifest_sha256': adapter.RUNTIME, 'source_id': 'a', 'target_id': 'c',
        'nodes': ['a', 'b', 'c', 'f'], 'edges': [['a', 'b'], ['b', 'c']],
    }


def encoded(value):
    return json.dumps(value, ensure_ascii=True, separators=(',', ':')).encode()


class NoPath(Exception):
    pass


class FakeGraph:
    def add_nodes_from(self, nodes):
        self.nodes = list(nodes)

    def add_edges_from(self, edges):
        self.edges = list(edges)


def fake_engine(outcome=None, error=None):
    def shortest(graph, source, target, *, backend):
        if backend != 'networkx':
            raise AssertionError('alternate backend selected')
        if graph.nodes != ['a', 'b', 'c', 'f'] or graph.edges != [['a', 'b'], ['b', 'c']]:
            raise AssertionError('supplied graph changed')
        if error:
            raise error
        return ['a', 'b', 'c'] if outcome is None else outcome
    return SimpleNamespace(__version__='3.6.1', Graph=FakeGraph,
                           NetworkXNoPath=NoPath, shortest_path=shortest)


class GraphContractTests(unittest.TestCase):
    def invalid(self, raw):
        with patch.object(adapter.importlib, 'import_module') as load:
            with self.assertRaisesRegex(adapter.GraphAdapterError, '^invalid-graph-request$'):
                adapter.execute(raw)
            load.assert_not_called()

    def test_fixed_shape_rejects_missing_extra_and_executable_fields_before_import(self):
        for key in adapter.REQUEST_FIELDS:
            value = request(); value.pop(key)
            self.invalid(encoded(value))
        for key in ['operation', 'code', 'sql', 'paths', 'assertions', 'provenance', '__class__', 'backend', 'backend_priority', 'backend_info']:
            value = request(); value[key] = 'do not execute or reflect'
            self.invalid(encoded(value))

    def test_identity_types_and_pins_are_exact(self):
        changes = {
            'schema_version': [True, 1.0, '1', 0, 2, None],
            'workspace_revision': [True, 1.0, '2', -1, 2**64, None],
            'recipe': ['', 'graph_paths', [], None],
            'policy': ['directed', None], 'engine': ['other', None],
            'engine_version': ['3.6.0', 3.6],
            'runtime_manifest_sha256': ['0' * 64, None],
            'snapshot_sha256': ['A' * 64, 'a' * 63, 'g' * 64, '../source', None],
            'nonce': ['', 'not-a-uuid', '63472A3B-7D95-4516-B5B8-4EAD64AFEF86',
                      '63472a3b-7d95-1516-b5b8-4ead64afef86',
                      '63472a3b-7d95-4516-15b8-4ead64afef86', None],
        }
        for key, values in changes.items():
            for replacement in values:
                with self.subTest(key=key, replacement=replacement):
                    value = request(); value[key] = replacement
                    self.invalid(encoded(value))
        for revision in [0, 2**64 - 1]:
            value = request(); value['workspace_revision'] = revision
            self.assertEqual(adapter.parse_request(encoded(value)), value)

    def test_malformed_json_duplicates_constants_and_non_utf8_never_import(self):
        raw = encoded(request())
        for bad in [b'', b'{', b'[]', b'null', b'\xff', raw + b'{}', b'\xef\xbb\xbf' + raw,
                    raw.replace(b'"schema_version":1', b'"schema_version":1,"schema_version":1'),
                    raw.replace(b'"workspace_revision":2', b'"workspace_revision":NaN'),
                    raw.replace(b'"workspace_revision":2', b'"workspace_revision":Infinity'),
                    raw.replace(b'"nodes":[', b'"nodes":{"bad":1,"bad":2},"unknown":['),
                    b'[' * 2000 + b'0' + b']' * 2000, b' ' * (adapter.MAX_INPUT_BYTES + 1),
                    bytearray(raw), raw.decode()]:
            self.invalid(bad)

    def test_nodes_are_sorted_unique_bounded_scalar_ids_and_endpoints_are_members(self):
        for nodes in [[], ['a'], ['a', 'c', 'b'], ['a', 'b', 'b', 'c'], 'abc',
                      ['a', True, 'c'], ['a', ['b'], 'c'], ['a', {'id': 'b'}, 'c'],
                      ['a', 'b' * 129, 'c'], ['a', 'b\n', 'c'], ['a', ' b', 'c'],
                      ['a', '\ud800', 'c'], [f'n{i:04}' for i in range(1001)]]:
            value = request(); value['nodes'] = nodes
            self.invalid(encoded(value))
        for key, replacement in [('source_id', 'absent'), ('source_id', True),
                                 ('target_id', 'a'), ('target_id', 'c '), ('source_id', 'a\x00')]:
            value = request(); value[key] = replacement
            self.invalid(encoded(value))
        # Preserve Rust's Unicode scalar ordering and UTF-8 byte bound, not ASCII-only IDs.
        value = request(); value.update(nodes=['A', '\u200b', '😀'], edges=[], source_id='A', target_id='😀')
        self.assertEqual(adapter.parse_request(encoded(value)), value)
        for node in ['é' * 65, '😀' * 33, '\x85', '\x7f']:
            value = request(); value.update(nodes=sorted(['a', node]), edges=[], target_id=node)
            self.invalid(encoded(value))

    def test_edges_are_flat_known_canonical_unique_and_bounded(self):
        for edges in [{}, None, 'edges', [['b', 'a']], [['a']], [['a', 'b', 'c']],
                      [['a', True]], [['a', ['b']]], [['a', 'absent']],
                      [['b', 'c'], ['a', 'b']], [['a', 'b'], ['a', 'b']],
                      [['a', 'b']] * 5001]:
            value = request(); value['edges'] = edges
            self.invalid(encoded(value))
        value = request(); value['edges'] = [['a', 'a'], ['a', 'b'], ['b', 'c']]
        self.assertEqual(adapter.parse_request(encoded(value)), value)

    def test_core_backend_preserves_identity_and_does_not_return_provenance(self):
        value = request()
        with patch.object(adapter.importlib, 'import_module', return_value=fake_engine()) as load:
            result = json.loads(adapter.execute(encoded(value)))
        load.assert_called_once_with('networkx')
        self.assertEqual(result, {**{key: value[key] for key in adapter.IDENTITY_FIELDS},
                                  'outcome': {'state': 'path', 'nodes': ['a', 'b', 'c']}})
        self.assertEqual(set(result), adapter.IDENTITY_FIELDS | {'outcome'})

    def test_isolated_endpoint_no_path_has_exact_empty_outcome(self):
        value = request(); value['target_id'] = 'f'
        with patch.object(adapter.importlib, 'import_module', return_value=fake_engine(error=NoPath())):
            self.assertEqual(json.loads(adapter.execute(encoded(value)))['outcome'], {'state': 'unreachable'})

    def test_unexpected_engine_failure_version_or_result_never_becomes_unreachable(self):
        for engine in [fake_engine(error=RuntimeError('private exception detail')),
                       fake_engine(outcome=['a', 'unknown', 'c']), fake_engine(outcome=['a', 'b', 'a', 'c']),
                       fake_engine(outcome=['c', 'b', 'a']), fake_engine(outcome=[]),
                       fake_engine(outcome=('a', 'b', 'c'))]:
            with patch.object(adapter.importlib, 'import_module', return_value=engine):
                with self.assertRaisesRegex(adapter.GraphAdapterError, '^graph-engine-failed$'):
                    adapter.execute(encoded(request()))
        engine = fake_engine(); engine.__version__ = 'other'
        with patch.object(adapter.importlib, 'import_module', return_value=engine):
            with self.assertRaisesRegex(adapter.GraphAdapterError, '^graph-engine-unavailable$'):
                adapter.execute(encoded(request()))
        with patch.object(adapter.importlib, 'import_module', side_effect=RuntimeError('private path')):
            with self.assertRaisesRegex(adapter.GraphAdapterError, '^graph-engine-unavailable$'):
                adapter.execute(encoded(request()))

    def test_serializer_bounds_escaping_before_output_growth(self):
        with self.assertRaisesRegex(adapter.GraphAdapterError, '^graph-result-too-large$'):
            adapter.encode_result({'nodes': ['"' * 128] * 1000})
        self.assertLess(len(adapter.encode_result({'state': 'unreachable'})), adapter.MAX_RESULT_BYTES)


class StreamTests(unittest.TestCase):
    def test_short_reads_and_writes_complete_only_the_closed_result(self):
        class ShortRead(io.BytesIO):
            def read(self, size):
                self.asserted_sizes.append(size)
                return super().read(min(size, 7))
        class ShortWrite(io.BytesIO):
            def write(self, value):
                return super().write(value[:5])
        source = ShortRead(encoded(request())); source.asserted_sizes = []
        target = ShortWrite()
        with patch.object(adapter.importlib, 'import_module', return_value=fake_engine()):
            expected = adapter.execute(encoded(request()))
            adapter.process_streams(source, target)
        self.assertEqual(target.getvalue(), expected)
        self.assertTrue(all(0 < size <= adapter.CHUNK_BYTES for size in source.asserted_sizes))
        self.assertFalse(source.closed); self.assertFalse(target.closed)

    def test_input_limit_reads_at_most_one_extra_byte_and_writes_nothing(self):
        source = io.BytesIO(b' ' * (adapter.MAX_INPUT_BYTES + 100))
        target = io.BytesIO()
        with patch.object(adapter.importlib, 'import_module') as load:
            with self.assertRaises(adapter.GraphAdapterError):
                adapter.process_streams(source, target)
        self.assertEqual(source.tell(), adapter.MAX_INPUT_BYTES + 1)
        self.assertEqual(target.getvalue(), b''); load.assert_not_called()

    def test_input_errors_and_read_contract_violations_write_nothing(self):
        for read in [lambda _: None, lambda n: b'x' * (n + 1), lambda _: 'text']:
            target = io.BytesIO()
            with self.assertRaisesRegex(adapter.GraphAdapterError, '^graph-input-io$'):
                adapter.process_streams(SimpleNamespace(read=read), target)
            self.assertEqual(target.getvalue(), b'')
        def fail(_):
            raise OSError('private read path')
        with self.assertRaisesRegex(adapter.GraphAdapterError, '^graph-input-io$'):
            adapter.process_streams(SimpleNamespace(read=fail), io.BytesIO())

    def test_partial_output_or_flush_error_stays_failed_without_retry(self):
        class Partial(io.BytesIO):
            def write(self, value):
                if self.tell():
                    raise OSError('private write path')
                return super().write(value[:5])
        class FlushError(io.BytesIO):
            def flush(self):
                raise OSError('private flush path')
        for target in [Partial(), FlushError()]:
            with patch.object(adapter.importlib, 'import_module', return_value=fake_engine()) as load:
                with self.assertRaisesRegex(adapter.GraphAdapterError, '^graph-output-io$'):
                    adapter.process_streams(io.BytesIO(encoded(request())), target)
            self.assertTrue(target.getvalue()); load.assert_called_once()
        for written in [0, -1, None, True, adapter.MAX_RESULT_BYTES + 1]:
            target = SimpleNamespace(write=lambda _, count=written: count, flush=lambda: None)
            with patch.object(adapter.importlib, 'import_module', return_value=fake_engine()):
                with self.assertRaisesRegex(adapter.GraphAdapterError, '^graph-output-io$'):
                    adapter.process_streams(io.BytesIO(encoded(request())), target)


if __name__ == '__main__':
    unittest.main()
