#!/usr/bin/env python3
"""Stage one reviewed CPython archive offline; never execute its contents."""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import stat
import sys
import tarfile

import verify_runtime_bundle as inventory

ARCHIVE_NAME = 'cpython-3.13.15+20260924-aarch64-apple-darwin-pgo+lto-full.tar.zst'
ARCHIVE_BYTES = 59_200_647
ARCHIVE_SHA256 = 'aa76e997b7bb159002dd38938e57ce06fc07bf73621131d3d126d459df4e9ac7'
SOURCE_COMMIT = '6a729962cddc76630b59b1b895501b1539412524'
RELEASE = '20260924'
MAX_MEMBERS = 5_000
MAX_DEPTH = 16
MAX_FILE = 80 * 1024**2
MAX_PAYLOAD = 280 * 1024**2
MAX_DECOMPRESSED = 300 * 1024**2
MAX_OUTPUT = 320 * 1024**2
CHUNK = 64 * 1024
LINKS = {
    'bin/idle3': 'idle3.13',
    'bin/pydoc3': 'pydoc3.13',
    'bin/python': 'python3.13',
    'bin/python3': 'python3.13',
    'bin/python3-config': 'python3.13-config',
    'lib/libpython3.13.a': 'python3.13/config-3.13-darwin/libpython3.13.a',
    'lib/pkgconfig/python3-embed.pc': 'python-3.13-embed.pc',
    'lib/pkgconfig/python3.pc': 'python-3.13.pc',
    'share/man/man1/python.1': 'python3.13.1',
    'share/man/man1/python3.1': 'python3.13.1',
}
LICENSES = tuple('LICENSE.' + name + '.txt' for name in (
    'bdb', 'bzip2', 'cpython', 'expat', 'libX11', 'libXau', 'libedit', 'libffi',
    'liblzma', 'libuuid', 'libxcb', 'mpdecimal', 'ncurses', 'openssl-1.1',
    'openssl-3', 'sqlite', 'tcl', 'tix', 'zlib'))
UNMET = (
    'locked-wheelhouse-not-staged', 'native-loader-closure-unverified',
    'relocated-interpreter-not-executed', 'worker-confinement-unverified',
    'canonical-worker-not-enabled', 'complete-notices-unverified',
    'upstream-metadata-references-absent-LICENSE.zlib-ng.txt',
    'minimum-supported-os-not-established',
)


class StagingError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise StagingError(message)


def safe_parts(value):
    try:
        inventory.path_key(value, MAX_DEPTH)
    except inventory.InvalidInventory as exc:
        raise StagingError('Unsafe archive or output path') from exc
    return value.split('/')


def directory_at(parent, parts, create=False):
    """Pin each directory; never traverse a symlink, including an ancestor."""
    current = os.dup(parent)
    try:
        for part in parts:
            if create:
                try:
                    os.mkdir(part, 0o700, dir_fd=current)
                except FileExistsError:
                    pass
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=current)
            os.close(current)
            current = child
        return current
    except BaseException:
        os.close(current)
        raise


def parent_at(path):
    path = Path(os.path.abspath(path))
    safe_parts(path.name)
    root = os.open(path.anchor, os.O_RDONLY | os.O_DIRECTORY)
    try:
        return directory_at(root, path.parts[1:-1]), path.name
    finally:
        os.close(root)


def regular_at(root, relative, write=False, executable=False):
    parts = safe_parts(relative)
    parent = directory_at(root, parts[:-1], create=write)
    try:
        flags = os.O_NOFOLLOW | (os.O_WRONLY | os.O_CREAT | os.O_EXCL if write else os.O_RDONLY)
        fd = os.open(parts[-1], flags, 0o755 if executable else 0o644, dir_fd=parent)
    finally:
        os.close(parent)
    opened = os.fstat(fd)
    if not stat.S_ISREG(opened.st_mode) or opened.st_nlink != 1:
        os.close(fd)
        raise StagingError('Expected single-link regular file')
    if write:
        try:
            os.fchmod(fd, 0o755 if executable else 0o644)
        except BaseException:
            os.close(fd)
            raise
    return os.fdopen(fd, 'wb' if write else 'rb')


