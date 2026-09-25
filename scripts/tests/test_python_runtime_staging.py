"""Synthetic archive checks; never execute an interpreter or fetch an asset."""
from contextlib import ExitStack
import hashlib
import io
import json
import os
import stat
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import stage_python_runtime as stager


def file_entry(name, data=b'synthetic', kind=tarfile.REGTYPE, link='', mode=0o644):
    member = tarfile.TarInfo(name)
    member.type, member.linkname, member.mode = kind, link, mode
    member.size = len(data) if kind == tarfile.REGTYPE else 0
    return member, data


def fixture():
    metadata = {'version': '8', 'target_triple': 'aarch64-apple-darwin',
                'python_version': '3.13.15', 'build_options': 'pgo+lto', 'python_tag': 'cp313'}
    entries = [file_entry('python/PYTHON.json', json.dumps(metadata).encode()),
               file_entry('python/build/not-retained.o')]
    entries += [file_entry('python/licenses/' + name) for name in stager.LICENSES]
    targets = {str(Path(alias).parent / target) for alias, target in stager.LINKS.items()}
    entries += [file_entry('python/install/' + name, mode=0o755 if name.startswith('bin/') else 0o644)
                for name in sorted(targets)]
    entries += [file_entry('python/install/' + alias, kind=tarfile.SYMTYPE, link=target)
                for alias, target in stager.LINKS.items()]
    return entries


def encode(entries):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode='w') as archive:
        for member, data in entries:
            archive.addfile(member, io.BytesIO(data) if member.isreg() else None)
    return stream.getvalue()


