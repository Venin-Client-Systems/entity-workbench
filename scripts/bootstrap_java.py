"""Developer-only download of a checksummed Java 21 JRE into ignored runtime storage."""
from pathlib import Path
import hashlib,json,tarfile,urllib.request,platform,subprocess
ROOT=Path(__file__).resolve().parents[1]
arch={'arm64':'aarch64','x86_64':'x64'}[platform.machine()]
assert platform.system()=='Darwin','This bootstrap currently targets macOS only'
manifest=ROOT/'runtime'/'java-download.json';manifest.parent.mkdir(exist_ok=True)
if manifest.exists():data=json.loads(manifest.read_text())
else:
    release=json.loads(subprocess.check_output(['gh','api','repos/adoptium/temurin21-binaries/releases/latest']))
    asset=next(a for a in release['assets'] if a['name'].startswith(f'OpenJDK21U-jre_{arch}_mac_hotspot_') and a['name'].endswith('.tar.gz'))
    link=asset['browser_download_url']
    checksum=urllib.request.urlopen(link+'.sha256.txt',timeout=30).read().decode().split()[0]
    data={'version':release['tag_name'],'package':{'link':link,'checksum':checksum},'license':'GPL-2.0-with-classpath-exception'}
    manifest.write_text(json.dumps(data,indent=2))
folder=ROOT/'runtime'/'java';folder.mkdir(exist_ok=True)
if not any(folder.iterdir()):
    archive=ROOT/'runtime'/'java.tar.gz'
    if not archive.exists():urllib.request.urlretrieve(data['package']['link'],archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest()!=data['package']['checksum']:raise RuntimeError('JRE checksum mismatch')
    with tarfile.open(archive) as package:package.extractall(folder,filter='data')
print(data['version'])
print(next(folder.glob('*/Contents/Home/bin/java')))