def digest_stream(stream):
    digest = hashlib.sha256()
    count = 0
    while block := stream.read(CHUNK):
        count += len(block)
        require(count <= ARCHIVE_BYTES, 'Archive size mismatch')
        digest.update(block)
    require(count == ARCHIVE_BYTES and digest.hexdigest() == ARCHIVE_SHA256,
            'Archive identity mismatch')


@contextmanager
def open_decoder(stream):
    # Importable on Python3.13 source CI; only actual zstd staging needs3.14.
    try:
        from compression import zstd
    except ImportError as exc:
        raise StagingError('Staging requires build-time Python 3.14 with stdlib zstd') from exc
    try:
        with zstd.ZstdFile(stream, 'rb') as decoded:
            yield decoded
    except zstd.ZstdError as exc:
        raise StagingError('Invalid compressed archive') from exc


class BoundedReader:
    def __init__(self, stream):
        self.stream = stream
        self.count = 0

    def read(self, amount=CHUNK):
        require(0 <= amount <= CHUNK, 'Unbounded decompression read')
        block = self.stream.read(amount)
        self.count += len(block)
        require(self.count <= MAX_DECOMPRESSED, 'Decompressed archive limit exceeded')
        return block


def copy_member(source, root, relative, size, executable):
    digest = hashlib.sha256()
    left = size
    with regular_at(root, relative, write=True, executable=executable) as output:
        while left:
            block = source.read(min(left, CHUNK))
            require(bool(block), 'Truncated archive member')
            output.write(block)
            digest.update(block)
            left -= len(block)
        output.flush()
        os.fsync(output.fileno())
    return {'bytes': size, 'sha256': digest.hexdigest(), 'executable': executable}


def write_json(root, name, value):
    data = (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=True, allow_nan=False) + '\n').encode()
    require(len(data) < 2 * 1024**2, 'Generated metadata exceeds bound')
    return copy_member(io.BytesIO(data), root, name, len(data), False)


