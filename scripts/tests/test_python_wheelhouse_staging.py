"""Only synthetic wheels; embedded Python bytes are never executed."""
from contextlib import ExitStack
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import struct
import sys
import tempfile
import unittest
from unittest import mock
import warnings
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import stage_python_wheelhouse as stager

NAME = 'fixture_pkg'
VERSION = '1.0.0'
FILENAME = NAME + '-' + VERSION + '-py3-none-any.whl'
INFO = NAME + '-' + VERSION + '.dist-info'


def wheel(extra=(), metadata=None, wheel_metadata=None, compression=zipfile.ZIP_DEFLATED):
    output = io.BytesIO()
    records = [(NAME + '/__init__.py', b'raise RuntimeError("must never execute package code")\n'),
               (INFO + '/METADATA', metadata or b'Metadata-Version: 2.4\nName: fixture-pkg\nVersion: 1.0.0\nLicense-File: LICENSE\n\nSynthetic description.'),
               (INFO + '/WHEEL', wheel_metadata or b'Wheel-Version: 1.0\nRoot-Is-Purelib: true\nTag: py3-none-any\n'),
               (INFO + '/RECORD', b''), (INFO + '/licenses/LICENSE', b'SYNTHETIC notice\n')]
    with warnings.catch_warnings():
        warnings.simplefilter('ignore', UserWarning)
        with zipfile.ZipFile(output, 'w', compression=compression) as archive:
            for name, data in records + list(extra): archive.writestr(name, data)
    return output.getvalue()


def item(data, filename=FILENAME):
    return {'name': 'fixture-pkg', 'version': VERSION, 'filename': filename, 'bytes': len(data),
            'sha256': hashlib.sha256(data).hexdigest(), 'url': 'https://files.pythonhosted.org/packages/' + filename}


class PortableWheelhouseTests(unittest.TestCase):
    def test_unsupported_host_refuses_before_selection_or_input_access(self):
        with mock.patch.object(stager.os, 'name', 'nt'), mock.patch.object(stager, 'selection') as selection:
            report = stager.stage('unused', 'unused')
        self.assertFalse(report['staged'])
        self.assertEqual(report['failure'], 'POSIX no-follow staging support required')
        selection.assert_not_called()

    def test_explicit_tag_contract_rejects_wrong_version_abi_platform_and_sdist(self):
        for name in ('fixture_pkg-2.0.0-py3-none-any.whl', 'fixture_pkg-1.0.0-cp313-cp313t-macosx_11_0_arm64.whl',
                     'fixture_pkg-1.0.0-cp313-cp313-win_amd64.whl', 'fixture_pkg-1.0.0-cp313-cp313-macosx_14_0_arm64.whl',
                     'fixture_pkg-1.0.0-py2-none-any.whl', 'fixture_pkg-1.0.0.tar.gz'):
            with self.subTest(filename=name), self.assertRaises(stager.StagingError):
                stager.filename_tags(name, 'fixture-pkg', VERSION)