@unittest.skipUnless(os.name == 'posix' and hasattr(os, 'O_NOFOLLOW'), 'POSIX build-tool checks')
class PythonRuntimeStaging(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.archive = self.root / 'source.tar'
        self.output = self.root / 'runtime'

    def stage(self, entries=None, patches=None, raw=None):
        data = raw if raw is not None else encode(fixture() if entries is None else entries)
        self.archive.write_bytes(data)
        with ExitStack() as stack:
            stack.enter_context(mock.patch.object(stager, 'ARCHIVE_BYTES', len(data)))
            stack.enter_context(mock.patch.object(stager, 'ARCHIVE_SHA256', hashlib.sha256(data).hexdigest()))
            stack.enter_context(mock.patch.object(stager, 'open_decoder', lambda stream: io.BytesIO(stream.read())))
            for name, value in (patches or {}).items():
                stack.enter_context(mock.patch.object(stager, name, value))
            return stager.stage(self.archive, self.output)

    def rejected(self, entries=None, patches=None, raw=None):
        result = self.stage(entries, patches, raw)
        self.assertFalse(result['staged'], result)
        self.assertFalse(result['complete_release'])
        self.assertFalse(self.output.exists(), result)
        return result

    def test_success_preserves_notices_and_metadata_materializes_all_ten_links(self):
        report = self.stage()
        self.assertTrue(report['staged'], report)
        self.assertFalse(report['complete_release'])
        self.assertFalse(report['interpreter_executed'])
        self.assertEqual(len(list((self.output / 'licenses').iterdir())), 19)
        for alias, target in stager.LINKS.items():
            copied = self.output / 'install' / alias
            source = self.output / 'install' / Path(alias).parent / target
            self.assertFalse(copied.is_symlink())
            self.assertEqual(copied.stat().st_nlink, 1)
            self.assertNotEqual(copied.stat().st_ino, source.stat().st_ino)
            self.assertEqual(copied.read_bytes(), source.read_bytes())
        self.assertFalse((self.output / 'build').exists())
        provenance = json.loads((self.output / 'provenance.json').read_text())
        self.assertEqual(provenance['materialized_links'], stager.LINKS)
        self.assertIn('complete-notices-unverified', provenance['unmet_checks'])
        self.assertEqual((self.output / 'PYTHON.json').read_bytes(), fixture()[0][1])

    def test_manifest_is_deterministic_and_matches_retained_files(self):
        first = self.stage()
        manifest = (self.output / 'manifest.json').read_bytes()
        self.output = self.root / 'second'
        second = self.stage()
        self.assertTrue(second['staged'], second)
        self.assertEqual(first['manifest_sha256'], second['manifest_sha256'])
        self.assertEqual(manifest, (self.output / 'manifest.json').read_bytes())
        for name, item in json.loads(manifest)['files'].items():
            data = (self.output / name).read_bytes()
            self.assertEqual(len(data), item['bytes'])
            self.assertEqual(hashlib.sha256(data).hexdigest(), item['sha256'])

    def test_paths_duplicate_case_collision_and_file_parent_conflicts(self):
        for name in ('/absolute', 'python/install/../escape', 'python/install/a\\b',
                     'python/install/a:stream', 'python/install/a./file', 'python/install//file',
                     'python/install/CON', 'python/install/bin/python3.13/child',
                     'python/install/BIN/python3.13', 'python/PYTHON.json'):
            with self.subTest(name=name):
                self.rejected(fixture() + [file_entry(name)])

    def test_special_sparse_privileged_and_unsupported_pax_members(self):
        for kind in (tarfile.DIRTYPE, tarfile.LNKTYPE, tarfile.FIFOTYPE, tarfile.CHRTYPE,
                     tarfile.GNUTYPE_SPARSE):
            with self.subTest(kind=kind):
                self.rejected(fixture() + [file_entry('python/install/extra', kind=kind)])
        self.rejected(fixture() + [file_entry('python/install/extra', mode=0o4755)])
        member, data = file_entry('python/install/extra')
        member.pax_headers = {'SCHILY.xattr.user.test': 'unexpected'}
        self.rejected(fixture() + [(member, data)])

    def test_link_targets_and_sets_are_exact(self):
        for target in ('/outside', '../outside', 'python3', 'other', 'python3.13/child'):
            entries = fixture()
            entries[-7][0].linkname = target
            self.rejected(entries)
        self.rejected(fixture() + [file_entry('python/install/extra', kind=tarfile.SYMTYPE, link='bin/python3.13')])
        self.rejected(fixture()[:-1])
        self.rejected([x for x in fixture() if x[0].name != 'python/install/bin/python3.13'])

    def test_licence_and_runtime_identity_are_required(self):
        self.rejected([x for x in fixture() if x[0].name != 'python/licenses/' + stager.LICENSES[0]])
        self.rejected(fixture() + [file_entry('python/licenses/extra')])
        entries = fixture()
        entries[0] = file_entry('python/PYTHON.json', b'{"version":"8", "python_version":"3.13.15t"}')
        self.rejected(entries)
        entries[0] = file_entry('python/PYTHON.json', b'{"version":"8","version":"8"}')
        self.rejected(entries)

    def test_count_depth_file_payload_output_and_decompressed_bounds(self):
        for limit in ('MAX_MEMBERS', 'MAX_DEPTH', 'MAX_FILE', 'MAX_PAYLOAD', 'MAX_OUTPUT', 'MAX_DECOMPRESSED'):
            with self.subTest(limit=limit):
                self.rejected(patches={limit: 1})
        # Every member fits, but the copied aliases push output over its budget.
        entries = fixture()
        ordinary = sum(member.size for member, _ in entries if member.isreg())
        self.rejected(entries, {'MAX_OUTPUT': ordinary})
        # tar's terminator cannot hide unbounded trailing decompressed bytes.
        raw = encode(entries)
        self.rejected(raw=raw + b'x' * 20480, patches={'MAX_DECOMPRESSED': len(raw)})

    def test_hash_and_size_fail_before_decoder_or_output_creation(self):
        data = encode(fixture())
        self.archive.write_bytes(data)
        with mock.patch.object(stager, 'open_decoder') as decoder:
            report = stager.stage(self.archive, self.output)
        self.assertFalse(report['staged'])
        self.assertFalse(self.output.exists())
        decoder.assert_not_called()
        self.rejected(patches={'ARCHIVE_SHA256': '0' * 64})

    def test_truncated_archive_and_decoder_failure_leave_no_output(self):
        self.rejected(raw=encode(fixture())[:2000])
        self.rejected(patches={'open_decoder': mock.Mock(side_effect=stager.StagingError('Invalid compressed archive'))})

    def test_missing_zstd_is_a_lazy_clear_build_prerequisite(self):
        with mock.patch.dict(sys.modules, {'compression': None}):
            with self.assertRaisesRegex(stager.StagingError, 'build-time Python 3.14'):
                with stager.open_decoder(io.BytesIO(b'not executed')):
                    self.fail('decoder must not open')

    def test_missing_destination_parent_fails_without_creating_it(self):
        self.output = self.root / 'absent' / 'child'
        self.rejected()
        self.assertFalse(self.output.parent.exists())

    def test_existing_destination_is_unchanged(self):
        self.output.mkdir()
        sentinel = self.output / 'sentinel'
        sentinel.write_bytes(b'preserve')
        report = self.stage()
        self.assertFalse(report['staged'])
        self.assertEqual(list(self.output.iterdir()), [sentinel])
        self.assertEqual(sentinel.read_bytes(), b'preserve')

    def test_symlinked_input_and_output_parents_and_hardlinked_input_reject(self):
        alias = self.root / 'alias'
        alias.symlink_to(self.root, target_is_directory=True)
        self.output = alias / 'out'
        self.rejected()
        self.output = self.root / 'out'
        original = self.archive
        self.archive = alias / 'source.tar'
        self.rejected()
        self.archive = original
        linked = self.root / 'hardlink'
        linked.hardlink_to(self.archive)
        self.rejected()

    def test_archive_change_after_initial_hash_cannot_publish_manifest(self):
        original = stager.unpack
        def mutate(decoded, root):
            result = original(decoded, root)
            with self.archive.open('r+b') as stream:
                stream.write(b'changed')
            return result
        self.rejected(patches={'unpack': mutate})

    def test_partial_copy_failure_and_cleanup_failure_are_not_success(self):
        original = stager.copy_member
        def fail(source, root, relative, size, executable):
            original(source, root, relative, size, executable)
            raise OSError('private local path must not appear')
        result = self.rejected(patches={'copy_member': fail})
        self.assertNotIn('private', result['failure'])
        with mock.patch.object(stager.shutil, 'rmtree', side_effect=OSError('private path')):
            result = self.stage(patches={'copy_member': fail})
        self.assertFalse(result['staged'])
        self.assertEqual(result['failure'], 'Partial-output cleanup failed; destination retained for recovery')
        self.assertEqual(result['preceding_failure'], 'Unreadable or invalid staging input/output')
        self.assertTrue(self.output.exists())
        self.assertFalse((self.output / 'manifest.json').exists())

    def test_modified_or_unexpected_output_fails_final_verification(self):
        original = stager.unpack
        for unexpected in (False, True):
            with self.subTest(unexpected=unexpected):
                def mutate(decoded, root):
                    result = original(decoded, root)
                    path = self.output / ('unexpected' if unexpected else 'PYTHON.json')
                    path.write_bytes(b'changed')
                    return result
                self.rejected(patches={'unpack': mutate})

    def test_manifest_write_failure_and_unexpected_link_never_pass(self):
        original = stager.write_json
        def fail(root, name, value):
            result = original(root, name, value)
            if name == 'manifest.json':
                raise OSError('simulated persistence failure')
            return result
        self.rejected(patches={'write_json': fail})
        unpack = stager.unpack
        def introduce(decoded, root):
            result = unpack(decoded, root)
            (self.output / 'unexpected').symlink_to(self.archive)
            return result
        self.rejected(patches={'unpack': introduce})

    def test_permission_normalization_is_independent_of_umask(self):
        old = os.umask(0o077)
        try:
            result = self.stage()
        finally:
            os.umask(old)
        self.assertTrue(result['staged'], result)
        self.assertEqual((self.output / 'install/bin/python3.13').stat().st_mode & 0o777, 0o755)
        self.assertEqual((self.output / 'PYTHON.json').stat().st_mode & 0o777, 0o644)

    def test_unknown_root_identity_retains_partial_output_and_failed_receipt(self):
        original = os.fstat
        def fail_directory(fd):
            info = original(fd)
            if stat.S_ISDIR(info.st_mode):
                raise OSError('simulated root identity read failure')
            return info
        with mock.patch.object(stager.os, 'fstat', side_effect=fail_directory):
            report = self.stage()
        self.assertFalse(report['staged'])
        self.assertEqual(report['failure'], 'Partial-output cleanup failed; destination retained for recovery')
        self.assertEqual(report['preceding_failure'], 'Unreadable or invalid staging input/output')
        self.assertTrue(self.output.is_dir())
        self.assertEqual(list(self.output.iterdir()), [])


class PythonStagingPortableBoundary(unittest.TestCase):
    def test_non_posix_host_refuses_before_file_access(self):
        with mock.patch.object(stager.os, 'name', 'nt'), mock.patch.object(stager, 'parent_at') as opened:
            report = stager.stage('unused archive', 'unused destination')
        self.assertFalse(report['staged'])
        self.assertEqual(report['failure'], 'POSIX no-follow staging support required')
        opened.assert_not_called()


if __name__ == '__main__':
    unittest.main()