def unpack(decoded, root):
    """A pinned, single-pass stream; no extractall, archive-controlled chmod or links."""
    reader = BoundedReader(decoded)
    names, keys, parents, seen_links, files = set(), set(), set(), set(), {}
    payload = output_bytes = 0
    with tarfile.open(fileobj=reader, mode='r|') as archive:
        for member in archive:
            parts = safe_parts(member.name)
            key = inventory.path_key(member.name, MAX_DEPTH)
            require(len(names) < MAX_MEMBERS, 'Archive member count exceeded')
            require(member.name not in names and key not in keys and key not in parents,
                    'Duplicate, colliding or conflicting archive member')
            prefixes = [inventory.path_key('/'.join(parts[:n]), MAX_DEPTH) for n in range(1, len(parts))]
            require(not keys.intersection(prefixes), 'Archive file used as a directory')
            names.add(member.name)
            keys.add(key)
            parents.update(prefixes)
            require(set(member.pax_headers) <= {'path'}, 'Unsupported archive metadata')
            require(member.sparse is None and member.mode & 0o7000 == 0, 'Unsupported sparse or privileged member')
            require(member.isreg() or member.issym(), 'Special files, directories and hardlinks are forbidden')
            require(0 <= member.size <= MAX_FILE, 'Archive member size exceeded')
            payload += member.size
            require(payload <= MAX_PAYLOAD, 'Archive payload limit exceeded')
            selected = None
            if member.name.startswith('python/install/'):
                selected = member.name.removeprefix('python/')
            elif member.name.startswith('python/licenses/'):
                require(member.name.removeprefix('python/licenses/') in LICENSES, 'Unexpected licence member')
                selected = member.name.removeprefix('python/')
            elif member.name == 'python/PYTHON.json':
                selected = 'PYTHON.json'
            else:
                require(member.name.startswith('python/build/'), 'Unexpected archive root')
            if member.issym():
                relative = member.name.removeprefix('python/install/')
                require(member.name.startswith('python/install/') and relative in LINKS
                        and member.linkname == LINKS[relative] and member.size == 0,
                        'Unreviewed archive link')
                safe_parts(member.linkname)
                seen_links.add(relative)
                continue
            if selected is not None:
                output_bytes += member.size
                require(output_bytes <= MAX_OUTPUT, 'Expanded output limit exceeded')
                files[selected] = copy_member(archive.extractfile(member), root, selected, member.size,
                                               bool(member.mode & 0o111))
    # Account for trailing decompressed bytes too; tarfile stops at its terminator.
    while reader.read(CHUNK):
        pass
    require(seen_links == set(LINKS), 'Reviewed link set is incomplete')
    require({name for name in files if name.startswith('licenses/')} == {'licenses/' + name for name in LICENSES},
            'Reviewed licence set is incomplete')
    require('PYTHON.json' in files, 'Upstream metadata missing')
    require(files['PYTHON.json']['bytes'] <= 2 * 1024**2, 'Upstream metadata exceeds bound')
    with regular_at(root, 'PYTHON.json') as metadata:
        value = json.load(metadata, object_pairs_hook=inventory.unique_object)
    require(isinstance(value, dict) and all(value.get(k) == v for k, v in {
        'version': '8', 'target_triple': 'aarch64-apple-darwin', 'python_version': '3.13.15',
        'build_options': 'pgo+lto', 'python_tag': 'cp313',
    }.items()), 'Upstream runtime metadata mismatch')
    for relative, link in sorted(LINKS.items()):
        target = 'install/' + str(Path(relative).parent / link)
        require(target in files, 'Reviewed link must target an ordinary retained file')
        expected = files[target]
        output_bytes += expected['bytes']
        require(output_bytes <= MAX_OUTPUT, 'Expanded output limit exceeded')
        with regular_at(root, target) as source:
            copied = copy_member(source, root, 'install/' + relative, expected['bytes'], expected['executable'])
            require(not source.read(1) and copied == expected, 'Link materialization source changed')
        files['install/' + relative] = copied
    return files, {'members': len(names), 'payload_bytes': payload, 'decompressed_bytes': reader.count}


def verify_output(root, files):
    pending, actual, count = [''], set(), 0
    while pending:
        relative = pending.pop()
        fd = directory_at(root, relative.split('/') if relative else [])
        try:
            with os.scandir(fd) as entries:
                for entry in entries:
                    count += 1
                    require(count <= MAX_MEMBERS * 2, 'Staged entry count exceeded')
                    path = relative + '/' + entry.name if relative else entry.name
                    safe_parts(path)
                    info = entry.stat(follow_symlinks=False)
                    if stat.S_ISDIR(info.st_mode):
                        pending.append(path)
                    else:
                        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1, 'Unsafe staged file')
                        actual.add(path)
        finally:
            os.close(fd)
    require(actual == set(files), 'Staged tree contains missing or unexpected files')
    for name, expected in files.items():
        with regular_at(root, name) as stream:
            require(bool(os.fstat(stream.fileno()).st_mode & 0o111) == expected['executable'],
                    'Staged executable mode changed')
            digest = hashlib.sha256()
            count = 0
            while block := stream.read(CHUNK):
                count += len(block)
                require(count <= expected['bytes'], 'Staged content changed')
                digest.update(block)
            require(count == expected['bytes'] and digest.hexdigest() == expected['sha256'],
                    'Staged content changed')


