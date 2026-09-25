#!/usr/bin/env python3
"""Verify and retain the reviewed offline wheel archives; never install/import them."""
from __future__ import annotations

import argparse
from email import policy
from email.parser import BytesParser
import hashlib
import itertools
import json
import os
from pathlib import Path
import re
import shutil
import stat
import sys
import tomllib
from urllib.parse import unquote, urlsplit
import zipfile
import zlib

import stage_python_runtime as files
import verify_runtime_bundle as inventory

ROOT = Path(__file__).resolve().parents[1]
PLAN_PATH = ROOT / 'packaging/plans/python-macos-arm64-wheels.v1.json'
PLAN_SHA = '983ac846a79e4985218b3f2582445abeac18de90fc3108c06fcb29b60928580e'
LOCK_SHA = '5fe63f3b0597df585840b9e6757c87a76754b52aa1570cb401cb48113ac81eb4'
PROJECT_SHA = 'a6b11f5ca5d427d963be6f63dd62c4c40f602c14e3252447a04be109de0b5375'
PACKAGE_COUNT = 58
CHUNK = 64 * 1024
MAX_WHEEL = 64 * 1024**2
MAX_MEMBER = 256 * 1024**2
MAX_EXPANDED = 512 * 1024**2
MAX_TOTAL = 2 * 1024**3
MAX_MEMBERS = 20_000
MAX_DEPTH = 32
MAX_METADATA = 4 * 1024**2
MAX_NOTICE = 4 * 1024**2
NOTICE = re.compile(r'^(?:licen[cs]es?|notice|copying|copyright|authors)(?:$|[._-])', re.I)
# This is a closed spelling check for the reviewed uv selection, not a tag solver.
COMPATIBLE = {
    'cp313-cp313-macosx_10_13_universal2', 'cp313-cp313-macosx_11_0_arm64',
    'cp313-cp313-macosx_12_0_arm64', 'cp39-abi3-macosx_11_0_arm64',
    'py3-none-any', 'py2-none-any',
}
UNMET = ('archives-not-installed', 'native-loader-closure-unverified', 'runtime-imports-unverified',
         'entry-points-and-pth-unreviewed', 'worker-confinement-unverified',
         'canonical-worker-not-enabled', 'complete-notices-unverified',
         'minimum-supported-os-not-established')
StagingError = files.StagingError
require = files.require


def normalized(value):
    require(isinstance(value, str) and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]*', value),
            'Invalid distribution name')
    return re.sub(r'[-_.]+', '-', value).lower()


def pinned_bytes(path, expected, maximum):
    parent, name = files.parent_at(path)
    try:
        with files.regular_at(parent, name) as stream:
            before = os.fstat(stream.fileno())
            data = stream.read(maximum + 1)
            require(inventory.fingerprint(os.fstat(stream.fileno())) == inventory.fingerprint(before),
                    'Reviewed source changed during reading')
    finally:
        os.close(parent)
    require(len(data) <= maximum and hashlib.sha256(data).hexdigest() == expected,
            'Reviewed selection, lock or project identity mismatch')
    return data


def filename_tags(filename, name, version):
    require(filename.endswith('.whl'), 'Only wheel artifacts are accepted')
    parts = filename[:-4].split('-')
    require(len(parts) == 5 and normalized(parts[0]) == name and parts[1] == version,
            'Wheel filename identity mismatch')
    tags = {'-'.join(t) for t in itertools.product(*(x.split('.') for x in parts[-3:]))}
    require(tags and tags <= COMPATIBLE and tags != {'py2-none-any'}, 'Unreviewed interpreter, ABI or platform tags')
    return tags, parts[0] + '-' + parts[1] + '.dist-info'


