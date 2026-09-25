#!/usr/bin/env python3
"""Static inspection of one reviewed Python layout; never install or import it."""
from __future__ import annotations

import argparse
from collections import Counter
import configparser
import hashlib
import json
import os
from pathlib import Path
import posixpath
import re
import stat
import subprocess
import sys
import zipfile

import stage_python_runtime as files
import stage_python_wheelhouse as wheels
import verify_runtime_bundle as inventory

RUNTIME_SHA = '6b4edce6755094b4f8393d5553da9a6968ac18b761035616430aaed7dbb0be95'
EXECUTABLE = 'install/bin/python3.13'
SITE = 'install/lib/python3.13/site-packages/'
HEADER = 'igraph-1.0.0.data/headers/igraphmodule_api.h'
MAX_FILE = 80 * 1024**2
MAX_NATIVE = 512 * 1024**2
MAX_EXPANDED = 512 * 1024**2
MAGIC = {bytes.fromhex(v) for v in ('feedface', 'cefaedfe', 'feedfacf', 'cffaedfe',
                                  'cafebabe', 'bebafeca', 'cafebabf', 'bfbafeca')}
LOADS = {'LC_LOAD_DYLIB', 'LC_LOAD_WEAK_DYLIB', 'LC_REEXPORT_DYLIB', 'LC_LOAD_UPWARD_DYLIB'}
require = files.require


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read_pinned(root, path, expected, maximum=MAX_FILE):
    with files.regular_at(root, path) as stream:
        before = os.fstat(stream.fileno())
        require(before.st_size <= maximum, 'Source size exceeds inspection bound')
        data = stream.read(maximum + 1)
        require(inventory.fingerprint(before) == inventory.fingerprint(os.fstat(stream.fileno())),
                'Source changed while inspecting')
    require(len(data) <= maximum and sha(data) == expected, 'Pinned source identity mismatch')
    return data


def proposed_location(member):
    inventory.path_key(member, 32)
    if member.split('/')[0].endswith('.data'):
        require(member == HEADER, 'Unreviewed wheel spread member')
        return 'install/include/python3.13/igraph/igraphmodule_api.h'
    return SITE + member


def entry_points(data):
    require(len(data) <= 1024**2, 'Entry-point metadata too large')
    parser = configparser.ConfigParser(interpolation=None, strict=True)
    parser.optionxform = str  # Entry-point names are case-sensitive.
    parser.read_string(data.decode('utf-8'))
    require(not parser.defaults(), 'Unexpected entry-point defaults')
    return {section: dict(parser[section]) for section in parser.sections()}


def load_commands(data):
    require(len(data) <= 2 * 1024**2, 'Static tool output exceeds bound')
    commands = []
    for block in re.split(r'(?m)^Load command [0-9]+\n', data.decode('utf-8'))[1:]:
        kind = re.search(r'(?m)^\s*cmd (LC_\S+)$', block)
        require(kind is not None, 'Malformed static tool load-command block')
        if kind[1] in LOADS | {'LC_ID_DYLIB', 'LC_RPATH'}:
            value = re.search(r'(?m)^\s*(?:name|path) (.*?) \(offset [0-9]+\)$', block)
            require(value is not None and 0 < len(value[1]) <= 1024
                    and all(32 <= ord(c) < 127 for c in value[1]), 'Invalid static loader string')
            commands.append({'command': kind[1], 'value': value[1]})
    require(commands, 'No understood loader commands')
    return commands


def expand(value, image):
    for prefix, base in (('@loader_path', posixpath.dirname(image)),
                         ('@executable_path', posixpath.dirname(EXECUTABLE))):
        if value == prefix or value.startswith(prefix + '/'):
            result = posixpath.normpath(base + value[len(prefix):])
            require(result.startswith('install/'), 'Loader path escapes proposed install tree')
            return result
    return None


