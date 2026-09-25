#!/usr/bin/env python3
"""Assemble one reviewed Python prefix offline, without executing its contents."""
from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import stat
import zipfile
import zlib

import inspect_python_native_layout as layout
import stage_python_runtime as files
import stage_python_wheelhouse as wheels
import verify_python_install as verifier
import verify_runtime_bundle as inventory

ROOT = Path(__file__).resolve().parents[1]
PLAN_PATH = ROOT / 'packaging/plans/python-install-macos-arm64.v1.json'
MAX_WHEEL_EXPANDED = 512 * 1024**2
require = files.require
UNMET = ['interpreter-and-packages-not-executed', 'native-loader-behaviour-unverified',
         'required-plugin-behaviour-unverified', 'isolated-launch-bootstrap-not-staged',
         'worker-confinement-unverified', 'canonical-worker-not-enabled',
         'signing-and-relocation-unverified', 'minimum-supported-os-not-established',
         'complete-notices-unverified', 'upstream-metadata-references-absent-LICENSE.zlib-ng.txt']


def installation_plan():
    raw = wheels.pinned_bytes(PLAN_PATH, verifier.PLAN_SHA, 1024**2)
    return raw, json.loads(raw, object_pairs_hook=inventory.unique_object)


class FileInventory:
    def __init__(self):
        self.entries, self.keys, self.parents = {}, set(), set()
        self.total = 0

    def check_path(self, path):
        key = inventory.path_key(path, 16)
        parts = path.split('/')
        parents = {inventory.path_key('/'.join(parts[:i]), 16) for i in range(1, len(parts))}
        require(key not in self.keys and key not in self.parents and not parents.intersection(self.keys),
                'Cross-source path collision or file-as-parent conflict')
        return key, parents

    def add(self, path, info):
        key, parents = self.check_path(path)
        require(len(self.entries) + 1 < verifier.MAX_FILES and 0 <= info['bytes'] <= verifier.MAX_FILE,
                'Installation file bound exceeded')
        self.total += info['bytes']
        require(self.total <= verifier.MAX_TOTAL, 'Installation total size exceeded')
        self.entries[path] = info
        self.keys.add(key)
        self.parents.update(parents)

    def copy(self, stream, root, path, size, executable=False):
        self.check_path(path)
        require(len(self.entries) + 1 < verifier.MAX_FILES and 0 <= size <= verifier.MAX_FILE
                and self.total + size <= verifier.MAX_TOTAL,
                'Installation copy size exceeded')
        info = files.copy_member(stream, root, path, size, executable)
        require(not stream.read(1), 'Source has unexpected trailing bytes')
        self.add(path, info)
        return info

    def blob(self, data, root, path):
        return self.copy(io.BytesIO(data), root, path, len(data))

    def json(self, value, root, path):
        data = (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=True, allow_nan=False) + '\n').encode()
        require(len(data) <= verifier.MAX_METADATA, 'Generated metadata exceeds bound')
        return self.blob(data, root, path)


def archive_record(data, record_path, members):
    require(len(data) <= verifier.MAX_METADATA, 'Archive RECORD exceeds bound')
    observed = set()
    for row in csv.reader(io.StringIO(data.decode('utf-8'), newline=''), strict=True):
        require(len(row) == 3 and row[0] in members and row[0] not in observed, 'Unknown or duplicate archive RECORD row')
        observed.add(row[0])
        info = members[row[0]]
        expected = ['', ''] if row[0] == record_path else [verifier.record_hash(info['sha256']), str(info['bytes'])]
        require(row[1:] == expected, 'Archive RECORD digest or size mismatch')
    require(observed == set(members), 'Archive RECORD omits a member')


def installed_record(record_path, members, entries):
    text = io.StringIO(newline='')
    writer = csv.writer(text, lineterminator='\n')
    for path in sorted(members, key=verifier.record_name):
        suffix = ['', ''] if path == record_path else [verifier.record_hash(entries[path]['sha256']), str(entries[path]['bytes'])]
        writer.writerow([verifier.record_name(path), *suffix])
    data = text.getvalue().encode('utf-8')
    require(len(data) <= verifier.MAX_METADATA, 'Installed RECORD exceeds bound')
    return data