def selection():
    raw = pinned_bytes(PLAN_PATH, PLAN_SHA, 1024**2)
    plan = json.loads(raw, object_pairs_hook=inventory.unique_object)
    lock = tomllib.loads(pinned_bytes(ROOT / 'workers/python/uv.lock', LOCK_SHA, 16 * 1024**2).decode())
    pinned_bytes(ROOT / 'workers/python/pyproject.toml', PROJECT_SHA, 1024**2)
    require(isinstance(plan, list) and len(plan) == PACKAGE_COUNT, 'Production selection count mismatch')
    packages = {x['name']: x for x in lock['package']}
    names, filenames = set(), set()
    for item in plan:
        inventory.exact_keys(item, ('name', 'version', 'filename', 'bytes', 'sha256', 'url'), 'wheel selection')
        require(normalized(item['name']) == item['name'] and item['name'] not in names, 'Ambiguous distribution selection')
        require(isinstance(item['version'], str) and re.fullmatch(r'[0-9][A-Za-z0-9.+]*', item['version']), 'Invalid pinned version')
        files.safe_parts(item['filename'])
        require('/' not in item['filename'] and item['filename'] not in filenames, 'Ambiguous wheel selection')
        require(inventory.integer(item['bytes']) and 0 < item['bytes'] <= MAX_WHEEL
                and isinstance(item['sha256'], str) and inventory.HASH.fullmatch(item['sha256']), 'Invalid wheel pin')
        url = urlsplit(item['url'])
        require(url.scheme == 'https' and url.netloc == 'files.pythonhosted.org' and not url.query and not url.fragment
                and unquote(url.path.rsplit('/', 1)[-1]) == item['filename'], 'Unreviewed source URL')
        package = packages.get(item['name'], {})
        require(package.get('version') == item['version'] and any(
            x.get('url') == item['url'] and x.get('size') == item['bytes']
            and x.get('hash') == 'sha256:' + item['sha256'] for x in package.get('wheels', [])),
            'Selected artifact differs from the locked wheel')
        filename_tags(item['filename'], item['name'], item['version'])
        names.add(item['name'])
        filenames.add(item['filename'])
    return sorted(plan, key=lambda x: x['name'])


def hash_wheel(stream, item):
    require(os.fstat(stream.fileno()).st_size == item['bytes'], 'Wheel size mismatch')
    digest = hashlib.sha256()
    count = 0
    while block := stream.read(CHUNK):
        count += len(block)
        require(count <= item['bytes'], 'Wheel size mismatch')
        digest.update(block)
    require(count == item['bytes'] and digest.hexdigest() == item['sha256'], 'Wheel digest mismatch')
    stream.seek(0)


def header(data):
    require(len(data) <= MAX_METADATA and b'\x00' not in data, 'Invalid or oversized wheel metadata')
    message = BytesParser(policy=policy.default).parsebytes(data)
    require(not message.defects, 'Malformed wheel metadata')
    return message


def single(message, name):
    values = message.get_all(name, [])
    require(len(values) == 1, 'Missing or duplicate identity metadata')
    return str(values[0])


def notice_path(path):
    parts = path.split('/')
    return bool(NOTICE.match(parts[-1])) or any(part.lower() in ('licenses', 'licences') for part in parts[:-1])


