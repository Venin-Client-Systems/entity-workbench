"""Development probe of macOS Seatbelt, not a signed-helper release attestation."""
from pathlib import Path
import json,os,socket,subprocess,tempfile
ROOT=Path(__file__).resolve().parents[1]
java=next((ROOT/'runtime/java').glob('*/Contents/Home/bin/java')).resolve()
libs=(ROOT/'workers/java/target').resolve()
with tempfile.TemporaryDirectory(dir=ROOT/'artifacts') as folder:
    root=Path(folder).resolve();job=root/'job';job.mkdir();(job/'input.txt').write_text('allowed')
    other=root/'other-workspace.txt';other.write_text('sentinel')
    original=root/'original.txt';original.write_text('retained')
    listener=socket.socket();listener.bind(('127.0.0.1',0));listener.listen()
    quote=lambda s:json.dumps(str(s))
    profile='\n'.join(['(version 1)','(deny default)','(import "dyld-support.sb")','(allow process-fork)','(allow sysctl-read)','(allow file-read-metadata)',
        f'(allow process-exec (literal {quote(java)}))',
        '(allow file-read* (subpath "/System") (subpath "/private/var/db/dyld") (subpath "/private/preboot") (subpath "/System/Volumes/Preboot") (subpath "/usr/lib") (literal "/dev/random") (literal "/dev/urandom") (literal "/dev/null"))',
        f'(allow file-read* file-map-executable (subpath {quote(java.parents[1])}) (subpath {quote(libs)}) (subpath "/usr/lib") (subpath "/System"))',
        f'(allow file-read* (subpath {quote(job)}))',f'(allow file-write* (subpath {quote(job)}))'])
    profile_path=root/'worker.sb';profile_path.write_text(profile)
    cmd=[str(java),'-Xmx128m','-XX:-UsePerfData','-cp',str(libs/'classes')+os.pathsep+str(libs/'lib/*'),'workbench.HostileProbe',str(other),str(original),str(listener.getsockname()[1])]
    baseline=subprocess.run(cmd,cwd=job,capture_output=True,text=True,timeout=20)
    if baseline.returncode:raise RuntimeError(baseline.stderr)
    original.write_text('retained');(job/'output.txt').unlink(missing_ok=True)
    isolated=subprocess.run(['/usr/bin/sandbox-exec','-f',str(profile_path),*cmd],cwd=job,capture_output=True,text=True,timeout=20)
    result={'method':'development Seatbelt profile; signed helpers not validated','baseline':json.loads(baseline.stdout),'returncode':isolated.returncode,'stdout':isolated.stdout,'stderr':isolated.stderr[:2000]}
    if isolated.returncode==0:
        probe=json.loads(isolated.stdout);result['passed']=probe=={'other_workspace_read':False,'original_write':False,'direct_network':False,'job_io':True} and original.read_text()=='retained'
    else:result['passed']=False
    (ROOT/'artifacts/confinement-result.json').write_text(json.dumps(result,indent=2))
    print(json.dumps(result,indent=2))
    listener.close()
    if not result['passed']:raise SystemExit(1)
