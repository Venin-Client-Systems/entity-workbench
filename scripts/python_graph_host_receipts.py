"""Independent bounded acceptance for one real host-constructor/public-API job."""
import json

import python_graph_coordinator_receipts as prior

common = prior.common
require = common.require
TEST = 'coordinator::native_graph_host_proof::native_graph_host_api'
identity = prior.identity
equal = prior.strict_equal


def summary(value, campaign):
    require(type(value) is dict and set(value) == {'schema_version', 'campaign_id', 'passed',
            'failure', 'observation', 'result', 'complete_release'}, 'host-receipt-shape')
    require(type(value['schema_version']) is int and value['schema_version'] == 1
            and value['campaign_id'] == campaign and common.canonical_uuid(campaign)
            and type(value['passed']) is bool and value['complete_release'] is False, 'host-receipt-binding')
    return prior.observation(value['observation'])


def canonical_delta(before, after, result):
    require(set(before) == set(after) == {'tables', 'originals'}
            and set(before['tables']) == set(after['tables']) == prior.TABLES, 'host-snapshot-shape')
    b, a = before['tables'], after['tables']
    queued, finished, record = result['queued']['job'], result['finished']['job'], result['inspection']['record']
    require(before['originals'] == after['originals'] and all(b[k] == a[k]
            for k in ('schema', 'version', 'derivative_objects')), 'host-static-or-original-mutation')
    require(b['meta'] == [[1, 2]] and a['meta'] == [[1, 5]], 'host-revisions')
    old, new = b['records'], a['records']
    require(new[:len(old)] == old and len(new) == len(old)+3
            and len({r[0] for r in new}) == len(new)
            and len({(r[1], r[2]) for r in new}) == len(new), 'host-record-set')
    expected = [('processing_request', queued['request_key'], queued['id']),
                ('processing_job', queued['id'], finished), ('graph_analysis', record['id'], record)]
    for row, (kind, key, body) in zip(new[len(old):], expected):
        require(len(row) == 4 and row[1:3] == [kind, key]
                and equal(json.loads(row[3], object_pairs_hook=common.inventory.unique_object), body), 'host-detached-record')
    require(equal(prior.canonical_record(after, 'processing_job', queued['id']), finished)
            and equal(prior.canonical_record(after, 'graph_analysis', record['id']), record), 'host-canonical-identity')
    for name, added in [('history', 2), ('events', 3)]:
        require(a[name][:len(b[name])] == b[name] and len(a[name]) == len(b[name])+added, 'host-audit-prefix')
    history = a['history'][len(b['history']):]
    require(history[0][1:3] == history[1][1:3] == ['processing_job', queued['id']]
            and [row[4] for row in history] == [4, 5]
            and equal(json.loads(history[0][3]), queued), 'host-queued-history')
    running = json.loads(history[1][3], object_pairs_hook=common.inventory.unique_object)
    require(running['state'] == 'running' and all(equal(running[k], finished[k])
            for k in ('id', 'request_key', 'input', 'attempt', 'lease')), 'host-running-history')
    events = a['events'][len(b['events']):]
    require([row[1:3] for row in events] == [[3, 'processing.graph_queue'],
            [4, 'processing.graph_claim'], [5, 'processing.graph_finish']], 'host-event-set')
    previous, current = dict(b['sqlite_sequence']), dict(a['sqlite_sequence'])
    require(len(current) == len(a['sqlite_sequence']) == 3 and set(current) == {'records', 'history', 'events'}
            and set(previous) <= set(current)
            and all(current[name] == previous.get(name, 0)+increment
                    for name, increment in [('records', 5), ('history', 2), ('events', 3)]), 'host-sequence-set')
    require(result['delta'] == {'before': prior.snapshot_identity(before), 'after': prior.snapshot_identity(after),
            'all_tables_checked': True, 'new_events': events}, 'host-snapshot-identities')


