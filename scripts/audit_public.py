"""Scan the exact staged public files. A private exclusion list stays outside the repo."""
from pathlib import Path
import re,subprocess,sys
ROOT=Path(__file__).resolve().parents[1]
files=subprocess.check_output(['git','diff','--cached','--name-only','-z'],cwd=ROOT).decode().split('\0')
private=[]
if len(sys.argv)>1:private=[line.strip().casefold() for line in Path(sys.argv[1]).read_text().splitlines() if line.strip()]
patterns=[rb'/' + rb'Users/[^/\s]+/',rb'C:\\Users\\',rb'gh[pousr]_[A-Za-z0-9]{25,}',rb'-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----',rb'AKIA[0-9A-Z]{16}']
failures=[]
for name in filter(None,files):
    data=subprocess.check_output(['git','show',':'+name],cwd=ROOT)
    if b'\0' in data[:8000]:continue
    if any(re.search(p,data) for p in patterns):failures.append(name+': sensitive pattern')
    text=data.decode('utf-8','ignore').casefold()
    if any(word in text for word in private):failures.append(name+': private exclusion matched')
print('\n'.join(failures) if failures else f'Public audit passed for {len(list(filter(None,files)))} staged files')
if failures:raise SystemExit(1)
