"""Developer-only Maven bootstrap. Application distributions never run this script."""
from pathlib import Path
import hashlib, tarfile, urllib.request
ROOT=Path(__file__).resolve().parents[1]
VERSION='3.9.11'
base=f'https://repo.maven.apache.org/maven2/org/apache/maven/apache-maven/{VERSION}/apache-maven-{VERSION}-bin.tar.gz'
folder=ROOT/'artifacts'/'tools';folder.mkdir(parents=True,exist_ok=True)
archive=folder/'maven.tar.gz'
if not (folder/f'apache-maven-{VERSION}').exists():
    payload=urllib.request.urlopen(base,timeout=30).read()
    expected=urllib.request.urlopen(base+'.sha512',timeout=30).read().decode().split()[0]
    if hashlib.sha512(payload).hexdigest()!=expected:raise RuntimeError('Maven checksum mismatch')
    archive.write_bytes(payload)
    with tarfile.open(archive) as package:package.extractall(folder,filter='data')
print(folder/f'apache-maven-{VERSION}'/'bin'/'mvn')