def public_results(result, request, graph, record):
    require(set(result) == {'queued', 'page', 'finished', 'inspection', 'replay', 'delta',
            'replay_unchanged', 'joined_shutdown', 'reopen_unchanged', 'scratch_empty'}
            and all(result[k] is True for k in ('replay_unchanged', 'joined_shutdown', 'reopen_unchanged', 'scratch_empty')),
            'host-result-shape')
    queued, page, finished, inspected = (result[k] for k in ('queued', 'page', 'finished', 'inspection'))
    q, f = queued['job'], finished['job']
    require(all(type(v['schema_version']) is int and v['schema_version'] == 1 for v in (queued, page, finished))
            and all(type(v['schema_version']) is int and v['schema_version'] == 5 for v in (q, f))
            and type(record['schema_version']) is int and record['schema_version'] == 1, 'host-public-schema')
    require(queued['availability'] == page['availability'] == finished['availability'] == 'ready'
            and queued['workspace_revision'] == 3 and page['workspace_revision'] == finished['workspace_revision'] == 5,
            'host-public-revision-availability')
    require(q['state'] == 'queued' and f['state'] == 'completed' and f['failure'] is None
            and q['attempt'] == f['attempt'] == 1 and common.canonical_uuid(q['request_key'])
            and common.canonical_uuid(q['id']) and common.canonical_uuid(f['lease'])
            and all(equal(q[k], f[k]) for k in ('id', 'request_key', 'input'))
            and q['input'] == {'operation': 'shortest_connection_path', 'source_id': 'a', 'target_id': 'c',
                              'requested_revision': 2, 'queued_revision': 3}, 'host-public-job')
    require(page['total_count'] == 1 and len(page['rows']) == 1 and page['next_cursor'] is None
            and equal(page['rows'][0]['job'], f) and finished['execution'] is None
            and finished['controls'] == {'can_cancel': False, 'can_retry_publication': False}
            and equal(result['replay'], finished), 'host-page-replay')
    expected_ref = {'id': record['id'], 'request_sha256': record['request_sha256'],
                    'result_sha256': record['result_sha256'], 'captured_revision': 4, 'published_revision': 5}
    require(finished['results'] == [expected_ref] and f['result_ids'] == [record['id']]
            and equal(inspected['record'], record) and inspected['compared_revision'] == 5
            and inspected['original_integrity'] == 'verified' and inspected['freshness'] == 'current_at_publication',
            'host-discoverable-reference')
    require(record['job_id'] == f['id'] and record['request_key'] == f['request_key']
            and record['host_attempt_lease'] == f['lease'] and record['attempt'] == 1
            and [record[k] for k in ('requested_revision', 'queued_revision', 'captured_revision', 'published_revision')] == [2, 3, 4, 5]
            and record['request_sha256'] == identity(record['request_json'].encode())['sha256']
            and record['result_sha256'] == identity(record['result_json'].encode())['sha256'], 'host-frozen-job-binding')
    require(request['workspace_revision'] == 4 and request['source_id'] == 'a' and request['target_id'] == 'c'
            and request['nodes'] == list('abcdef') and request['edges'] == [['a', 'b'], ['a', 'd'], ['b', 'c'], ['c', 'e'], ['d', 'e']],
            'host-real-fixture-topology')
    expected = {k: v for k, v in request.items() if k not in ('nodes', 'edges', 'source_id', 'target_id')}
    expected['outcome'] = {'state': 'path', 'nodes': ['a', 'b', 'c']}
    require(equal(graph, expected) and record['frozen']['assertion_reviews'] == {'accepted': 6, 'pending': 1, 'rejected': 1, 'deferred': 1}
            and record['outcome'] == {'state': 'path', 'nodes': ['a', 'b', 'c'], 'hops': [
                {'source_id': 'a', 'target_id': 'b', 'assertion_ids': ['r1', 'r1-parallel']},
                {'source_id': 'b', 'target_id': 'c', 'assertion_ids': ['r2']}]}, 'host-exact-path-provenance')


def accept(receipt, campaign, directory):
    # Parent-written receipt only until this gate. Uncertainty forbids DB/original/runtime reads.
    require(summary(receipt, campaign), 'host-termination-or-cleanup-unconfirmed')
    require(receipt['passed'] is True and receipt['failure'] is None, 'host-native-case-failed')
    o, result = receipt['observation'], receipt['result']
    require(o['execution_calls'] == o['launch_count'] == 1 and type(o['pid']) is int and o['pid'] > 0
            and common.canonical_uuid(o['assignment_id']) and common.canonical_uuid(o['capture_nonce'])
            and o['pre_inventory_verified'] is o['post_inventory_verified'] is True
            and o['stop_ms'] >= o['launch_ms'] and o['cancellation_seen_ms'] is None, 'host-single-successful-launch')
    paths = {'code/graph_worker.py': 'workers/python/graph_worker.py', 'code/runtime_support.py': 'workers/python/runtime_support.py',
             'code/graph_path.py': 'workers/python/graph_path.py', 'input/runtime-versions.json': 'workers/python/runtime_versions.json'}
    require(common.sha256(o['profile_sha256']) and type(o['assigned_files']) is dict
            and set(o['assigned_files']) == set(paths) | {'input/assignment.json', 'input/graph-request.json'}, 'host-assets')
    for name, source in paths.items():
        require(o['assigned_files'][name] == identity((common.ROOT/source).read_bytes()), 'host-asset-source-binding')
    require(o['assigned_files']['input/graph-request.json'] == o['request_identity']
            and common.asset_identity(o['assigned_files']['input/assignment.json'], 64*1024), 'host-assigned-request')
    req, wrapper_raw, raw_result = (prior.raw(directory/name, maximum, o[field]) for name, maximum, field in [
        ('request.json', 1024**2, 'request_identity'), ('wrapper.json', 64*1024, 'wrapper_identity'), ('result.json', 128*1024, 'result_identity')])
    request, wrapper, graph = (json.loads(raw, object_pairs_hook=common.inventory.unique_object) for raw in (req, wrapper_raw, raw_result))
    require(request['nonce'] == o['capture_nonce'] and request['runtime_manifest_sha256'] == common.MANIFEST
            and request['engine'] == 'networkx' and request['engine_version'] == '3.6.1', 'host-runtime-binding')
    versions = common.read_json(common.ROOT/'workers/python/runtime_versions.json', 64*1024)
    require(equal(wrapper, {'schema_version': 1, 'recipe': 'python-graph-job-v1', 'job_id': o['assignment_id'],
            'manifest_sha256': common.MANIFEST, 'python_version': '3.13.15', 'isolated': True, 'no_site': True,
            'no_bytecode': True, 'verified_paths': True, 'checks': {'versions': versions, 'imported_modules': ['networkx'],
            'backend_metadata_checked': True, 'capture_nonce': o['capture_nonce'], 'request_identity': o['request_identity'],
            'result_identity': o['result_identity']}}), 'host-raw-wrapper-binding')
    record = common.read_json(directory/'record.json', 16*1024**2)
    require(record['request_json'].encode() == req and record['result_json'].encode() == raw_result, 'host-frozen-raw-bytes')
    public_results(result, request, graph, record)
    before = common.read_json(directory/'canonical-before.json', 64*1024**2)
    after = common.read_json(directory/'canonical-after.json', 64*1024**2)
    kinds = [row[1] for row in before['tables']['records']]
    require(all(kinds.count(kind) == count for kind, count in [('entity', 6), ('assertion', 9), ('observation', 9)]), 'host-fixture-cardinality')
    canonical_delta(before, after, result)
    prior.retained_workspace(directory, after)