def install_wheel(stream, item, root, book, remaining):
    # Reuse the reviewed hash/ZIP/identity/licence validator. It retains original
    # RECORD/METADATA/WHEEL and notice bytes separately under review/<package>.
    retained = {}
    observed = wheels.inspect_wheel(stream, item, root, retained, remaining)
    for path, info in retained.items():
        book.add(path, info)
    _, info_root = wheels.filename_tags(item['filename'], item['name'], item['version'])
    record_path = info_root + '/RECORD'
    installed_path = layout.proposed_location(record_path)
    source_members, mapping, record_data = {}, {}, None
    with zipfile.ZipFile(stream) as archive:
        for entry in archive.infolist():
            if entry.is_dir():
                continue
            path = entry.filename
            destination = layout.proposed_location(path)
            require(entry.file_size <= verifier.MAX_FILE and not (entry.external_attr >> 16) & 0o7000,
                    'Oversized or privileged wheel member')
            mapping[path] = destination
            if path == record_path:
                require(entry.file_size <= verifier.MAX_METADATA, 'Archive RECORD exceeds bound')
                with archive.open(entry) as member:
                    record_data = member.read(verifier.MAX_METADATA + 1)
                    require(len(record_data) == entry.file_size and not member.read(1), 'Truncated archive RECORD')
                source_members[path] = {'bytes': len(record_data), 'sha256': hashlib.sha256(record_data).hexdigest()}
            else:
                with archive.open(entry) as member:
                    copied = book.copy(member, root, destination, entry.file_size, bool(entry.external_attr >> 16 & 0o111))
                source_members[path] = copied
    require(record_data is not None, 'Missing primary archive RECORD')
    archive_record(record_data, record_path, source_members)
    members = sorted(mapping.values())
    book.blob(installed_record(installed_path, members, book.entries), root, installed_path)
    original_path = 'review/' + item['name'] + '/' + record_path
    require(book.entries[original_path]['sha256'] == source_members[record_path]['sha256'], 'Original RECORD provenance changed')
    return observed, {'name': item['name'], 'version': item['version'], 'archive': item['filename'],
                     'archive_sha256': item['sha256'], 'source_to_destination': mapping,
                     'original_record': original_path, 'installed_record': installed_path}, members


def open_directory(path):
    parent, name = files.parent_at(path)
    try:
        return files.directory_at(parent, [name])
    finally:
        os.close(parent)


def validate_input_set(root, plan):
    actual = set()
    with os.scandir(root) as entries:
        for entry in entries:
            require(len(actual) < len(plan), 'Unexpected wheel input count')
            info = entry.stat(follow_symlinks=False)
            require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1, 'Wheel inputs must be ordinary single-link files')
            actual.add(entry.name)
    require(actual == {item['filename'] for item in plan}, 'Missing or alternate wheel input')


def reject_nested_output(parent, sources):
    """Compare directory identities, including case aliases on insensitive hosts."""
    forbidden = {(os.fstat(fd).st_dev, os.fstat(fd).st_ino) for fd in sources}
    current = os.dup(parent)
    try:
        for _ in range(64):
            info = os.fstat(current)
            identity = (info.st_dev, info.st_ino)
            require(identity not in forbidden, 'Output must not be inside an immutable input')
            above = os.open('..', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=current)
            os.close(current)
            current = above
            parent_info = os.fstat(current)
            if identity == (parent_info.st_dev, parent_info.st_ino):
                return
        raise files.StagingError('Output ancestry exceeds bound')
    finally:
        os.close(current)