def resolve(natives):
    locations = {path: item for item in natives for path in item['locations']}
    require(EXECUTABLE in locations, 'Fixed executable missing from native inventory')
    executable_rpaths = [x['value'] for x in locations[EXECUTABLE]['commands'] if x['command'] == 'LC_RPATH']
    edges, rpaths, identities = [], [], []
    for image, item in sorted(locations.items()):
        own = [x['value'] for x in item['commands'] if x['command'] == 'LC_RPATH']
        search = {expand(x, image) for x in own} | {expand(x, EXECUTABLE) for x in executable_rpaths}
        require(None not in search, 'Unreviewed run-path spelling')
        rpaths.extend({'image': image, 'rpath': value, 'expanded': expand(value, image)} for value in own)
        for command in item['commands']:
            value, kind = command['value'], command['command']
            if kind == 'LC_ID_DYLIB':
                identities.append({'image': image, 'identity': value})
            if kind not in LOADS:
                continue
            edge = {'image': image, 'command': kind, 'install_name': value}
            if value.startswith(('/usr/lib/', '/System/Library/')):
                edge['classification'] = 'os-library'
            else:
                direct = expand(value, image)
                candidates = {direct} if direct else set()
                if value.startswith('@rpath/'):
                    candidates = {posixpath.normpath(base + '/' + value[7:]) for base in search}
                present = sorted(candidates & locations.keys())
                edge['classification'] = 'local' if len(present) == 1 else ('ambiguous' if present else 'unresolved')
                edge['resolved_locations'] = present
            edges.append(edge)
    return {'executable': EXECUTABLE, 'edge_counts': dict(sorted(Counter(x['classification'] for x in edges).items())),
            'system_install_names': sorted({x['install_name'] for x in edges if x['classification'] == 'os-library'}),
            'rpaths': rpaths, 'library_identities': identities, 'edges': edges}


def run_tool(command, scratch, maximum):
    # Output goes to a temporary ordinary file rather than an unbounded PIPE.
    # Inputs are the exact reviewed bytes, not arbitrary application evidence.
    with (scratch / 'tool-output.tmp').open('x+b') as output:
        os.fchmod(output.fileno(), 0o600)
        try:
            result = subprocess.run(command, stdin=subprocess.DEVNULL, stdout=output,
                                    stderr=subprocess.DEVNULL, timeout=10, check=False)
            require(result.returncode == 0 and output.tell() <= maximum, 'Static tool failed or exceeded output bound')
            output.seek(0)
            return output.read(maximum + 1)
        finally:
            (scratch / 'tool-output.tmp').unlink()


def write_json(path, value):
    data = (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=True, allow_nan=False) + '\n').encode()
    require(len(data) <= 16 * 1024**2, 'Inspection report exceeds bound')
    with path.open('xb') as output:
        os.fchmod(output.fileno(), 0o600)
        output.write(data)
    return sha(data)


