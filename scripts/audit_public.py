"""Scan exact surviving staged blobs; exclusions stay outside the repository."""
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
PATTERNS = [
    rb'/' + rb'Users/[^/\s]+/',
    rb'C:\\Users\\',
    rb'gh[pousr]_[A-Za-z0-9]{25,}',
    rb'-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----',
    rb'AKIA[0-9A-Z]{16}',
]


def main():
    # Lowercase d excludes only deletions. Adds, modifications, rename
    # destinations and type changes still use their exact staged index blobs.
    # Other unresolved Git states fail when their index blob cannot be read.
    names = subprocess.check_output(
        ['git', 'diff', '--cached', '--name-only', '--diff-filter=d', '-z'],
        cwd=ROOT,
    ).decode().split('\0')
    files = [name for name in names if name]
    private = []
    if len(sys.argv) > 1:
        private = [
            line.strip().casefold()
            for line in Path(sys.argv[1]).read_text().splitlines()
            if line.strip()
        ]
    failures = []
    for name in files:
        data = subprocess.check_output(['git', 'show', ':' + name], cwd=ROOT)
        if b'\0' in data[:8000]:
            continue
        if any(re.search(pattern, data) for pattern in PATTERNS):
            failures.append(name + ': sensitive pattern')
        text = data.decode('utf-8', 'ignore').casefold()
        if any(word in text for word in private):
            failures.append(name + ': private exclusion matched')
    if failures:
        print('\n'.join(failures))
        return 1
    print(f'Public audit passed for {len(files)} staged files')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