def inspect_wheel(stream, item, root, retained, remaining):
    tags, info_root = filename_tags(item['filename'], item['name'], item['version'])
    primary = {info_root + '/' + name for name in ('METADATA', 'WHEEL', 'RECORD')}
    with zipfile.ZipFile(stream) as archive:
        entries = archive.infolist()
        require(0 < len(entries) <= MAX_MEMBERS, 'Wheel member count exceeded')
        keys, ordinary = set(), {}
        declared = 0
        for entry in entries:
            path = entry.filename[:-1] if entry.is_dir() else entry.filename
            key = inventory.path_key(path, MAX_DEPTH)
            require(key not in keys and entry.orig_filename == entry.filename, 'Duplicate or colliding ZIP member')
            keys.add(key)
            kind = stat.S_IFMT(entry.external_attr >> 16)
            require(kind in ((0, stat.S_IFDIR) if entry.is_dir() else (0, stat.S_IFREG)), 'ZIP links or special files forbidden')
            require(not entry.flag_bits & 1 and entry.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED),
                    'Encrypted or unsupported ZIP member')
            require(0 <= entry.file_size <= MAX_MEMBER, 'Wheel member size exceeded')
            if entry.is_dir():
                require(entry.file_size == 0 and entry.CRC == 0, 'Invalid ZIP directory')
            else:
                ordinary[path] = entry
                declared += entry.file_size
        require(declared <= MAX_EXPANDED and declared <= remaining, 'Expanded wheelhouse limit exceeded')
        ordinary_keys = {inventory.path_key(path, MAX_DEPTH) for path in ordinary}
        for path in ordinary.keys() | {x.filename.rstrip('/') for x in entries if x.is_dir()}:
            parts = path.split('/')
            require(not any(inventory.path_key('/'.join(parts[:i]), MAX_DEPTH) in ordinary_keys
                            for i in range(1, len(parts))), 'ZIP file used as a directory')
        require(primary <= set(ordinary), 'Primary wheel metadata missing')
        require({path.split('/')[0] for path in ordinary if path.split('/')[0].endswith('.dist-info')} == {info_root},
                'Ambiguous root distribution metadata')
        selected = primary | {path for path in ordinary if notice_path(path)}
        require(all(ordinary[path].file_size <= (MAX_METADATA if path in primary else MAX_NOTICE) for path in selected),
                'Wheel notice or metadata exceeds bound')
        metadata = {}
        expanded = 0
        for path, entry in ordinary.items():
            count, digest = 0, hashlib.sha256()
            destination = 'review/' + item['name'] + '/' + path if path in selected else None
            output = files.regular_at(root, destination, write=True) if destination else None
            blob = bytearray() if path in primary else None
            try:
                with archive.open(entry) as member:
                    while block := member.read(CHUNK):
                        count += len(block)
                        expanded += len(block)
                        require(count <= entry.file_size and expanded <= MAX_EXPANDED and expanded <= remaining,
                                'Actual expanded wheelhouse limit exceeded')
                        digest.update(block)
                        if output:
                            output.write(block)
                        if blob is not None:
                            blob.extend(block)
                require(count == entry.file_size, 'Truncated ZIP member')
                if output:
                    output.flush()
                    os.fsync(output.fileno())
                    retained[destination] = {'bytes': count, 'sha256': digest.hexdigest(), 'executable': False}
                if blob is not None:
                    metadata[path] = bytes(blob)
            finally:
                if output:
                    output.close()
        require(expanded == declared, 'Expanded byte count mismatch')
    meta = header(metadata[info_root + '/METADATA'])
    require(single(meta, 'Metadata-Version') in ('2.1', '2.4', '2.5'), 'Unreviewed metadata version')
    require(normalized(single(meta, 'Name')) == item['name'] and single(meta, 'Version') == item['version'],
            'Wheel distribution metadata mismatch')
    wheel = header(metadata[info_root + '/WHEEL'])
    require(single(wheel, 'Wheel-Version') == '1.0' and single(wheel, 'Root-Is-Purelib') in ('true', 'false'),
            'Unsupported wheel format')
    declared_tags = wheel.get_all('Tag', [])
    require(len(declared_tags) == len(set(declared_tags)) and set(declared_tags) == tags,
            'Wheel metadata compatibility tags mismatch')
    notices = sorted(selected - primary)
    for name in meta.get_all('License-File', []):
        inventory.path_key(str(name), MAX_DEPTH)
        candidates = {info_root + '/licenses/' + str(name), info_root + '/' + str(name)} & set(ordinary)
        require(len(candidates) == 1 and candidates <= set(notices), 'Missing or ambiguous declared licence file')
    return {'name': item['name'], 'version': item['version'], 'filename': item['filename'],
            'bytes': item['bytes'], 'sha256': item['sha256'], 'source_url': item['url'],
            'members': len(entries), 'expanded_bytes': expanded, 'zip_crc_verified': True,
            'primary_metadata': sorted(primary), 'notice_paths': notices,
            'installation_state': 'archives-only', 'runnable_engine': False}


