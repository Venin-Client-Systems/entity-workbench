#!/usr/bin/env python3
"""Copy the exact reviewed prefix without interpreter or package execution."""
from __future__ import annotations
import argparse
import json
import os
from pathlib import Path
import shutil

import install_python_offline as assembler
import stage_python_runtime as files
import verify_python_install as verifier
import verify_runtime_bundle as inventory

MANIFEST = '4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822'


def relocate(source, destination, expected_manifest=MANIFEST):
    report = {'schema_version': 1, 'copied': False, 'candidate_executed': False, 'complete_release': False,
              'manifest_sha256': expected_manifest, 'failure': None, 'preceding_failure': None}
    parent = root = input_root = identity = None
    created = False
    try:
        files.require(os.name == 'posix' and shutil.rmtree.avoids_symlink_attacks, 'POSIX no-follow relocation required')
        input_root = assembler.open_directory(source)
        before_root = os.fstat(input_root)
        initial = verifier.verify_root(input_root, expected_manifest)
        raw = verifier.read_bytes(input_root, 'manifest.json')
        entries = json.loads(raw, object_pairs_hook=inventory.unique_object)['files']
        entries = entries | {'manifest.json': {'bytes': len(raw), 'sha256': expected_manifest, 'executable': False}}
        parent, name = files.parent_at(destination)
        assembler.reject_nested_output(parent, (input_root,))
        os.mkdir(name, 0o700, dir_fd=parent); created = True
        root = files.directory_at(parent, [name]); identity = os.fstat(root)
        book = assembler.FileInventory()
        for path, expected in sorted(entries.items()):
            with files.regular_at(input_root, path) as stream:
                before = inventory.fingerprint(os.fstat(stream.fileno()))
                copied = book.copy(stream, root, path, expected['bytes'], expected['executable'])
                files.require(copied == expected and before == inventory.fingerprint(os.fstat(stream.fileno())),
                              'Relocation source changed during copying')
        # Independent source and destination readers, plus pathname identity readback.
        source_check = verifier.verify(source, expected_manifest)
        output_check = verifier.verify(destination, expected_manifest)
        source_parent, source_name = files.parent_at(source)
        try:
            now = os.stat(source_name, dir_fd=source_parent, follow_symlinks=False)
        finally:
            os.close(source_parent)
        current = os.stat(name, dir_fd=parent, follow_symlinks=False)
        files.require((now.st_dev, now.st_ino) == (before_root.st_dev, before_root.st_ino)
                      and (current.st_dev, current.st_ino) == (identity.st_dev, identity.st_ino)
                      and source_check.get('verified') is True and output_check.get('verified') is True,
                      'Relocation source or destination verification failed')
        files.require(source_check == initial and output_check == initial, 'Relocation inventory differs')
        report.update(copied=True, files=len(entries), bytes=book.total,
                      immutable_source_verified=True, independent_verification=output_check)
    except (OSError, ValueError, TypeError, KeyError, RecursionError):
        report['failure'] = 'Invalid or unavailable relocation input/output'
    finally:
        if created and not report['copied']:
            try:
                current = os.stat(name, dir_fd=parent, follow_symlinks=False)
                files.require(root is not None and identity is not None
                              and (current.st_dev, current.st_ino) == (identity.st_dev, identity.st_ino),
                              'Partial relocation identity unavailable')
                shutil.rmtree(name, dir_fd=parent)
            except (OSError, files.StagingError):
                report['preceding_failure'] = report['failure']
                report['failure'] = 'Partial relocation cleanup failed; retained for recovery'
        for descriptor in (root, parent, input_root):
            if descriptor is not None:
                os.close(descriptor)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--destination', type=Path, required=True)
    args = parser.parse_args()
    report = relocate(args.source, args.destination)
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report['copied'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
