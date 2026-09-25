#!/usr/bin/env python3
"""Independently verify an assembled, unexecuted Python development prefix."""
from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import posixpath
import stat

import stage_python_runtime as files
import verify_runtime_bundle as inventory

PLAN_SHA = '849ad7b775879f7e8f213af11f1421ad75d30019ed6aab3c35e047a8a50b9b59'
RUNTIME_SHA = '6b4edce6755094b4f8393d5553da9a6968ac18b761035616430aaed7dbb0be95'
WHEEL_SHA = '983ac846a79e4985218b3f2582445abeac18de90fc3108c06fcb29b60928580e'
SITE = 'install/lib/python3.13/site-packages'
WHEEL_COUNT = 58
MAX_FILES = 20_000
MAX_ENTRIES = 40_000
MAX_FILE = 80 * 1024**2
MAX_TOTAL = 768 * 1024**2
MAX_METADATA = 16 * 1024**2
require = files.require


def read_bytes(root, name, maximum=MAX_METADATA):
    with files.regular_at(root, name) as stream:
        before = inventory.fingerprint(os.fstat(stream.fileno()))
        require(os.fstat(stream.fileno()).st_size <= maximum, 'Metadata exceeds bound')
        data = stream.read(maximum + 1)
        require(len(data) <= maximum and before == inventory.fingerprint(os.fstat(stream.fileno())),
                'Metadata changed during reading')
    return data


def record_name(path):
    files.safe_parts(path)
    return posixpath.relpath(path, SITE)


def record_hash(digest):
    require(isinstance(digest, str) and inventory.HASH.fullmatch(digest), 'Invalid record digest')
    return 'sha256=' + base64.urlsafe_b64encode(bytes.fromhex(digest)).decode().rstrip('=')


def validate_record(data, record_path, members, entries):
    require(len(data) <= MAX_METADATA, 'Installed RECORD exceeds bound')
    expected = {record_name(path): path for path in members}
    require(record_path in members and len(expected) == len(members), 'Incomplete or duplicate record ownership')
    actual = set()
    for row in csv.reader(io.StringIO(data.decode('utf-8'), newline=''), strict=True):
        require(len(row) == 3 and row[0] in expected and row[0] not in actual, 'Unknown or duplicate installed RECORD row')
        actual.add(row[0])
        path = expected[row[0]]
        wanted = ['', ''] if path == record_path else [record_hash(entries[path]['sha256']), str(entries[path]['bytes'])]
        require(row[1:] == wanted, 'Installed RECORD digest or size mismatch')
    require(actual == set(expected), 'Installed RECORD omits a file')