def stage(input_path, destination):
    report = {'schema_version': 1, 'staged': False, 'complete_release': False,
              'installation_state': 'archives-only', 'package_code_executed': False,
              'runtime_components_satisfied': [], 'unmet_checks': list(UNMET),
              'failure': None, 'preceding_failure': None}
    parent = root = source = identity = None
    created = False
    try:
        require(os.name == 'posix' and hasattr(os, 'O_NOFOLLOW'), 'POSIX no-follow staging support required')
        plan = selection()
        input_parent, input_name = files.parent_at(input_path)
        try:
            source = files.directory_at(input_parent, [input_name])
        finally:
            os.close(input_parent)
        with os.scandir(source) as entries:
            actual = set()
            for entry in entries:
                require(len(actual) < PACKAGE_COUNT and entry.name not in actual, 'Unexpected wheelhouse input')
                info = entry.stat(follow_symlinks=False)
                require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1, 'Wheel inputs must be ordinary single-link files')
                actual.add(entry.name)
        require(actual == {x['filename'] for x in plan}, 'Missing, extra or alternate wheel input')
        parent, name = files.parent_at(destination)
        os.mkdir(name, 0o700, dir_fd=parent)
        created = True
        root = files.directory_at(parent, [name])
        identity = os.fstat(root)
        retained, observations, expanded = {}, [], 0
        for item in plan:
            with files.regular_at(source, item['filename']) as stream:
                before = os.fstat(stream.fileno())
                hash_wheel(stream, item)
                observed = inspect_wheel(stream, item, root, retained, MAX_TOTAL - expanded)
                expanded += observed['expanded_bytes']
                observations.append(observed)
                stream.seek(0)
                hash_wheel(stream, item)
                require(inventory.fingerprint(os.fstat(stream.fileno())) == inventory.fingerprint(before),
                        'Wheel changed during verification')
                path = 'wheels/' + item['filename']
                copied = files.copy_member(stream, root, path, item['bytes'], False)
                require(copied['sha256'] == item['sha256'] and not stream.read(1), 'Wheel changed during copying')
                retained[path] = copied
        provenance = {'schema_version': 1, 'selection_sha256': PLAN_SHA, 'uv_lock_sha256': LOCK_SHA,
                      'pyproject_sha256': PROJECT_SHA, 'resolver': 'reviewed uv 0.11.32 production closure',
                      'target': 'macos-aarch64', 'python': '3.13.15-standard-gil',
                      'wheel_selection_macos_context': '12.0', 'minimum_os_support_established': False,
                      'installation_state': 'archives-only', 'package_code_executed': False,
                      'runtime_components_satisfied': [], 'complete_release': False,
                      'unmet_checks': list(UNMET), 'archives': observations}
        retained['provenance.json'] = files.write_json(root, 'provenance.json', provenance)
        files.verify_output(root, retained)
        require(selection() == plan, 'Reviewed selection changed during staging')
        manifest = {'schema_version': 1, 'kind': 'locked-wheelhouse', 'installation_state': 'archives-only',
                    'runtime_components_satisfied': [], 'complete_release': False,
                    'unmet_checks': list(UNMET), 'files': retained}
        manifest_info = files.write_json(root, 'manifest.json', manifest)
        current = os.stat(name, dir_fd=parent, follow_symlinks=False)
        require((current.st_dev, current.st_ino) == (identity.st_dev, identity.st_ino), 'Destination changed')
        report.update(staged=True, archives=len(observations), expanded_bytes=expanded,
                      files=len(retained) + 1, manifest_sha256=manifest_info['sha256'], selection_sha256=PLAN_SHA)
    except (StagingError, inventory.InvalidInventory, OSError, ValueError, TypeError, KeyError,
            RecursionError, EOFError, zipfile.BadZipFile, zlib.error, NotImplementedError) as exc:
        report['failure'] = str(exc) if isinstance(exc, StagingError) else 'Unreadable or invalid wheelhouse input/output'
    finally:
        if created and not report['staged']:
            try:
                current = os.stat(name, dir_fd=parent, follow_symlinks=False)
                require(identity is not None and (current.st_dev, current.st_ino) == (identity.st_dev, identity.st_ino),
                        'Partial-output identity unavailable')
                shutil.rmtree(name, dir_fd=parent)
            except (OSError, StagingError):
                report['preceding_failure'] = report['failure']
                report['failure'] = 'Partial-output cleanup failed; destination retained for recovery'
        for fd in (root, parent, source):
            if fd is not None:
                os.close(fd)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--destination', type=Path, required=True)
    args = parser.parse_args()
    report = stage(args.input, args.destination)
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report['staged'] else 1


if __name__ == '__main__':
    sys.exit(main())