@unittest.skipUnless(os.name == 'posix' and hasattr(os, 'O_NOFOLLOW'), 'POSIX build-tool checks')
class WheelhouseStagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.inputs = self.root / 'inputs'; self.inputs.mkdir()
        self.output = self.root / 'output'

    def stage(self, data=None, patches=None, filename=FILENAME):
        data = wheel() if data is None else data
        self.source = self.inputs / filename
        self.source.write_bytes(data)
        self.plan = [item(data, filename)]
        with ExitStack() as stack:
            stack.enter_context(mock.patch.object(stager, 'selection', return_value=self.plan))
            stack.enter_context(mock.patch.object(stager, 'PACKAGE_COUNT', 1))
            for key, value in (patches or {}).items(): stack.enter_context(mock.patch.object(stager, key, value))
            return stager.stage(self.inputs, self.output)

    def rejected(self, *args, **kwargs):
        report = self.stage(*args, **kwargs)
        self.assertFalse(report['staged'], report)
        self.assertFalse(report['complete_release'])
        self.assertEqual(report['runtime_components_satisfied'], [])
        self.assertFalse(self.output.exists(), report)
        return report

    def test_success_is_archives_only_preserves_bytes_notices_and_metadata(self):
        data = wheel(); result = self.stage(data)
        self.assertTrue(result['staged'], result)
        self.assertEqual(result['installation_state'], 'archives-only')
        self.assertFalse(result['package_code_executed'])
        self.assertEqual(result['runtime_components_satisfied'], [])
        self.assertEqual((self.output / 'wheels' / FILENAME).read_bytes(), data)
        self.assertEqual((self.output / 'review/fixture-pkg' / INFO / 'licenses/LICENSE').read_bytes(), b'SYNTHETIC notice\n')
        self.assertFalse((self.output / 'review/fixture-pkg' / NAME).exists())
        self.assertNotIn(NAME, sys.modules)
        provenance = json.loads((self.output / 'provenance.json').read_text())
        self.assertTrue(provenance['archives'][0]['zip_crc_verified'])
        self.assertFalse(provenance['archives'][0]['runnable_engine'])

    def test_manifest_is_deterministic_and_independently_matches_all_files(self):
        # ZIP member timestamps vary between independently created archives.
        # Deterministic staging compares two outputs from the same input bytes.
        data = wheel()
        first = self.stage(data); self.assertTrue(first['staged'], first)
        expected = (self.output / 'manifest.json').read_bytes()
        self.output = self.root / 'second'
        second = self.stage(data); self.assertTrue(second['staged'], second)
        self.assertEqual(first['manifest_sha256'], second['manifest_sha256'])
        self.assertEqual(expected, (self.output / 'manifest.json').read_bytes())
        for name, info in json.loads(expected)['files'].items():
            path = self.output / name
            self.assertEqual(path.stat().st_nlink, 1)
            self.assertEqual(path.stat().st_mode & 0o111, 0)
            self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), info['sha256'])

    def test_paths_duplicates_collisions_and_file_parents_reject(self):
        for path in ('../escape', '/absolute', 'module\\escape', 'module:stream', 'module./file', 'CON',
                     NAME + '/__init__.py', NAME.upper() + '/__init__.py', NAME + '/__init__.py/child'):
            with self.subTest(path=path): self.rejected(wheel([(path, b'unsafe')]))
        self.rejected(wheel([('caf\u00e9', b'a'), ('cafe\u0301', b'b')]))

    def test_special_links_and_root_distribution_ambiguity_reject(self):
        for kind in (stat.S_IFLNK, stat.S_IFIFO, stat.S_IFCHR):
            info = zipfile.ZipInfo('unexpected'); info.create_system = 3; info.external_attr = (kind | 0o644) << 16
            self.rejected(wheel([(info, b'target')]))
        self.rejected(wheel([('other-1.0.dist-info/METADATA', b'other')]))
        # A vendored nested distribution is not a second primary distribution.
        report = self.stage(wheel([('vendor/other-1.0.dist-info/METADATA', b'vendored')]))
        self.assertTrue(report['staged'], report)

    def test_metadata_identity_duplicates_tags_and_declared_notices_reject(self):
        for metadata in (b'Metadata-Version: 2.4\nName: other\nVersion: 1.0.0\n',
                         b'Metadata-Version: 2.4\nName: fixture-pkg\nName: fixture-pkg\nVersion: 1.0.0\n',
                         b'Metadata-Version: 2.4\nName: fixture-pkg\nVersion: 2.0.0\n',
                         b'Metadata-Version: 2.4\nName: fixture-pkg\nVersion: 1.0.0\nLicense-File: absent\n',
                         b'Metadata-Version: 2.4\nName: fixture-pkg\nVersion: 1.0.0\nLicense-File: ../escape\n'):
            self.rejected(wheel(metadata=metadata))
        for tags in (b'Tag: cp313-cp313-win_amd64\n', b'Tag: py3-none-any\nTag: py3-none-any\n'):
            self.rejected(wheel(wheel_metadata=b'Wheel-Version: 1.0\nRoot-Is-Purelib: true\n' + tags))
        self.rejected(wheel([(INFO + '/LICENSE', b'creates ambiguous notice')]))

    def test_expanded_member_notice_metadata_and_count_limits_fail_closed(self):
        for limit in ('MAX_MEMBER', 'MAX_EXPANDED', 'MAX_TOTAL', 'MAX_METADATA', 'MAX_NOTICE', 'MAX_MEMBERS', 'MAX_DEPTH'):
            with self.subTest(limit=limit): self.rejected(patches={limit: 1})

    def test_corrupt_crc_encryption_and_unsupported_compression_reject(self):
        for offset, value, fmt in ((16, 0, '<I'), (8, 1, '<H'), (24, stager.MAX_MEMBER + 1, '<I')):
            data = bytearray(wheel()); position = data.index(b'PK\x01\x02')
            struct.pack_into(fmt, data, position + offset, value)
            self.rejected(bytes(data))
        self.rejected(wheel(compression=zipfile.ZIP_BZIP2))

    def test_missing_extra_alternate_and_sdist_inputs_are_rejected(self):
        for name in ('alternate.whl', 'fixture_pkg-1.0.0.tar.gz', '.partial'):
            extra = self.inputs / name; extra.write_bytes(b'unselected')
            self.rejected(); extra.unlink()
        with mock.patch.object(stager, 'selection', return_value=[item(wheel())]):
            self.source.unlink()
            result = stager.stage(self.inputs, self.output)
        self.assertFalse(result['staged'])
        self.assertFalse(self.output.exists())

    def test_wrong_digest_is_rejected_before_zip_inspection(self):
        with mock.patch.object(stager, 'inspect_wheel') as inspect:
            data = wheel(); self.source = self.inputs / FILENAME; self.source.write_bytes(data)
            expected = item(data); expected['sha256'] = '0' * 64
            with mock.patch.object(stager, 'selection', return_value=[expected]):
                result = stager.stage(self.inputs, self.output)
        self.assertFalse(result['staged']); inspect.assert_not_called()
        self.assertFalse(self.output.exists())

    def test_existing_destination_and_parent_symlinks_do_not_clobber(self):
        self.output.mkdir(); sentinel = self.output / 'keep'; sentinel.write_bytes(b'keep')
        self.assertFalse(self.stage()['staged']); self.assertEqual(sentinel.read_bytes(), b'keep')
        alias = self.root / 'alias'; alias.symlink_to(self.root, target_is_directory=True)
        self.output = alias / 'fresh'; self.rejected()
        self.output = self.root / 'fresh'; self.inputs = alias / 'inputs'; self.rejected()

    def test_input_symlink_and_hardlink_are_rejected(self):
        data = wheel(); source = self.root / 'source'; source.write_bytes(data)
        path = self.inputs / FILENAME
        for hard in (False, True):
            if hard: path.hardlink_to(source)
            else: path.symlink_to(source)
            with mock.patch.object(stager, 'selection', return_value=[item(data)]):
                result = stager.stage(self.inputs, self.output)
            self.assertFalse(result['staged']); self.assertFalse(self.output.exists()); path.unlink()

    def test_input_change_after_inspection_rolls_back(self):
        original = stager.inspect_wheel
        def change(*args):
            result = original(*args)
            with self.source.open('r+b') as output: output.write(b'changed')
            return result
        self.rejected(patches={'inspect_wheel': change})

    def test_copy_failure_cleanup_failure_and_unknown_identity_remain_failed(self):
        failure = mock.Mock(side_effect=OSError('sensitive local path'))
        result = self.rejected(patches={'inspect_wheel': failure})
        self.assertNotIn('sensitive', result['failure'])
        with mock.patch.object(stager.shutil, 'rmtree', side_effect=OSError('private')):
            result = self.stage(patches={'inspect_wheel': failure})
        self.assertFalse(result['staged']); self.assertTrue(self.output.exists())
        self.assertEqual(result['preceding_failure'], 'Unreadable or invalid wheelhouse input/output')
        self.assertFalse((self.output / 'manifest.json').exists())
        self.output = self.root / 'unknown'
        original = os.fstat
        def fail_directory(fd):
            info = original(fd)
            if stat.S_ISDIR(info.st_mode): raise OSError('unknown destination identity')
            return info
        with mock.patch.object(stager.os, 'fstat', side_effect=fail_directory): result = self.stage()
        self.assertFalse(result['staged']); self.assertTrue(self.output.exists())
        self.assertEqual(result['failure'], 'Partial-output cleanup failed; destination retained for recovery')

    def test_decompression_errors_get_sanitized_receipts(self):
        self.rejected(patches={'inspect_wheel': mock.Mock(side_effect=stager.zlib.error('untrusted detail'))})


