#!/usr/bin/env python3
"""Offline inventory verification, not proof of executable behaviour or isolation."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import sys
import unicodedata

POLICY_PATH = Path(__file__).resolve().parents[1] / 'packaging/runtime-requirements.v1.json'
MAX_JSON_BYTES = 16 * 1024 * 1024
ID = re.compile(r'[a-z0-9][a-z0-9_-]{0,63}\Z')
VERSION = re.compile(r'[0-9][A-Za-z0-9._+\-]{0,127}\Z')
HASH = re.compile(r'[0-9a-f]{64}\Z')
RESERVED = re.compile(r'(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?\Z', re.IGNORECASE)


class InvalidInventory(ValueError):
    pass


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise InvalidInventory('Duplicate JSON member')
        result[key] = value
    return result


def read_json(path):
    with Path(path).open('rb') as stream:
        raw = stream.read(MAX_JSON_BYTES + 1)
    if len(raw) > MAX_JSON_BYTES:
        raise InvalidInventory('JSON exceeds 16 MiB limit')
    try:
        result = json.loads(raw, object_pairs_hook=unique_object)
    except (UnicodeError, json.JSONDecodeError, RecursionError) as exc:
        raise InvalidInventory('Invalid JSON encoding or structure') from exc
    return result, hashlib.sha256(raw).hexdigest()


def exact_keys(value, keys, context):
    if not isinstance(value, dict) or set(value) != set(keys):
        raise InvalidInventory(f'{context}: unexpected or missing fields')


def integer(value):
    return isinstance(value, int) and not isinstance(value, bool)


def identifiers(value, context):
    if not isinstance(value, list) or len(value) > 1000:
        raise InvalidInventory(f'{context}: expected bounded list')
    if any(not isinstance(v, str) or not ID.fullmatch(v) for v in value):
        raise InvalidInventory(f'{context}: invalid identifier')
    if len(set(value)) != len(value):
        raise InvalidInventory(f'{context}: duplicate identifier')
    return value


def path_key(value, depth):
    if not isinstance(value, str) or not value or len(value) > 1024:
        raise InvalidInventory('Invalid bundle-relative path')
    parts = value.split('/')
    if len(parts) > depth or any(p in ('', '.', '..') for p in parts):
        raise InvalidInventory('Unsafe path segments or excessive depth')
    if any(ord(c) < 32 or ord(c) == 127 or c in '\\:*?"<>|' for c in value):
        raise InvalidInventory('Non-portable or unsafe path characters')
    if any(p.endswith((' ', '.')) or RESERVED.fullmatch(p) for p in parts):
        raise InvalidInventory('Reserved filename')
    if PurePosixPath(value).is_absolute():
        raise InvalidInventory('Absolute path')
    return unicodedata.normalize('NFC', value).casefold()


def is_link(info):
    return stat.S_ISLNK(info.st_mode) or bool(
        getattr(info, 'st_file_attributes', 0) & getattr(stat, 'FILE_ATTRIBUTE_REPARSE_POINT', 0x400)
    )


def file_identity(info):
    return info.st_dev, info.st_ino, info.st_size, info.st_nlink


def fingerprint(info):
    return info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns


def verify(bundle_root, inventory_path, target):
    report = {'schema_version': 1, 'target': target, 'complete': False,
              'inventory_sha256': None, 'requirements_sha256': None,
              'missing_components': [], 'invalid_components': [],
              'verified_files': 0, 'verified_bytes': 0, 'errors': []}
    invalid = set()
    owners = {}
    error_limit = 200

    def error(code, detail, path=None, component=None):
        item = {'code': code, 'detail': detail}
        if path is not None:
            item['path'] = path
            invalid.update(owners.get(path, []))
        if component is not None:
            item['component'] = component
            invalid.add(component)
        if len(report['errors']) < error_limit:
            report['errors'].append(item)
        else:
            report['errors_truncated'] = True

    try:
        policy, report['requirements_sha256'] = read_json(POLICY_PATH)
        manifest, report['inventory_sha256'] = read_json(inventory_path)
        if target not in policy['targets']:
            raise InvalidInventory('Unsupported target')
        exact_keys(manifest, ('schema_version', 'target', 'ocr_languages', 'regions', 'components', 'files'), 'inventory')
        if not integer(manifest['schema_version']) or manifest['schema_version'] != 1:
            raise InvalidInventory('Unsupported inventory schema')
        if manifest['target'] != target:
            raise InvalidInventory('Inventory target does not match requested target')
        languages = identifiers(manifest['ocr_languages'], 'ocr_languages')
        regions = identifiers(manifest['regions'], 'regions')
        required = set(policy['common'] + policy['targets'][target])
        required.update('ocr-language:' + x for x in set(languages + policy['minimum_ocr_languages']))
        required.update('region:' + x for x in regions)
        if not set(policy['minimum_ocr_languages']).issubset(languages):
            error('missing_advertised_language', 'Initial English OCR support must be declared')
        limits = policy['limits']
        error_limit = limits['errors']
        components = manifest['components']
        files = manifest['files']
        if not isinstance(components, list) or len(components) > 2100:
            raise InvalidInventory('Expected bounded component list')
        if not isinstance(files, list) or len(files) > limits['files']:
            raise InvalidInventory('Expected bounded file list')
        declared = set()
        versions = {}
        for component in components:
            exact_keys(component, ('id', 'version', 'files'), 'component')
            name = component['id']
            if not isinstance(name, str) or name not in required:
                raise InvalidInventory('Unknown or unadvertised component')
            if name in declared:
                raise InvalidInventory('Duplicate component')
            declared.add(name)
            version = component['version']
            if not isinstance(version, str) or not VERSION.fullmatch(version):
                error('invalid_version', 'Expected a pinned version beginning with a digit', component=name)
            prefix = policy.get('version_prefixes', {}).get(name)
            if prefix and isinstance(version, str) and not version.startswith(prefix):
                error('incompatible_version', 'Component version is outside the supported runtime series', component=name)
            versions[name] = version
            paths = component['files']
            if not isinstance(paths, list) or not paths or len(paths) > limits['files']:
                error('component_files', 'Component must reference a bounded non-empty file list', component=name)
                continue
            seen = set()
            for path in paths:
                key = path_key(path, limits['path_depth'])
                if key in seen:
                    raise InvalidInventory('Duplicate component file reference')
                seen.add(key)
                owners.setdefault(path, set()).add(name)
        report['missing_components'] = sorted(required - declared)
        if report['missing_components']:
            error('missing_components', 'Required runtimes or advertised assets are absent')
        if 'duckdb' in versions and 'duckdb-spatial' in versions and versions['duckdb'] != versions['duckdb-spatial']:
            error('incompatible_version', 'DuckDB and Spatial versions must match', component='duckdb-spatial')
        for name in ('playwright-driver',):
            if name in versions and 'playwright' in versions and versions[name] != versions['playwright']:
                error('incompatible_version', 'Playwright and its driver versions must match', component=name)
        entries = {}
        keys = set()
        total = 0
        for entry in files:
            exact_keys(entry, ('path', 'size', 'sha256'), 'file')
            path = entry['path']
            key = path_key(path, limits['path_depth'])
            if key in keys:
                raise InvalidInventory('Duplicate or case/Unicode-colliding file paths')
            keys.add(key)
            if not integer(entry['size']) or not 0 <= entry['size'] <= limits['file_bytes']:
                raise InvalidInventory('Invalid file size')
            if not isinstance(entry['sha256'], str) or not HASH.fullmatch(entry['sha256']):
                raise InvalidInventory('Invalid SHA-256')
            total += entry['size']
            if total > limits['total_bytes']:
                raise InvalidInventory('Declared bundle exceeds total size limit')
            entries[path] = entry
            if path not in owners:
                error('unowned_file', 'Every file must belong to a declared component', path=path)
        for path in sorted(owners.keys() - entries.keys()):
            error('missing_file_declaration', 'Component references an undeclared file', path=path)
        root = Path(bundle_root)
        root_info = root.lstat()
        if is_link(root_info) or not stat.S_ISDIR(root_info.st_mode):
            raise InvalidInventory('Bundle root must be a real directory')
        actual = {}
        pending = [root]
        visited = 0
        actual_keys = set()
        while pending:
            directory = pending.pop()
            with os.scandir(directory) as children:
                for child in children:
                    visited += 1
                    if visited > limits['files'] * 2:
                        raise InvalidInventory('Bundle exceeds entry limit')
                    path = Path(child.path).relative_to(root).as_posix()
                    key = path_key(path, limits['path_depth'])
                    if key in actual_keys:
                        error('path_collision', 'Bundle paths collide across supported filesystems', path=path)
                    actual_keys.add(key)
                    # DirEntry.stat reports zero link/identity fields on Windows.
                    # Read real metadata without following reparse points.
                    info = os.stat(child.path, follow_symlinks=False)
                    if is_link(info):
                        error('unsafe_link', 'Symlinks and reparse points are forbidden', path=path)
                    elif stat.S_ISDIR(info.st_mode):
                        pending.append(Path(child.path))
                    elif not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
                        error('unsafe_file', 'Only regular, non-hardlinked files are allowed', path=path)
                    else:
                        actual[path] = info
        for path in sorted(actual.keys() - entries.keys()):
            error('unlisted_file', 'File is absent from inventory', path=path)
        for path, entry in entries.items():
            before = actual.get(path)
            if before is None:
                error('missing_file', 'Declared regular file is absent', path=path)
                continue
            if before.st_size != entry['size']:
                error('size_mismatch', 'File size differs from inventory', path=path)
                continue
            try:
                digest = hashlib.sha256()
                flags = os.O_RDONLY | getattr(os, 'O_BINARY', 0) | getattr(os, 'O_NOFOLLOW', 0)
                with os.fdopen(os.open(root / path, flags), 'rb') as stream:
                    opened = os.fstat(stream.fileno())
                    if file_identity(opened) != file_identity(before):
                        error('file_changed', 'File changed before hashing', path=path)
                        continue
                    count = 0
                    while block := stream.read(1024 * 1024):
                        count += len(block)
                        if count > entry['size']:
                            break
                        digest.update(block)
                    # Compare timestamps from the same descriptor API: Windows
                    # path queries and handle queries can expose different times.
                    changed = fingerprint(os.fstat(stream.fileno())) != fingerprint(opened)
                    changed |= file_identity((root / path).lstat()) != file_identity(opened)
                if changed or count != entry['size']:
                    error('file_changed', 'File changed during hashing', path=path)
                elif digest.hexdigest() != entry['sha256']:
                    error('hash_mismatch', 'SHA-256 differs from inventory', path=path)
                else:
                    report['verified_files'] += 1
                    report['verified_bytes'] += count
            except OSError:
                error('file_read_error', 'Could not safely read file', path=path)
    except (InvalidInventory, OSError, ValueError, TypeError, KeyError, RecursionError) as exc:
        # Do not expose absolute build paths or operating-system exception messages.
        detail = str(exc) if isinstance(exc, InvalidInventory) else 'Unreadable inventory, policy or bundle'
        error('invalid_inventory', detail)
    report['invalid_components'] = sorted(invalid)
    report['complete'] = not report['errors']
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', required=True, type=Path)
    parser.add_argument('--inventory', required=True, type=Path)
    parser.add_argument('--target', required=True, choices=('windows-x86_64', 'macos-aarch64', 'macos-x86_64'))
    args = parser.parse_args()
    report = verify(args.bundle, args.inventory, args.target)
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report['complete'] else 1


if __name__ == '__main__':
    sys.exit(main())