def install(runtime, wheelhouse, destination):
    report = {'schema_version': 1, 'assembled': False, 'installation_state': 'assembled-unexecuted',
              'complete_release': False, 'runtime_components_satisfied': [], 'package_code_executed': False,
              'unmet_checks': UNMET, 'failure': None, 'preceding_failure': None}
    parent = root = runtime_fd = wheel_fd = identity = None
    created = False
    try:
        require(os.name == 'posix' and hasattr(os, 'O_NOFOLLOW'), 'POSIX no-follow installation required')
        plan_bytes, plan = installation_plan()
        selection = wheels.selection()
        require(len(selection) == verifier.WHEEL_COUNT and plan['wheels'] == verifier.WHEEL_COUNT,
                'Reviewed wheel count mismatch')
        runtime_fd = open_directory(runtime)
        wheel_fd = open_directory(wheelhouse)
        raw = layout.read_pinned(runtime_fd, 'manifest.json', verifier.RUNTIME_SHA, 2 * 1024**2)
        manifest = json.loads(raw, object_pairs_hook=inventory.unique_object)
        runtime_entries = manifest['files'] | {'manifest.json': {'bytes': len(raw), 'sha256': verifier.RUNTIME_SHA, 'executable': False}}
        files.verify_output(runtime_fd, runtime_entries)
        validate_input_set(wheel_fd, selection)
        parent, name = files.parent_at(destination)
        reject_nested_output(parent, (runtime_fd, wheel_fd))
        os.mkdir(name, 0o700, dir_fd=parent)
        created = True
        root = files.directory_at(parent, [name])
        identity = os.fstat(root)
        book = FileInventory()
        for path, expected in sorted(manifest['files'].items()):
            require(path not in ('manifest.json', 'installation-provenance.json'), 'Runtime uses reserved installation metadata path')
            with files.regular_at(runtime_fd, path) as source:
                before = inventory.fingerprint(os.fstat(source.fileno()))
                copied = book.copy(source, root, path, expected['bytes'], expected['executable'])
                require(copied == expected and before == inventory.fingerprint(os.fstat(source.fileno())),
                        'Runtime source changed during copying')
        book.blob(raw, root, plan['runtime_manifest_destination'])
        book.blob(plan_bytes, root, 'review/installation-plan.json')
        observations, attributions, records, expanded = [], [], {}, 0
        for item in selection:
            with files.regular_at(wheel_fd, item['filename']) as stream:
                before = inventory.fingerprint(os.fstat(stream.fileno()))
                wheels.hash_wheel(stream, item)
                observation, attribution, members = install_wheel(stream, item, root, book,
                    min(MAX_WHEEL_EXPANDED - expanded, verifier.MAX_TOTAL - book.total))
                expanded += observation['expanded_bytes']
                observations.append(observation)
                attributions.append(attribution)
                records[attribution['installed_record']] = members
                stream.seek(0)
                wheels.hash_wheel(stream, item)
                require(before == inventory.fingerprint(os.fstat(stream.fileno())), 'Wheel changed during installation')
        # Re-read every immutable source, including early copies, before publishing.
        files.verify_output(runtime_fd, runtime_entries)
        validate_input_set(wheel_fd, selection)
        for item in selection:
            with files.regular_at(wheel_fd, item['filename']) as stream:
                wheels.hash_wheel(stream, item)
        require(wheels.selection() == selection and installation_plan()[0] == plan_bytes, 'Reviewed plans changed')
        provenance = {'schema_version': 1, 'installation_state': 'assembled-unexecuted', 'package_code_executed': False,
                      'complete_release': False, 'runtime_components_satisfied': [], 'unmet_checks': UNMET,
                      'runtime': {'manifest_sha256': verifier.RUNTIME_SHA, 'retained_manifest': plan['runtime_manifest_destination'],
                                  'mapping': 'All manifest.files copied to identical relative paths with unchanged bytes and executable flag.'},
                      'wheel_selection_sha256': wheels.PLAN_SHA, 'uv_lock_sha256': wheels.LOCK_SHA,
                      'installation_plan_sha256': verifier.PLAN_SHA, 'wheels': attributions,
                      'archive_validation': observations, 'omissions': ['console-wrappers', 'generated-bytecode', 'worker-bootstrap'],
                      'record_changes': 'Primary installed RECORD regenerated using actual locations and hashes. Original RECORD bytes retained under review; all other member bytes unchanged.',
                      'mode_policy': 'Ordinary file modes normalized to 0644 or 0755 using the source executable flag; no privileged bits retained.'}
        book.json(provenance, root, 'installation-provenance.json')
        output_manifest = {'schema_version': 1, 'kind': 'python-development-prefix', 'installation_state': 'assembled-unexecuted',
                           'complete_release': False, 'runtime_components_satisfied': [], 'package_code_executed': False,
                           'installation_plan_sha256': verifier.PLAN_SHA, 'runtime_manifest_sha256': verifier.RUNTIME_SHA,
                           'wheel_selection_sha256': wheels.PLAN_SHA, 'files': dict(book.entries), 'record_members': records}
        info = book.json(output_manifest, root, 'manifest.json')
        # The independent reader checks the complete finished tree and all records.
        verified = verifier.verify(destination, info['sha256'])
        require(verified.get('verified') is True, 'Independent finished-prefix verification failed')
        current = os.stat(name, dir_fd=parent, follow_symlinks=False)
        require((current.st_dev, current.st_ino) == (identity.st_dev, identity.st_ino), 'Destination changed')
        report.update(assembled=True, files=len(book.entries), bytes=book.total, wheels=len(selection),
                      manifest_sha256=info['sha256'], installation_plan_sha256=verifier.PLAN_SHA,
                      immutable_source_recheck=True, independent_verification=verified)
    except (OSError, ValueError, TypeError, KeyError, RecursionError, EOFError, csv.Error,
            zipfile.BadZipFile, zlib.error, NotImplementedError) as exc:
        report['failure'] = str(exc) if isinstance(exc, files.StagingError) else 'Unreadable or invalid installation input/output'
    finally:
        if created and not report['assembled']:
            try:
                current = os.stat(name, dir_fd=parent, follow_symlinks=False)
                require(root is not None and identity is not None
                        and (current.st_dev, current.st_ino) == (identity.st_dev, identity.st_ino),
                        'Partial-output identity unavailable')
                shutil.rmtree(name, dir_fd=parent)
            except (OSError, files.StagingError):
                report['preceding_failure'] = report['failure']
                report['failure'] = 'Partial-output cleanup failed; destination retained for recovery'
        for fd in (root, parent, runtime_fd, wheel_fd):
            if fd is not None:
                os.close(fd)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runtime', type=Path, required=True)
    parser.add_argument('--wheelhouse', type=Path, required=True)
    parser.add_argument('--destination', type=Path, required=True)
    args = parser.parse_args()
    report = install(args.runtime, args.wheelhouse, args.destination)
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report['assembled'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