def verify_root(root, expected_manifest_sha256):
    require(isinstance(expected_manifest_sha256, str) and inventory.HASH.fullmatch(expected_manifest_sha256),
            'Expected manifest SHA-256 required')
    raw = read_bytes(root, 'manifest.json')
    require(hashlib.sha256(raw).hexdigest() == expected_manifest_sha256, 'Manifest identity mismatch')
    manifest = json.loads(raw, object_pairs_hook=inventory.unique_object)
    inventory.exact_keys(manifest, ('schema_version', 'kind', 'installation_state', 'complete_release',
        'runtime_components_satisfied', 'package_code_executed', 'installation_plan_sha256',
        'runtime_manifest_sha256', 'wheel_selection_sha256', 'files', 'record_members'), 'Python installation manifest')
    require(manifest['schema_version'] == 1 and type(manifest['schema_version']) is int
            and manifest['kind'] == 'python-development-prefix' and manifest['installation_state'] == 'assembled-unexecuted'
            and manifest['complete_release'] is False and manifest['package_code_executed'] is False
            and manifest['runtime_components_satisfied'] == [] and manifest['installation_plan_sha256'] == PLAN_SHA
            and manifest['runtime_manifest_sha256'] == RUNTIME_SHA and manifest['wheel_selection_sha256'] == WHEEL_SHA,
            'Unsupported or misleading installation manifest')
    entries, keys = manifest['files'], set()
    require(isinstance(entries, dict) and 0 < len(entries) < MAX_FILES and 'manifest.json' not in entries,
            'Invalid installation file inventory')
    total = len(raw)
    for name, expected in entries.items():
        key = inventory.path_key(name, 16)
        require(key not in keys, 'Colliding declared files')
        keys.add(key)
        inventory.exact_keys(expected, ('bytes', 'sha256', 'executable'), 'file')
        require(inventory.integer(expected['bytes']) and 0 <= expected['bytes'] <= MAX_FILE
                and isinstance(expected['sha256'], str) and inventory.HASH.fullmatch(expected['sha256'])
                and type(expected['executable']) is bool, 'Invalid declared file identity')
        total += expected['bytes']
    require(total <= MAX_TOTAL, 'Installation size exceeds bound')
    expected_files = entries | {'manifest.json': {'bytes': len(raw), 'sha256': expected_manifest_sha256, 'executable': False}}
    actual, pending, count = set(), [''], 0
    while pending:
        relative = pending.pop()
        directory = files.directory_at(root, relative.split('/') if relative else [])
        try:
            with os.scandir(directory) as children:
                for child in children:
                    count += 1
                    require(count <= MAX_ENTRIES, 'Installation entry bound exceeded')
                    name = relative + '/' + child.name if relative else child.name
                    files.safe_parts(name)
                    info = child.stat(follow_symlinks=False)
                    if stat.S_ISDIR(info.st_mode):
                        pending.append(name)
                    else:
                        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1, 'Linked or special installation file')
                        actual.add(name)
        finally:
            os.close(directory)
    require(actual == set(expected_files), 'Missing or unexpected installation file')
    for name, expected in expected_files.items():
        with files.regular_at(root, name) as stream:
            before = os.fstat(stream.fileno())
            require(before.st_size == expected['bytes'] and stat.S_IMODE(before.st_mode) == (0o755 if expected['executable'] else 0o644),
                    'Installed size or permissions mismatch')
            digest, size = hashlib.sha256(), 0
            while block := stream.read(files.CHUNK):
                size += len(block)
                require(size <= expected['bytes'], 'Installed file grew during hashing')
                digest.update(block)
            require(size == expected['bytes'] and digest.hexdigest() == expected['sha256']
                    and inventory.fingerprint(before) == inventory.fingerprint(os.fstat(stream.fileno())),
                    'Installed file changed or hash mismatch')
    records, owned = manifest['record_members'], set()
    require(isinstance(records, dict) and len(records) == WHEEL_COUNT, 'Installed distribution count mismatch')
    for record_path, members in records.items():
        relative = record_path.removeprefix(SITE + '/')
        require(record_path.startswith(SITE + '/') and relative.count('/') == 1
                and relative.endswith('.dist-info/RECORD') and isinstance(members, list) and 0 < len(members) <= MAX_FILES,
                'Invalid distribution RECORD ownership')
        require(len(set(members)) == len(members) and set(members) <= entries.keys() and not owned.intersection(members),
                'Invalid or overlapping distribution files')
        for name in members:
            require(name.startswith(SITE + '/') or name == 'install/include/python3.13/igraph/igraphmodule_api.h',
                    'Unreviewed installed wheel destination')
        validate_record(read_bytes(root, record_path), record_path, members, entries)
        owned.update(members)
    return {'verified': True, 'installation_state': 'assembled-unexecuted', 'complete_release': False,
            'runtime_components_satisfied': [], 'package_code_executed': False, 'files': len(expected_files),
            'bytes': total, 'wheels': len(records), 'manifest_sha256': expected_manifest_sha256}


def verify(path, expected_manifest_sha256):
    parent = root = None
    try:
        require(os.name == 'posix', 'POSIX no-follow verification required')
        parent, name = files.parent_at(path)
        root = files.directory_at(parent, [name])
        before = os.fstat(root)
        report = verify_root(root, expected_manifest_sha256)
        after = os.stat(name, dir_fd=parent, follow_symlinks=False)
        require((before.st_dev, before.st_ino) == (after.st_dev, after.st_ino), 'Installation root changed')
        return report
    except (OSError, ValueError, TypeError, KeyError, RecursionError, csv.Error):
        return {'verified': False, 'complete_release': False, 'failure': 'Installation verification failed'}
    finally:
        for fd in (root, parent):
            if fd is not None:
                os.close(fd)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prefix', type=Path, required=True)
    parser.add_argument('--expected-manifest-sha256', required=True)
    args = parser.parse_args()
    report = verify(args.prefix, args.expected_manifest_sha256)
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report['verified'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
