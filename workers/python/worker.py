"""Thin analytical adapters. Rust validates manifests, permissions and results.

This module never opens the canonical workspace or accesses the network. Running
it in an ordinary process does not isolate it: the native launcher must do that.
"""
import json
from pathlib import Path
import sys

MAX_MESSAGE=1024*1024

def execute(request):
    if request['protocol_version'] != 1:
        raise ValueError('Unsupported protocol')
    operation=request['operation']
    source=Path(request['inputs'][0])
    if source.is_absolute() or '..' in source.parts or source.is_symlink():
        raise ValueError('Invalid input')
    if source.stat().st_size > 64*1024*1024:
        raise ValueError('Input exceeds adapter limit')
    if operation == 'transaction_totals':
        import duckdb
        # Parameter binding is the only input to a fixed, reviewed query.
        with duckdb.connect(':memory:') as conn:
            conn.execute("SET enable_external_access=false")
            # Arrow reads only the job-authorised file; DuckDB external I/O is disabled.
            import pyarrow.parquet as pq
            table=pq.read_table(source)
            conn.register('transactions',table)
            rows=conn.execute("SELECT currency, sum(CAST(amount AS DECIMAL(38,8)))::VARCHAR AS net, list(id) AS transaction_ids FROM transactions WHERE review='accepted' AND transfer_peer IS NULL GROUP BY currency ORDER BY currency").fetchall()
            return {'totals':[{'currency':c,'net':n,'transaction_ids':ids} for c,n,ids in rows]}
    payload=json.loads(source.read_text())
    if operation == 'graph_paths':
        import networkx as nx
        graph=nx.Graph()
        for edge in payload['edges']:
            if edge['review']=='accepted':
                graph.add_edge(edge['subject_id'],edge['object_id'],assertion_id=edge['id'])
        try:
            path=nx.shortest_path(graph,payload['source'],payload['target'])
        except (nx.NetworkXNoPath,nx.NodeNotFound):
            return {'path':[],'assertion_ids':[]}
        return {'path':path,'assertion_ids':[graph[a][b]['assertion_id'] for a,b in zip(path,path[1:])]}
    if operation == 'candidate_mentions':
        import spacy
        from spacy.matcher import PhraseMatcher
        # No model downloads or generative model. Explicit analyst-supplied terms.
        nlp=spacy.blank('en');matcher=PhraseMatcher(nlp.vocab,attr='LOWER')
        matcher.add('selected_identifiers',[nlp.make_doc(t) for t in payload['terms']])
        doc=nlp.make_doc(payload['text'])
        return {'candidates':[{'text':doc[start:end].text,'start':doc[start].idx,'end':doc[end-1].idx+len(doc[end-1]),'review':'pending'} for _,start,end in matcher(doc)]}
    if operation == 'identity_candidates':
        # Splink linkage is deliberately not invoked without a reviewed, calibrated
        # settings model. A probability from untrained defaults is not evidence.
        return {'state':'blocked','reason':'A calibrated and versioned Splink model is required'}
    raise ValueError('Unsupported analytical recipe')

def main():
    try:
        raw=sys.stdin.buffer.read(MAX_MESSAGE+1)
        if len(raw)>MAX_MESSAGE:raise ValueError('Message too large')
        request=json.loads(raw)
        result=execute(request)
        data=json.dumps(result,allow_nan=False).encode()
        limit=request['limits']['output_bytes']
        if not 0<limit<=64*1024*1024 or len(data)>limit:raise ValueError('Output limit')
        output=Path(request['output'])
        if output.is_absolute() or '..' in output.parts or output.is_symlink():raise ValueError('Invalid output')
        with output.open('xb') as stream:stream.write(data)
        print(json.dumps({'protocol_version':1,'job_id':request['job_id'],'output':str(output),'bytes':len(data)}))
    except Exception:
        print(json.dumps({'protocol_version':1,'state':'failed','error':'Analysis operation failed'}))
        raise SystemExit(1)

if __name__=='__main__':main()