def inspect(runtime, wheelhouse, output):
    require(sys.platform == 'darwin', 'Actual static inspection requires macOS system tools')
    plan = wheels.selection()
    parent, name = files.parent_at(output)
    try:
        os.mkdir(name, 0o700, dir_fd=parent)
    finally:
        os.close(parent)
    # A fresh failed receipt remains even after success; only report.json is final.
    write_json(output / 'initial.json', {'static_inspection_complete': False, 'complete_release': False})
    scratch = output / 'native-copies'
    scratch.mkdir(mode=0o700)
    roots = []
    try:
        for path in (runtime, wheelhouse):
            parent, name = files.parent_at(path)
            try:
                roots.append(files.directory_at(parent, [name]))
            finally:
                os.close(parent)
        runtime_fd, wheel_fd = roots
        manifest_data = read_pinned(runtime_fd, 'manifest.json', RUNTIME_SHA, 2 * 1024**2)
        manifest = json.loads(manifest_data, object_pairs_hook=inventory.unique_object)
        expected = dict(manifest['files'])
        expected['manifest.json'] = {'bytes': len(manifest_data), 'sha256': RUNTIME_SHA, 'executable': False}
        files.verify_output(runtime_fd, expected)
        require(set(os.listdir(wheel_fd)) == {x['filename'] for x in plan}, 'Unexpected wheel input set')
        locations, keys, natives, hooks = {}, set(), {}, {'pth': [], 'entry_points': [], 'data': [], 'notice_files': 0}
        native_bytes = expanded = 0

        def add(location, origin, data):
            nonlocal native_bytes
            key = inventory.path_key(location, 32)
            require(key not in keys, 'Proposed cross-source or case/Unicode collision')
            keys.add(key)
            digest = sha(data)
            locations[location] = {'origin': origin, 'sha256': digest, 'bytes': len(data)}
            if data[:4] not in MAGIC:
                return
            if digest not in natives:
                native_bytes += len(data)
                require(native_bytes <= MAX_NATIVE, 'Native scratch bound exceeded')
                copy = scratch / (str(len(natives)).zfill(4) + '.bin')
                with copy.open('xb') as stream:
                    os.fchmod(stream.fileno(), 0o600)
                    stream.write(data)
                require(sha(copy.read_bytes()) == digest, 'Native scratch identity mismatch')
                arch = run_tool(['/usr/bin/lipo', '-archs', str(copy)], scratch, 512).decode('ascii').strip().split()
                require(arch and all(re.fullmatch('[A-Za-z0-9_]+', x) for x in arch) and 'arm64' in arch,
                        'Native image lacks arm64 slice')
                commands = load_commands(run_tool(['/usr/bin/otool', '-arch', 'arm64', '-l', str(copy)], scratch, 2 * 1024**2))
                natives[digest] = {'sha256': digest, 'bytes': len(data), 'locations': [], 'architectures': arch, 'commands': commands}
            natives[digest]['locations'].append(location)

        for path, spec in sorted(manifest['files'].items()):
            add(path, 'cpython-runtime', read_pinned(runtime_fd, path, spec['sha256']))
        for item in plan:
            with files.regular_at(wheel_fd, item['filename']) as stream:
                before = inventory.fingerprint(os.fstat(stream.fileno()))
                wheels.hash_wheel(stream, item)
                with zipfile.ZipFile(stream) as archive:
                    entries, seen = archive.infolist(), set()
                    require(len(entries) <= 20_000, 'Wheel member count exceeded')
                    for entry in entries:
                        path = entry.filename.rstrip('/')
                        key = inventory.path_key(path, 32)
                        require(key not in seen and entry.orig_filename == entry.filename, 'Duplicate or truncated ZIP path')
                        seen.add(key)
                        kind = stat.S_IFMT(entry.external_attr >> 16)
                        require(kind in ((0, stat.S_IFDIR) if entry.is_dir() else (0, stat.S_IFREG))
                                and not entry.flag_bits & 1 and entry.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED),
                                'Unsafe wheel member')
                        require(0 <= entry.file_size <= MAX_FILE, 'Wheel member inspection bound exceeded')
                        if entry.is_dir():
                            require(entry.file_size == 0, 'Nonempty directory')
                            continue
                        expanded += entry.file_size
                        require(expanded <= MAX_EXPANDED, 'Wheel expansion inspection bound exceeded')
                        with archive.open(entry) as member:
                            data = member.read(MAX_FILE + 1)
                            require(len(data) == entry.file_size and not member.read(1), 'Member length mismatch')
                        add(proposed_location(path), item['name'], data)
                        if wheels.notice_path(path):
                            hooks['notice_files'] += 1
                        common = {'wheel': item['filename'], 'path': path, 'bytes': len(data), 'sha256': sha(data)}
                        if path.endswith('.pth'):
                            require(len(data) <= 1024**2, 'Oversized pth metadata')
                            hooks['pth'].append(common | {'content': data.decode('utf-8')})
                        if path.endswith('.dist-info/entry_points.txt'):
                            hooks['entry_points'].append(common | {'primary': path.count('/') == 1, 'groups': entry_points(data)})
                        if path.split('/')[0].endswith('.data'):
                            hooks['data'].append(common | {'proposed_location': proposed_location(path)})
                stream.seek(0)
                wheels.hash_wheel(stream, item)
                require(before == inventory.fingerprint(os.fstat(stream.fileno())), 'Wheel source changed')
        # Include file-as-parent conflicts in the global proposed layout check.
        for path in locations:
            parts = path.split('/')
            require(not any(inventory.path_key('/'.join(parts[:i]), 32) in keys for i in range(1, len(parts))),
                    'Proposed file used as directory')
        require(not any(SITE + name in locations for name in ('site.py', 'sitecustomize.py', 'usercustomize.py')),
                'Unexpected site startup shadow')
        files.verify_output(runtime_fd, expected)
        native_list = sorted(natives.values(), key=lambda x: x['sha256'])
        mapping_sha = write_json(output / 'proposed-locations.json', locations)
        report = {'schema_version': 1, 'static_inspection_complete': True, 'complete_release': False,
                  'package_code_executed': False, 'runtime_layout_modified': False,
                  'runtime_manifest_sha256': RUNTIME_SHA, 'wheel_selection_sha256': wheels.PLAN_SHA,
                  'logical_files': len(locations), 'proposed_locations_sha256': mapping_sha,
                  'wheel_expanded_bytes': expanded, 'native_scratch_bytes': native_bytes,
                  'unique_native_files': len(native_list), 'native_logical_locations': sum(len(x['locations']) for x in native_list),
                  'hooks': hooks, 'natives': native_list, 'resolution': resolve(native_list)}
        write_json(output / 'report.json', report)
        return report
    finally:
        for fd in roots:
            os.close(fd)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runtime', type=Path, required=True)
    parser.add_argument('--wheelhouse', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        report = inspect(args.runtime, args.wheelhouse, args.output)
    except (OSError, ValueError, UnicodeError, configparser.Error, zipfile.BadZipFile, subprocess.SubprocessError, files.StagingError):
        print(json.dumps({'static_inspection_complete': False, 'complete_release': False, 'failure': 'static-preflight-failed'}))
        return 1
    print(json.dumps({k: report[k] for k in ('static_inspection_complete', 'complete_release', 'logical_files',
                                          'unique_native_files', 'native_logical_locations')} | {'edge_counts': report['resolution']['edge_counts']}))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