def stage(archive_path, destination):
    report = {'schema_version': 1, 'staged': False, 'complete_release': False,
              'interpreter_executed': False, 'unmet_checks': list(UNMET), 'failure': None,
              'preceding_failure': None}
    parent = root = identity = None
    created = False
    try:
        require(os.name == 'posix' and hasattr(os, 'O_NOFOLLOW'), 'POSIX no-follow staging support required')
        source_parent, source_name = parent_at(archive_path)
        try:
            stream = regular_at(source_parent, source_name)
        finally:
            os.close(source_parent)
        with stream:
            before = os.fstat(stream.fileno())
            require(before.st_size == ARCHIVE_BYTES, 'Archive size mismatch')
            digest_stream(stream)
            stream.seek(0)
            # Check capability before creating a destination.
            with open_decoder(stream) as decoded:
                parent, name = parent_at(destination)
                os.mkdir(name, 0o700, dir_fd=parent)
                created = True
                root = directory_at(parent, [name])
                identity = os.fstat(root)
                files, observed = unpack(decoded, root)
            require(inventory.fingerprint(os.fstat(stream.fileno())) == inventory.fingerprint(before),
                    'Archive changed during staging')
            stream.seek(0)
            digest_stream(stream)
        provenance = {'schema_version': 1, 'development_only': True, 'complete_release': False,
                      'source': {'project': 'astral-sh/python-build-standalone', 'commit': SOURCE_COMMIT,
                                 'release': RELEASE, 'asset': ARCHIVE_NAME, 'bytes': ARCHIVE_BYTES,
                                 'sha256': ARCHIVE_SHA256,
                                 'release_url': 'https://github.com/astral-sh/python-build-standalone/releases/tag/' + RELEASE},
                      'materialized_links': LINKS, 'observed_archive': observed,
                      'unmet_checks': list(UNMET), 'interpreter_executed': False,
                      'transformations': 'Contained reviewed links copied into independent regular files; modes normalized to 0644 or 0755. Upstream metadata and retained file bytes unchanged.'}
        files['provenance.json'] = write_json(root, 'provenance.json', provenance)
        verify_output(root, files)
        manifest = {'schema_version': 1, 'target': 'macos-aarch64', 'engine': 'cpython-3.13.15',
                    'development_only': True, 'complete_release': False, 'files': files,
                    'unmet_checks': list(UNMET)}
        info = write_json(root, 'manifest.json', manifest)
        require(sum(x['bytes'] for x in files.values()) + info['bytes'] <= MAX_OUTPUT,
                'Expanded output limit exceeded')
        current = os.stat(name, dir_fd=parent, follow_symlinks=False)
        require((current.st_dev, current.st_ino) == (identity.st_dev, identity.st_ino), 'Destination changed')
        report.update(staged=True, files=len(files) + 1, bytes=sum(x['bytes'] for x in files.values()) + info['bytes'],
                      manifest_sha256=info['sha256'], archive_sha256=ARCHIVE_SHA256)
    except (StagingError, inventory.InvalidInventory, OSError, tarfile.TarError, ValueError,
            TypeError, KeyError, RecursionError, EOFError) as exc:
        report['failure'] = str(exc) if isinstance(exc, StagingError) else 'Unreadable or invalid staging input/output'
    finally:
        if created and not report['staged']:
            try:
                current = os.stat(name, dir_fd=parent, follow_symlinks=False)
                require(root is not None and identity is not None
                        and (current.st_dev, current.st_ino) == (identity.st_dev, identity.st_ino),
                        'Partial-output identity unavailable')
                shutil.rmtree(name, dir_fd=parent)
            except (OSError, StagingError):
                report['preceding_failure'] = report['failure']
                report['failure'] = 'Partial-output cleanup failed; destination retained for recovery'
        if root is not None:
            os.close(root)
        if parent is not None:
            os.close(parent)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--destination', type=Path, required=True)
    args = parser.parse_args()
    report = stage(args.archive, args.destination)
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report['staged'] else 1


if __name__ == '__main__':
    sys.exit(main())
