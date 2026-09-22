"""Create public component inventories from lockfiles; never export local build paths."""
from pathlib import Path
import json,tomllib,uuid,shutil
ROOT=Path(__file__).resolve().parents[1]
out=ROOT/'sbom';out.mkdir(exist_ok=True)
def write(name,components):
    document={'bomFormat':'CycloneDX','specVersion':'1.6','serialNumber':'urn:uuid:'+str(uuid.uuid4()),'version':1,'metadata':{'component':{'type':'application','name':'Entity Workbench','version':'0.1.0'}},'components':components}
    (out/name).write_text(json.dumps(document,indent=2)+'\n')
cargo=tomllib.loads((ROOT/'Cargo.lock').read_text())
write('rust.cdx.json',[{'type':'library','name':p['name'],'version':p['version'],'purl':f"pkg:cargo/{p['name']}@{p['version']}",**({'hashes':[{'alg':'SHA-256','content':p['checksum']}]} if 'checksum' in p else {})} for p in cargo['package']])
node=json.loads((ROOT/'package-lock.json').read_text())
write('javascript.cdx.json',[{'type':'library','name':key.rsplit('node_modules/',1)[-1],'version':value['version'],'purl':f"pkg:npm/{key.rsplit('node_modules/',1)[-1]}@{value['version']}",**({'licenses':[{'expression':value['license']}]} if value.get('license') else {})} for key,value in node['packages'].items() if key and 'version' in value])
python=tomllib.loads((ROOT/'workers/python/uv.lock').read_text())
write('python.cdx.json',[{'type':'library','name':p['name'],'version':p['version'],'purl':f"pkg:pypi/{p['name']}@{p['version']}"} for p in python['package'] if 'version' in p])
java=ROOT/'workers/java/target/bom.json'
if java.exists():shutil.copy(java,out/'java.cdx.json')
print('Generated dependency inventories; distribution licence review remains required.')
