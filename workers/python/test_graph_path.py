"""Real-engine source tests in the existing locked development environment.

This is not the packaged candidate interpreter, a confined/native worker test,
or evidence that echoed runtime identities authenticate this development process.
"""
import json
from pathlib import Path

import networkx as nx
import pytest

import graph_path as adapter

FIXTURE = Path(__file__).parent / 'fixtures/canonical-graph-cases.v1.json'


def encode(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()


def base():
    return json.loads(FIXTURE.read_bytes())['cases'][0]['request']


@pytest.mark.parametrize('case_index', [0, 1, 2])
def test_captured_rust_protocol_fixtures_match_actual_networkx_result(case_index):
    assert nx.__version__ == '3.6.1'
    case = json.loads(FIXTURE.read_bytes())['cases'][case_index]
    assert adapter.execute(encode(case['request'])) == encode(case['result'])


def test_exact_core_backend_is_used_for_path_and_unreachable(monkeypatch):
    original = nx.shortest_path
    calls = []

    def observe(graph, source, target, *, backend):
        calls.append((type(graph), source, target, backend))
        return original(graph, source, target, backend=backend)

    monkeypatch.setattr(nx, 'shortest_path', observe)
    request = base()
    assert json.loads(adapter.execute(encode(request)))['outcome']['nodes'] == ['a', 'b', 'c']
    request['target_id'] = 'f'
    assert json.loads(adapter.execute(encode(request)))['outcome'] == {'state': 'unreachable'}
    assert calls == [(nx.Graph, 'a', 'c', 'networkx'), (nx.Graph, 'a', 'f', 'networkx')]


def test_equal_shortest_paths_and_self_edges_do_not_require_one_arbitrary_tie():
    request = base()
    request.update(nodes=['a', 'b', 'c', 'd', 'f'], edges=[['a', 'a'], ['a', 'b'], ['a', 'd'],
                                                          ['b', 'c'], ['c', 'd']])
    result = json.loads(adapter.execute(encode(request)))
    assert result['outcome'] in [{'state': 'path', 'nodes': ['a', 'b', 'c']},
                                {'state': 'path', 'nodes': ['a', 'd', 'c']}]
    assert set(result) == adapter.IDENTITY_FIELDS | {'outcome'}


def test_unicode_scalar_ids_preserve_exact_values_without_normalization():
    request = base()
    nodes = sorted(['e\u0301', 'é', '😀', '\u200b'])
    request.update(nodes=nodes, edges=sorted([sorted(['e\u0301', 'é']), sorted(['é', '😀'])]),
                   source_id='e\u0301', target_id='😀')
    assert json.loads(adapter.execute(encode(request)))['outcome'] == {
        'state': 'path', 'nodes': ['e\u0301', 'é', '😀'],
    }
    request['target_id'] = '\u200b'
    assert json.loads(adapter.execute(encode(request)))['outcome'] == {'state': 'unreachable'}


def test_closed_count_limits_admit_bounded_large_graphs():
    request = base()
    nodes = [f'n{i:04}' for i in range(1000)]
    request.update(nodes=nodes, edges=[], source_id=nodes[0], target_id=nodes[-1])
    assert json.loads(adapter.execute(encode(request)))['outcome'] == {'state': 'unreachable'}
    edges = [[a, b] for i, a in enumerate(nodes[:101]) for b in nodes[i + 1:101]][:5000]
    request.update(edges=edges, target_id=nodes[100])
    assert json.loads(adapter.execute(encode(request)))['outcome'] == {
        'state': 'path', 'nodes': [nodes[0], nodes[100]],
    }


def test_result_escaping_bound_refuses_complete_long_path_without_output(tmp_path):
    request = base()
    nodes = [f'n{i:04}' + '"' * 123 for i in range(1000)]
    request.update(nodes=nodes, edges=[[a, b] for a, b in zip(nodes, nodes[1:])],
                   source_id=nodes[0], target_id=nodes[-1])
    payload = encode(request)
    assert len(payload) <= adapter.MAX_INPUT_BYTES
    source = tmp_path / 'assigned.json'; source.write_bytes(payload)
    output = tmp_path / 'result.json'
    with source.open('rb') as reader, output.open('xb') as writer:
        with pytest.raises(adapter.GraphAdapterError, match='^graph-result-too-large$'):
            adapter.process_streams(reader, writer)
    assert output.read_bytes() == b''
    assert source.read_bytes() == payload


def test_assigned_file_streams_have_exact_complete_results_and_preserve_input(tmp_path):
    payload = encode(base())
    source = tmp_path / 'assigned.json'; source.write_bytes(payload)
    output = tmp_path / 'result.json'
    with source.open('rb') as reader, output.open('xb') as writer:
        adapter.process_streams(reader, writer)
        assert not reader.closed and not writer.closed
    assert output.read_bytes() == adapter.execute(payload)
    assert source.read_bytes() == payload


def test_pinned_development_metadata_has_no_import_time_backend_info_plugin():
    # Metadata inspection does not call EntryPoint.load(). Pinned NetworkX's
    # nx_loopback test entry is discarded during discovery without loading it.
    from importlib.metadata import entry_points
    assert list(entry_points(group='networkx.backend_info')) == []
    assert sorted((item.name, item.value) for item in entry_points(group='networkx.backends')) == [
        ('nx_loopback', 'networkx.classes.tests.dispatch_interface:backend_interface'),
    ]
    assert nx.utils.backends.backends == {}
    assert nx.utils.backends._loaded_backends == {}
    assert set(nx.utils.backends.backend_info) == {'networkx'}
