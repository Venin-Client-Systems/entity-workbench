import json
import worker

def test_paths_only_use_accepted_assertions(tmp_path,monkeypatch):
    monkeypatch.chdir(tmp_path)
    data={'source':'a','target':'c','edges':[{'id':'ab','subject_id':'a','object_id':'b','review':'accepted'},{'id':'bc','subject_id':'b','object_id':'c','review':'accepted'},{'id':'ac','subject_id':'a','object_id':'c','review':'pending'}]}
    (tmp_path/'graph.json').write_text(json.dumps(data))
    assert worker.execute({'protocol_version':1,'operation':'graph_paths','inputs':['graph.json']})=={'path':['a','b','c'],'assertion_ids':['ab','bc']}

def test_exact_analytical_totals_and_drillthrough(tmp_path,monkeypatch):
    import pyarrow as pa
    import pyarrow.parquet as pq
    monkeypatch.chdir(tmp_path)
    pq.write_table(pa.table({'id':['a','b','c','d'],'amount':['0.10','0.20','999','-1.00'],'currency':['AUD','AUD','AUD','USD'],'review':['accepted','accepted','pending','accepted'],'transfer_peer':pa.array([None]*4,pa.string())}),tmp_path/'transactions.parquet')
    result=worker.execute({'protocol_version':1,'operation':'transaction_totals','inputs':['transactions.parquet']})
    assert result['totals'][0]=={'currency':'AUD','net':'0.30000000','transaction_ids':['a','b']}
    assert result['totals'][1]['net']=='-1.00000000'

def test_candidate_mentions_are_not_identity_decisions(tmp_path,monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path/'text.json').write_text(json.dumps({'text':'Rowan Ellis visited. A second Rowan Ellis is unrelated.','terms':['Rowan Ellis']}))
    result=worker.execute({'protocol_version':1,'operation':'candidate_mentions','inputs':['text.json']})
    assert len(result['candidates'])==2
    assert all(c['review']=='pending' for c in result['candidates'])
