"""Synthetic engine checks. These are not confinement or release tests."""
from pathlib import Path
import json,os,shutil,subprocess,tempfile,uuid
ROOT=Path(__file__).resolve().parents[1]
java=next((ROOT/'runtime/java').glob('*/Contents/Home/bin/java')).resolve()
classpath=str(ROOT/'workers/java/target/classes')+os.pathsep+str(ROOT/'workers/java/target/lib/*')
with tempfile.TemporaryDirectory(dir=ROOT/'artifacts') as directory:
    work=Path(directory)
    def run(worker,operation,source,output):
        request={'protocol_version':1,'job_id':str(uuid.uuid4()),'operation':operation,'inputs':[source],'output':output,'limits':{'output_bytes':1024*1024,'pages':10,'pixels':10_000_000}}
        result=subprocess.run([str(java),'-Xmx256m','-XX:-UsePerfData','-cp',classpath,'workbench.'+worker],input=json.dumps(request),text=True,capture_output=True,cwd=work,timeout=30)
        if result.returncode:raise RuntimeError(result.stdout+'\n'+result.stderr)
        return json.loads((work/output).read_text())
    shutil.copy(ROOT/'fixtures/brief.txt',work/'brief.txt')
    parsed=run('ParseWorker','parse','brief.txt','parsed.json')
    assert 'Rowan Ellis' in parsed['text']
    (work/'manifest.json').write_text(json.dumps({'workspace_revision':7,'documents':[{'id':'a','name':'Synthetic notice','text':parsed['text']},{'id':'b','name':'Other synthetic source','text':'Unrelated shipping log'}]}))
    indexed=run('SearchWorker','index','manifest.json','indexed.json');assert indexed['indexed']==2
    checks=[]
    for index,query in enumerate(['"Rowan Ellis"','Rowan AND Cooperative','"Rowan account"~20','Rowen~1','name:notice']):
        (work/'query.json').write_text(json.dumps({'query':query}))
        result=run('SearchWorker','search','query.json',f'result-{index}.json')
        assert result['workspace_revision']=='7'
        assert [h['id'] for h in result['hits']]==['a'],query
        checks.append({'query':query,'hits':1})
    report={'java':subprocess.check_output([str(java),'-version'],stderr=subprocess.STDOUT,text=True).splitlines()[0],'parse':'passed','lucene':checks,'isolation':'not tested by this script'}
    (ROOT/'artifacts/java-engine-results.json').write_text(json.dumps(report,indent=2));print(json.dumps(report,indent=2))
