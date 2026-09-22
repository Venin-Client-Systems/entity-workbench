"""Stage app-local Java and Lucene. This is not the complete release runtime bundle."""
from pathlib import Path
import shutil,json
ROOT=Path(__file__).resolve().parents[1]
stage=ROOT/'runtime'/'staged'/'engines';stage.mkdir(parents=True,exist_ok=True)
java=next((ROOT/'runtime/java').glob('*/Contents/Home'))
shutil.copytree(java,stage/'java',dirs_exist_ok=True,symlinks=False)
search=stage/'search';(search/'lib').mkdir(parents=True,exist_ok=True)
shutil.copy(ROOT/'workers/java/target/workers-0.1.0.jar',search)
for path in (ROOT/'workers/java/target/lib').glob('*.jar'):
    if path.name.startswith(('lucene-','jackson-')):shutil.copy(path,search/'lib')
(stage/'development-only.json').write_text(json.dumps({'complete_release':False,'included':['Java 21','Lucene search adapter'],'missing':['parser and OCR distribution','Python distribution','browser capture distribution','Spatial extension','regional data packs','signed sandbox helpers']}))
print('Staged Java and Lucene for macOS development bundle')
