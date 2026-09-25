"""Fixed canonical graph adapter; no launcher, paths or canonical write authority.

Only Rust selects accepted records and preserves provenance. A result's echoed
identity is correlation data, not proof of execution or snapshot authenticity.
The future supervisor must supply verified code/runtime and assigned streams.
"""
import importlib
import json
import re
import unicodedata
import uuid

MAX_INPUT_BYTES = 1024 * 1024
MAX_RESULT_BYTES = 128 * 1024
MAX_NODES = 1_000
MAX_EDGES = 5_000
CHUNK_BYTES = 64 * 1024
RECIPE = 'shortest_connection_path_v1'
POLICY = 'accepted_undirected_all_retained_time_v1'
ENGINE = 'networkx'
ENGINE_VERSION = '3.6.1'
RUNTIME = '4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822'
IDENTITY_FIELDS = frozenset({
    'schema_version', 'recipe', 'policy', 'nonce', 'workspace_revision',
    'snapshot_sha256', 'engine', 'engine_version', 'runtime_manifest_sha256',
})
REQUEST_FIELDS = IDENTITY_FIELDS | {'source_id', 'target_id', 'nodes', 'edges'}
HEX = re.compile(r'[0-9a-f]{64}\Z', re.ASCII)


class GraphAdapterError(ValueError):
    """Fixed sanitized failure code; a failure is never a graph result."""


def require(condition, code='invalid-graph-request'):
    if not condition:
        raise GraphAdapterError(code)


def unique(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result)
        result[key] = value
    return result


def reject_constant(_):
    raise GraphAdapterError('invalid-graph-request')


def identifier(value):
    require(type(value) is str and value and value.strip() == value)
    try:
        size = len(value.encode('utf-8', errors='strict'))
    except UnicodeError:
        raise GraphAdapterError('invalid-graph-request') from None
    require(size <= 128 and not any(unicodedata.category(c) == 'Cc' for c in value))


def parse_request(raw):
    require(type(raw) is bytes and 0 < len(raw) <= MAX_INPUT_BYTES)
    try:
        request = json.loads(raw.decode('utf-8', errors='strict'),
                             object_pairs_hook=unique, parse_constant=reject_constant)
    except (ValueError, UnicodeError, RecursionError):
        raise GraphAdapterError('invalid-graph-request') from None
    require(type(request) is dict and request.keys() == REQUEST_FIELDS)
    require(type(request['schema_version']) is int and request['schema_version'] == 1)
    require(type(request['workspace_revision']) is int
            and 0 <= request['workspace_revision'] <= 2**64 - 1)
    for key, expected in (
        ('recipe', RECIPE), ('policy', POLICY), ('engine', ENGINE),
        ('engine_version', ENGINE_VERSION), ('runtime_manifest_sha256', RUNTIME),
    ):
        require(type(request[key]) is str and request[key] == expected)
    nonce = request['nonce']
    require(type(nonce) is str and len(nonce) == 36)
    try:
        parsed_nonce = uuid.UUID(nonce)
    except ValueError:
        raise GraphAdapterError('invalid-graph-request') from None
    require(str(parsed_nonce) == nonce and parsed_nonce.version == 4
            and parsed_nonce.variant == uuid.RFC_4122)
    digest = request['snapshot_sha256']
    require(type(digest) is str and HEX.fullmatch(digest) is not None)
    nodes = request['nodes']
    require(type(nodes) is list and 2 <= len(nodes) <= MAX_NODES)
    previous = None
    for node in nodes:
        identifier(node)
        require(previous is None or previous < node)
        previous = node
    members = set(nodes)
    for key in ('source_id', 'target_id'):
        identifier(request[key])
        require(request[key] in members)
    require(request['source_id'] != request['target_id'])
    edges = request['edges']
    require(type(edges) is list and len(edges) <= MAX_EDGES)
    previous = None
    for edge in edges:
        require(type(edge) is list and len(edge) == 2)
        require(all(type(node) is str and node in members for node in edge))
        a, b = edge
        require(a <= b)  # Rust emits each undirected pair in canonical order.
        pair = (a, b)
        require(previous is None or previous < pair)
        previous = pair
    return request


def encode_result(result):
    output = bytearray()
    encoder = json.JSONEncoder(ensure_ascii=False, allow_nan=False, sort_keys=True,
                               separators=(',', ':'))
    for part in encoder.iterencode(result):
        data = part.encode('utf-8', errors='strict')
        require(len(data) <= MAX_RESULT_BYTES - len(output), 'graph-result-too-large')
        output.extend(data)
    return bytes(output)


def execute(raw):
    """Compute only the supplied topology; Rust independently validates semantics."""
    request = parse_request(raw)
    try:
        nx = importlib.import_module('networkx')
        require(nx.__version__ == ENGINE_VERSION, 'graph-engine-unavailable')
    except Exception:
        raise GraphAdapterError('graph-engine-unavailable') from None
    try:
        graph = nx.Graph()
        graph.add_nodes_from(request['nodes'])  # Include isolated endpoints.
        graph.add_edges_from(request['edges'])
        try:
            nodes = nx.shortest_path(graph, request['source_id'], request['target_id'],
                                     backend='networkx')
            # Bound the result shape. Rust owns the independent path/shortest check.
            require(type(nodes) is list and 2 <= len(nodes) <= MAX_NODES,
                    'graph-engine-failed')
            members = set(request['nodes'])
            require(all(type(node) is str and node in members for node in nodes)
                    and nodes[0] == request['source_id'] and nodes[-1] == request['target_id']
                    and len(set(nodes)) == len(nodes), 'graph-engine-failed')
            outcome = {'state': 'path', 'nodes': nodes}
        except nx.NetworkXNoPath:
            outcome = {'state': 'unreachable'}
    except Exception:
        raise GraphAdapterError('graph-engine-failed') from None
    result = {key: request[key] for key in IDENTITY_FIELDS}
    result['outcome'] = outcome
    return encode_result(result)


def process_streams(source, destination):
    """Bounded IPC on already assigned binary streams; neither stream is owned here.

    The trusted supervisor must open a read-only assigned input and an exclusive
    no-follow scratch output. It enforces deadlines, reaps the process, validates
    the result file, and calls Rust's owned-capture validator. This function does
    not open paths, truncate files, print acknowledgements or close descriptors.
    Partial output on I/O failure remains failed evidence, never a success.
    """
    raw = bytearray()
    try:
        while len(raw) <= MAX_INPUT_BYTES:
            remaining = min(CHUNK_BYTES, MAX_INPUT_BYTES + 1 - len(raw))
            chunk = source.read(remaining)
            require(type(chunk) is bytes and len(chunk) <= remaining, 'graph-input-io')
            if not chunk:
                break
            raw.extend(chunk)
    except Exception:
        raise GraphAdapterError('graph-input-io') from None
    result = execute(bytes(raw))  # No destination write before complete validation/serialization.
    try:
        view = memoryview(result)
        offset = 0
        while offset < len(view):
            chunk = view[offset:offset + CHUNK_BYTES]
            written = destination.write(chunk)
            require(type(written) is int and 0 < written <= len(chunk), 'graph-output-io')
            offset += written
        destination.flush()
    except Exception:
        raise GraphAdapterError('graph-output-io') from None
