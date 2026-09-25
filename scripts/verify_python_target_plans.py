#!/usr/bin/env python3
"""Validate fixed cross-target wheel metadata offline; never download or execute it."""
from __future__ import annotations

import argparse
import hashlib
import itertools
import json
from pathlib import Path
import re
import tomllib
from urllib.parse import urlsplit

import verify_runtime_bundle as inventory

ROOT = Path(__file__).resolve().parents[1]
LOCK_SHA = '5fe63f3b0597df585840b9e6757c87a76754b52aa1570cb401cb48113ac81eb4'
PROJECT_SHA = 'a6b11f5ca5d427d963be6f63dd62c4c40f602c14e3252447a04be109de0b5375'
# Deliberately closed reviewed spellings; this is not a general wheel-tag solver.
TARGETS = {
    'macos-x86_64': {
        'count': 58,
        'tags': {'cp313-cp313-macosx_10_12_x86_64', 'cp313-cp313-macosx_10_13_x86_64',
                 'cp313-cp313-macosx_10_13_universal2', 'cp313-cp313-macosx_12_0_x86_64',
                 'cp39-abi3-macosx_10_15_x86_64'},
    },
    'windows-x86_64': {
        'count': 60,
        'tags': {'cp313-cp313-win_amd64', 'cp39-abi3-win_amd64'},
    },
}
PURE = {'py2-none-any', 'py3-none-any'}
WINDOWS_MARKERS = {"sys_platform == 'win32'", "sys_platform == 'emscripten' or sys_platform == 'win32'"}
ROOT_PACKAGE = 'entity-workbench-analysis'


class PlanError(ValueError):
    pass


def require(value, message):
    if not value:
        raise PlanError(message)


def source_bytes(relative, expected, maximum):
    with (ROOT / relative).open('rb') as stream:
        raw = stream.read(maximum + 1)
    require(len(raw) <= maximum and hashlib.sha256(raw).hexdigest() == expected,
            'Reviewed source identity changed')
    return raw


def package_map(lock):
    packages = {}
    for package in lock['package']:
        require(package['name'] not in packages, 'Multiple locked versions need a reviewed recipe')
        packages[package['name']] = package
    require(ROOT_PACKAGE in packages, 'Project package absent from lock')
    return packages


def dependency_names(packages, target):
    require(target in TARGETS, 'Unreviewed target')
    pending, visited = [ROOT_PACKAGE], set()
    while pending:
        name = pending.pop()
        if name in visited:
            continue
        require(name in packages and len(visited) < 128, 'Missing or excessive locked dependency')
        visited.add(name)
        for dependency in packages[name].get('dependencies', []):
            require(set(dependency) in ({'name'}, {'name', 'marker'}), 'Unreviewed dependency shape')
            marker = dependency.get('marker')
            require(marker is None or marker in WINDOWS_MARKERS, 'Unreviewed dependency marker')
            if marker is None or target == 'windows-x86_64':
                pending.append(dependency['name'])
    return visited - {ROOT_PACKAGE}


def validate_plan(plan, packages, target):
    wanted = dependency_names(packages, target)
    require(type(plan) is list and len(plan) == len(wanted), 'Incomplete target dependency set')
    names, filenames = set(), set()
    total = 0
    for item in plan:
        require(type(item) is dict and set(item) == {'name', 'version', 'filename', 'bytes', 'sha256', 'url'},
                'Unexpected wheel metadata fields')
        name, version, filename = item['name'], item['version'], item['filename']
        require(type(name) is str and name in wanted and name not in names, 'Wrong or duplicate selected dependency')
        package = packages[name]
        require(package.get('source') == {'registry': 'https://pypi.org/simple'}
                and version == package['version'], 'Unreviewed package source or version')
        require(type(filename) is str and len(filename) <= 200 and '/' not in filename
                and '\\' not in filename and filename.endswith('.whl') and filename not in filenames,
                'Invalid or duplicate wheel filename')
        parts = filename[:-4].split('-')
        require(len(parts) == 5 and re.sub(r'[-_.]+', '-', parts[0]).lower() == name
                and parts[1] == version, 'Wheel filename identity mismatch')
        tags = {'-'.join(tag) for tag in itertools.product(*(part.split('.') for part in parts[2:]))}
        require(tags and tags <= TARGETS[target]['tags'] | PURE and tags != {'py2-none-any'},
                'Unreviewed interpreter, ABI or platform tags')
        require(type(item['bytes']) is int and 0 < item['bytes'] <= 64 * 1024**2
                and type(item['sha256']) is str and re.fullmatch('[0-9a-f]{64}', item['sha256']),
                'Invalid wheel size or hash')
        require(type(item['url']) is str and len(item['url']) <= 1024, 'Invalid wheel source URL')
        url = urlsplit(item['url'])
        require(url.scheme == 'https' and url.netloc == 'files.pythonhosted.org'
                and not url.query and not url.fragment and url.path.rsplit('/', 1)[-1] == filename,
                'Unreviewed wheel source URL')
        require(any(w.get('url') == item['url'] and w.get('size') == item['bytes']
                    and w.get('hash') == 'sha256:' + item['sha256'] for w in package.get('wheels', [])),
                'Wheel does not match locked identity')
        names.add(name)
        filenames.add(filename)
        total += item['bytes']
    require(names == wanted and total <= 512 * 1024**2, 'Incomplete or excessive target plan')
    return {'packages': len(names), 'archive_bytes_declared': total}


def verify(target):
    lock = tomllib.loads(source_bytes('workers/python/uv.lock', LOCK_SHA, 16 * 1024**2).decode())
    source_bytes('workers/python/pyproject.toml', PROJECT_SHA, 1024**2)
    path = ROOT / f'packaging/plans/python-{target}-wheels.v1.json'
    with path.open('rb') as stream:
        raw = stream.read(1024**2 + 1)
    require(len(raw) <= 1024**2, 'Target plan exceeds size bound')
    plan = json.loads(raw, object_pairs_hook=inventory.unique_object)
    result = validate_plan(plan, package_map(lock), target)
    require(result['packages'] == TARGETS[target]['count'], 'Reviewed dependency count changed')
    return dict(target=target, plan_sha256=hashlib.sha256(raw).hexdigest(), **result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', choices=tuple(TARGETS))
    args = parser.parse_args()
    try:
        results = [verify(target) for target in ([args.target] if args.target else TARGETS)]
        print(json.dumps({'schema_version': 1, 'verified': True, 'scope': 'locked wheel metadata only',
                          'targets': results, 'archives_downloaded': False, 'package_code_executed': False,
                          'runtime_components_satisfied': [], 'complete_release': False}, indent=2))
        return 0
    except (ValueError, OSError, KeyError, TypeError):
        print('Target wheel plan validation failed; no downloads or package execution occurred.')
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