@unittest.skipUnless(os.name == 'posix' and hasattr(os, 'O_NOFOLLOW'), 'POSIX source pin checks')
class WheelhouseSelectionTests(unittest.TestCase):
    def selection_fixture(self, plan, expected=1, bad_lock=False):
        entry = item(wheel())
        locked = {'package': [{'name': entry['name'], 'version': entry['version'], 'wheels': [
            {'url': entry['url'], 'size': entry['bytes'], 'hash': 'sha256:' + ('0' * 64 if bad_lock else entry['sha256'])}]}]}
        with mock.patch.object(stager, 'pinned_bytes', return_value=json.dumps(plan).encode()), \
                mock.patch.object(stager.tomllib, 'loads', return_value=locked), \
                mock.patch.object(stager, 'PACKAGE_COUNT', expected):
            return stager.selection()

    def test_reviewed_production_selection_is_exact_without_network(self):
        plan = stager.selection()
        self.assertEqual(len(plan), 58)
        self.assertEqual(sum(x['bytes'] for x in plan), 94887529)
        self.assertEqual(len({x['name'] for x in plan}), 58)

    def test_plan_lock_and_project_changes_fail_closed(self):
        for pin in ('PLAN_SHA', 'LOCK_SHA', 'PROJECT_SHA'):
            with self.subTest(pin=pin), mock.patch.object(stager, pin, '0' * 64), self.assertRaises(stager.StagingError):
                stager.selection()

    def test_missing_duplicate_ambiguous_or_changed_artifact_declarations_reject(self):
        entry = item(wheel())
        self.assertEqual(self.selection_fixture([entry]), [entry])
        for plan, count in (([], 1), ([entry, entry], 2), ([entry], 2)):
            with self.subTest(plan=plan, count=count), self.assertRaises(stager.StagingError):
                self.selection_fixture(plan, count)
        with self.assertRaises(stager.StagingError):
            self.selection_fixture([entry], bad_lock=True)
        for changed in ({**entry, 'url': 'https://unreviewed.invalid/' + FILENAME},
                        {**entry, 'bytes': True}, {**entry, 'sha256': 'not-a-hash'},
                        {**entry, 'version': '2.0.0'}):
            with self.subTest(changed=changed), self.assertRaises(stager.StagingError):
                self.selection_fixture([changed])


if __name__ == '__main__':
    unittest.main()
