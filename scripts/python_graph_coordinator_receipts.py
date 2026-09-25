"""Independent bounded acceptance of the four-case coordinator campaign; no engine import."""
import hashlib
import json
import sqlite3
from urllib.parse import quote
import test_python_compatibility as common

CASES = ('ordinary_retry', 'unreachable', 'large_chain', 'live_cancel')
TEST = 'coordinator::graph::native_graph_proof::native_graph_coordinator_campaign'
require = common.require
TABLES = {'schema', 'version', 'meta', 'records', 'history', 'events', 'derivative_objects', 'sqlite_sequence'}
ACTIONS = {'processing.graph_claim', 'processing.graph_finish', 'processing.cancel',
           'processing.claim', 'processing.finish', 'collection.durable.checkpoint'}
OBSERVER = {'execution_calls', 'launch_count', 'pid', 'launch_ms', 'live_observed_ms',
            'cancellation_seen_ms', 'stop_ms', 'termination', 'cleanup', 'pre_inventory_verified',
            'post_inventory_verified', 'assignment_id', 'capture_nonce', 'request_identity',
            'wrapper_identity', 'result_identity', 'profile_sha256', 'assigned_files'}


def identity(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def raw(path, maximum, expected):
    require(path.is_file() and not path.is_symlink() and path.stat().st_nlink == 1
            and path.stat().st_size <= maximum, 'retained-file-unavailable')
    with path.open('rb') as stream:
        data = stream.read(maximum + 1)
    require(len(data) <= maximum and identity(data) == expected, 'retained-byte-identity-mismatch')
    return data


def strict_equal(a, b):
    return json.dumps(a,sort_keys=True,separators=(',',':'),ensure_ascii=False,allow_nan=False) == json.dumps(b,sort_keys=True,separators=(',',':'),ensure_ascii=False,allow_nan=False)


def snapshot_identity(value):
    # Match the Rust Snapshot struct field order and both BTreeMap orders.
    ordered = {'tables':dict(sorted(value['tables'].items())), 'originals':dict(sorted(value['originals'].items()))}
    return identity(json.dumps(ordered,separators=(',',':'),ensure_ascii=False,allow_nan=False).encode())


def canonical_record(snapshot, kind, key):
    selected = [row for row in snapshot['tables']['records'] if row[1:3] == [kind,key]]
    require(len(selected) == 1 and len(selected[0]) == 4 and type(selected[0][0]) is int
            and selected[0][0] > 0 and type(selected[0][3]) is str, 'canonical-selected-row-identity')
    body = json.loads(selected[0][3],object_pairs_hook=common.inventory.unique_object)
    require(type(body) is dict and body.get('id') == key, 'canonical-key-body-identity')
    return body


def bind_canonical(before, after, result, record=None):
    queued, finished = result['queued'], result['finished']
    require(strict_equal(canonical_record(before,'processing_job',queued['id']),queued)
            and strict_equal(canonical_record(after,'processing_job',finished['id']),finished), 'detached-job-receipt')
    if result['case'] == 'ordinary_retry':
        e = result['exclusive']
        for kind,key,terminal in [('processing_job',e['document_job'],result['competitors']['document']),
                                  ('collection_run',e['collection_job'],result['competitors']['collection'])]:
            initial = canonical_record(before,kind,key)
            require((initial.get('state') if kind == 'processing_job' else initial['checkpoint']['state']) == 'queued'
                    and strict_equal(canonical_record(after,kind,key),terminal), 'detached-competitor-receipt')
    if record is not None:
        require(strict_equal(canonical_record(after,'graph_analysis',result['record_id']),record), 'detached-graph-record')


def observation(value):
    require(type(value) is dict and set(value) == OBSERVER, 'observer-shape')
    require(all(type(value[k]) is int and 0 <= value[k] <= 2 for k in ('execution_calls', 'launch_count')),
            'observer-count')
    require(value['termination'] in ('not_started', 'unconfirmed', 'unverified', 'confirmed')
            and value['cleanup'] in ('not_started', 'not_attempted_unverified', 'failed', 'confirmed'),
            'observer-lifecycle')
    require(all(type(value[k]) is bool for k in ('pre_inventory_verified', 'post_inventory_verified')),
            'observer-inventory')
    for key in ('launch_ms', 'live_observed_ms', 'cancellation_seen_ms', 'stop_ms'):
        require(value[key] is None or type(value[key]) is int and 0 <= value[key] <= 720_000,
                'observer-time')
    for key, maximum in [('request_identity', 1024**2), ('wrapper_identity', 64*1024), ('result_identity', 128*1024)]:
        ident = value[key]
        require(type(ident) is dict and set(ident) == {'bytes', 'sha256'} and type(ident['bytes']) is int
                and 0 <= ident['bytes'] <= maximum and common.sha256(ident['sha256']), 'observer-asset')
    return value['termination'] == 'confirmed' and value['cleanup'] == 'confirmed'


def summary(receipt, campaign, case):
    require(type(receipt) is dict and set(receipt) == {'schema_version', 'campaign_id', 'case', 'passed',
            'failure', 'observation', 'result', 'complete_release'}, 'receipt-shape')
    require(type(receipt['schema_version']) is int and receipt['schema_version'] == 1
            and receipt['campaign_id'] == campaign and common.canonical_uuid(campaign)
            and receipt['case'] == case and case in CASES and type(receipt['passed']) is bool
            and receipt['complete_release'] is False, 'receipt-binding')
    return observation(receipt['observation'])


def canonical_delta(before, after, mutable, result):
    require(type(before) is dict and type(after) is dict and set(before) == set(after) == {'tables', 'originals'}
            and set(before['tables']) == set(after['tables']) == TABLES, 'canonical-snapshot-shape')
    require(before['originals'] == after['originals'], 'originals-changed')
    b, a = before['tables'], after['tables']
    require(all(b[k] == a[k] for k in ('schema', 'version', 'derivative_objects')), 'static-tables-changed')
    old = {row[0]: row for row in b['records']}
    new = {row[0]: row for row in a['records']}
    require(len(old) == len(b['records']) and len(new) == len(a['records'])
            and old.keys() <= new.keys() and len(new) == len(old) + int(result is not None), 'record-set-changed')
    allowed = set(map(tuple, mutable))
    for sequence, row in old.items():
        now = new[sequence]
        require(now == row or now[:3] == row[:3] and tuple(now[1:3]) in allowed, 'unrelated-record-change')
    inserted = [new[n] for n in new.keys() - old.keys()]
    require(all(row[1:3] == ['graph_analysis', result] for row in inserted), 'unexpected-insert')
    for table in ('events', 'history'):
        require(a[table][:len(b[table])] == b[table], 'existing-audit-changed')
        for row in a[table][len(b[table]):]:
            require((tuple(row[1:3]) in allowed and type(row[4]) is int and b['meta'][0][1] < row[4] <= a['meta'][0][1]) if table == 'history' else row[2] in ACTIONS,
                    'unexpected-audit-write')
    events = a['events'][len(b['events']):]
    r = b['meta'][0][1]
    require(a['meta'] == [[1, r + len(events)]] and [row[1] for row in events] == list(range(r+1, r+len(events)+1)),
            'revision-sequence')
    before_seq, after_seq = dict(b['sqlite_sequence']), dict(a['sqlite_sequence'])
    require(set(before_seq) <= set(after_seq) <= {'records', 'history', 'events'}, 'sequence-set')
    for table, sequence in after_seq.items():
        added = len(a[table]) - len(b[table])
        updates = len(a['history']) - len(b['history']) if table == 'records' else 0
        require(sequence == before_seq.get(table, 0) + added + updates, 'sequence-write-count')


def retained_workspace(directory, expected):
    queries = {
        'schema':'SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name',
        'version':'PRAGMA user_version', 'meta':'SELECT rowid,revision FROM meta ORDER BY rowid',
        'records':'SELECT sequence,kind,id,body FROM records ORDER BY sequence',
        'history':'SELECT sequence,kind,id,body,revision FROM history ORDER BY sequence',
        'events':'SELECT sequence,revision,action,at FROM events ORDER BY sequence',
        'derivative_objects':'SELECT sha256,bytes FROM derivative_objects ORDER BY sha256',
        'sqlite_sequence':'SELECT name,seq FROM sqlite_sequence ORDER BY name'}
    db = directory/'workspace/workspace.db'
    require(db.is_file() and not db.is_symlink() and db.stat().st_size <= 64*1024**2, 'retained-database-bound')
    with sqlite3.connect('file:'+quote(str(db))+'?mode=ro', uri=True) as conn:
        conn.execute('PRAGMA query_only=ON')
        actual = {key:[list(row) for row in conn.execute(query)] for key,query in queries.items()}
    require(actual == expected['tables'], 'retained-database-differs-from-receipt')
    originals = directory/'workspace/originals'
    require(not originals.is_symlink() and set(p.name for p in originals.iterdir()) == set(expected['originals']), 'retained-original-set')
    for name,(size,digest) in expected['originals'].items():
        require(common.sha256(name) and size <= 64*1024, 'retained-original-identity')
        raw(originals/name,64*1024,{'bytes':size,'sha256':digest})


def accept(receipt, campaign, case, directory):
    # All filesystem output/snapshot reads are below this explicit lifecycle gate.
    require(summary(receipt, campaign, case), 'termination-or-cleanup-unconfirmed')
    require(receipt['passed'] is True and receipt['failure'] is None, 'native-case-failed')
    o, result = receipt['observation'], receipt['result']
    require(o['execution_calls'] == o['launch_count'] == 1 and type(o['pid']) is int and o['pid'] > 0
            and common.canonical_uuid(o['assignment_id']) and common.canonical_uuid(o['capture_nonce'])
            and o['pre_inventory_verified'] is True and o['stop_ms'] >= o['launch_ms'], 'native-assignment')
    require(result['case'] == case and result['observation'] == o
            and all(result[k] is True for k in ('replay_unchanged', 'reopen_unchanged', 'scratch_empty')),
            'canonical-result-binding')
    require(common.sha256(o['profile_sha256']) and type(o['assigned_files']) is dict, 'prepared-profile-assets')
    assigned = o['assigned_files']
    paths = {'code/graph_worker.py':'workers/python/graph_worker.py',
             'code/runtime_support.py':'workers/python/runtime_support.py',
             'code/graph_path.py':'workers/python/graph_path.py',
             'input/runtime-versions.json':'workers/python/runtime_versions.json'}
    require(set(assigned) == set(paths) | {'input/assignment.json','input/graph-request.json'}, 'closed-assigned-assets')
    for name, source in paths.items():
        require(assigned[name] == identity((common.ROOT/source).read_bytes()), 'assigned-source-bytes')
    require(assigned['input/graph-request.json'] == o['request_identity']
            and common.asset_identity(assigned['input/assignment.json'],64*1024), 'assigned-request-binding')
    req_raw = raw(directory/'request.json', 1024**2, o['request_identity'])
    wrapper_raw = raw(directory/'wrapper.json', 64*1024, o['wrapper_identity'])
    graph_raw = raw(directory/'result.json', 128*1024, o['result_identity'])
    request = json.loads(req_raw,object_pairs_hook=common.inventory.unique_object)
    require(request['nonce'] == o['capture_nonce'] and request['runtime_manifest_sha256'] == common.MANIFEST
            and request['engine'] == 'networkx' and request['engine_version'] == '3.6.1', 'request-runtime-binding')
    before = common.read_json(directory/'canonical-before.json', 64*1024**2)
    after = common.read_json(directory/'canonical-after.json', 64*1024**2)
    retained_workspace(directory, after)
    kinds = [row[1] for row in before['tables']['records']]
    require(all(kinds.count(kind) == count for kind,count in ( [('entity',700),('assertion',699),('observation',699)] if case == 'large_chain' else [('entity',6),('assertion',9),('observation',9)] )), 'canonical-fixture-cardinality')
    delta = result['delta']
    require(delta['before'] == snapshot_identity(before) and delta['after'] == snapshot_identity(after)
            and delta['revision_before'] == before['tables']['meta'][0][1]
            and delta['revision_after'] == after['tables']['meta'][0][1]
            and delta['new_events'] == after['tables']['events'][len(before['tables']['events']):], 'snapshot-identity-mismatch')
    require(request['workspace_revision'] == delta['revision_before']+1
            and delta['new_events'][0][1:3] == [request['workspace_revision'],'processing.graph_claim'], 'actual-capture-revision')
    bind_canonical(before,after,result)
    canonical_delta(before, after, delta['expected_mutable_records'], result['record_id'])
    # Restrict expected mutation IDs independently of the harness's allowed list.
    expected_mutable = [['processing_job', result['queued']['id']]]
    if case == 'ordinary_retry':
        e = result['exclusive']
        expected_mutable += [['processing_job', e['document_job']], ['collection_run', e['collection_job']]]
        require(result['competitors']['document']['id'] == e['document_job'] and result['competitors']['document']['state'] == 'blocked'
                and result['competitors']['collection']['id'] == e['collection_job']
                and result['competitors']['collection']['checkpoint']['state'] == 'blocked', 'competitor-terminal-resumption')
        require(result['synthetic_document_calls'] == result['synthetic_collection_calls'] == 1
                and e['document_calls'] == e['collection_calls'] == 0 and e['publication_attempts_before_retry'] == 1
                and e['request_sha256'] == o['request_identity']['sha256']
                and e['pending_revision'] == request['workspace_revision'] and e['job']['state'] == 'running',
                'exclusive-retry-interval')
    require(delta['expected_mutable_records'] == expected_mutable and delta['all_tables_checked'] is True,
            'mutation-scope')
    queued, finished = result['queued'], result['finished']
    require(queued['state'] == 'queued' and queued['attempt'] == finished['attempt'] == 1
            and queued['id'] == finished['id'] and queued['request_key'] == finished['request_key']
            and queued['input'] == finished['input'] and common.canonical_uuid(finished['lease'])
            and queued['input']['source_id'] == request['source_id'] and queued['input']['target_id'] == request['target_id'], 'job-binding')
    require(queued['input']['requested_revision'] == result['requested_revision']
            and queued['input']['queued_revision'] == result['requested_revision']+1, 'queued-revision')
    chain = [f'00000000-0000-4000-8000-{n:012}' for n in range(700)]
    if case == 'large_chain':
        require(64*1024 < len(req_raw) <= 1024**2 and request['nodes'] == chain
                and request['edges'] == [[chain[n], chain[n+1]] for n in range(699)]
                and request['source_id'] == chain[0] and request['target_id'] == chain[-1], 'genuine-large-capture')
    else:
        require(request['nodes'] == list('abcdef') and request['edges'] == [['a','b'],['a','d'],['b','c'],['c','e'],['d','e']]
                and request['source_id'] == 'a' and request['target_id'] == ('f' if case == 'unreachable' else 'c'), 'ordinary-capture')
    if case == 'live_cancel':
        require(finished['state'] == 'cancelled' and finished['failure'] == 'cancelled_by_analyst'
                and finished['cancellation_requested'] is True and finished['result_ids'] == []
                and result['record_id'] is None and not wrapper_raw and not graph_raw
                and o['live_observed_ms'] is not None and o['cancellation_seen_ms'] is not None
                and o['launch_ms'] <= o['live_observed_ms'] <= o['cancellation_seen_ms'] <= o['stop_ms']
                and o['post_inventory_verified'] is False, 'live-child-cancellation')
        require(delta['revision_after'] == request['workspace_revision']+2, 'cancelled-revision')
        return
    require(o['post_inventory_verified'] is True and finished['state'] == 'completed'
            and finished['result_ids'] == [result['record_id']], 'completed-graph')
    graph, wrapper = (json.loads(data,object_pairs_hook=common.inventory.unique_object) for data in (graph_raw,wrapper_raw))
    expected = {k: v for k, v in request.items() if k not in ('nodes', 'edges', 'source_id', 'target_id')}
    nodes = ['a', 'b', 'c'] if case == 'ordinary_retry' else [f'00000000-0000-4000-8000-{n:012}' for n in range(700)]
    expected['outcome'] = {'state': 'unreachable'} if case == 'unreachable' else {'state': 'path', 'nodes': nodes}
    require(strict_equal(graph,expected), 'exact-path-result')
    versions = common.read_json(common.ROOT/'workers/python/runtime_versions.json', 64*1024)
    expected_wrapper = {'schema_version':1,'recipe':'python-graph-job-v1','job_id':o['assignment_id'],
        'manifest_sha256':common.MANIFEST,'python_version':'3.13.15','isolated':True,'no_site':True,
        'no_bytecode':True,'verified_paths':True,'checks':{'versions':versions,'imported_modules':['networkx'],
        'backend_metadata_checked':True,'capture_nonce':o['capture_nonce'],'request_identity':o['request_identity'],
        'result_identity':o['result_identity']}}
    require(strict_equal(wrapper,expected_wrapper), 'exact-host-wrapper-binding')
    record = common.read_json(directory/'record.json', 16*1024**2)
    bind_canonical(before,after,result,record)
    require(record['id'] == result['record_id'] and record['job_id'] == finished['id']
            and record['request_key'] == finished['request_key'] and record['host_attempt_lease'] == finished['lease']
            and record['request_json'].encode() == req_raw and record['result_json'].encode() == graph_raw
            and record['captured_revision'] == request['workspace_revision']
            and record['published_revision'] == request['workspace_revision']+1
            and record['requested_revision'] == result['requested_revision']
            and record['queued_revision'] == result['requested_revision']+1, 'frozen-record-binding')
    require(record['frozen']['assertion_reviews'] == ({'accepted':699,'pending':0,'rejected':0,'deferred':0}
            if case == 'large_chain' else {'accepted':6,'pending':1,'rejected':1,'deferred':1}), 'review-denominators')
    if case == 'ordinary_retry':
        require([h['assertion_ids'] for h in record['outcome']['hops']] == [['r1','r1-parallel'],['r2']], 'frozen-hop-provenance')
