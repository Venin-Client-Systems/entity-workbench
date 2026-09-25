"""Synthetic installation fixtures; candidate interpreter/package bytes never run."""
from contextlib import ExitStack
import csv
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
from unittest.mock import patch
import warnings
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import install_python_offline as installer
import verify_python_install as verifier


def sha(data):
    return hashlib.sha256(data).hexdigest()


def make_wheel(extra=(), record_change=None, name='fixture_pkg'):
    info = name + '-1.0.0.dist-info'
    entries = [(name + '/__init__.py', b'raise RuntimeError("fixture must never execute")\n'),
               (info + '/METADATA', ('Metadata-Version: 2.4\nName: ' + name.replace('_', '-') + '\nVersion: 1.0.0\nLicense-File: LICENSE\n\n').encode()),
               (info + '/WHEEL', b'Wheel-Version: 1.0\nRoot-Is-Purelib: true\nTag: py3-none-any\n'),
               (info + '/licenses/LICENSE', b'SYNTHETIC notice\n'),
               (info + '/entry_points.txt', b'[console_scripts]\nfixture-command = fixture_pkg:main\n'),
               ('fixture.pth', b'import never_execute_fixture\n'), *extra]
    rows = [[path if isinstance(path, str) else path.filename, verifier.record_hash(sha(data)), str(len(data))]
            for path, data in entries]
    rows.append([info + '/RECORD', '', ''])
    if record_change:
        rows = record_change(rows)
    text = io.StringIO(newline='')
    csv.writer(text, lineterminator='\n').writerows(rows)
    record = text.getvalue().encode()
    output = io.BytesIO()
    with warnings.catch_warnings():
        warnings.simplefilter('ignore', UserWarning)
        with zipfile.ZipFile(output, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
            for path, data in entries + [(info + '/RECORD', record)]:
                archive.writestr(path, data)
    data = output.getvalue()
    item = {'name': name.replace('_', '-'), 'version': '1.0.0', 'filename': name + '-1.0.0-py3-none-any.whl',
            'bytes': len(data), 'sha256': sha(data), 'url': 'https://files.pythonhosted.org/synthetic.whl'}
    return data, item, record


class PortableInstallTests(unittest.TestCase):
    def test_unsupported_host_refuses_before_opening_sources(self):
        with patch.object(installer.os, 'name', 'nt'), patch.object(installer, 'installation_plan') as plan:
            report = installer.install('unused', 'unused', 'unused')
        self.assertFalse(report['assembled'])
        self.assertEqual(report['failure'], 'POSIX no-follow installation required')
        plan.assert_not_called()

    def test_record_paths_hashes_and_blank_self_are_explicit(self):
        record = verifier.SITE + '/fixture-1.0.dist-info/RECORD'
        header = 'install/include/python3.13/igraph/igraphmodule_api.h'
        entries = {header: {'bytes': 4, 'sha256': sha(b'four')}}
        data = installer.installed_record(record, [header, record], entries)
        rows = list(csv.reader(io.StringIO(data.decode())))
        self.assertEqual(rows[0], ['../../../include/python3.13/igraph/igraphmodule_api.h', verifier.record_hash(sha(b'four')), '4'])
        self.assertEqual(rows[1], ['fixture-1.0.dist-info/RECORD', '', ''])
        verifier.validate_record(data, record, [header, record], entries)


@unittest.skipUnless(os.name == 'posix' and hasattr(os, 'O_NOFOLLOW'), 'POSIX descriptor installation tests')
class OfflineInstallationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.runtime, self.wheelhouse, self.output = [self.base / name for name in ('runtime', 'wheelhouse', 'output')]
        self.runtime.mkdir(); self.wheelhouse.mkdir()
        self.runtime_files = {'install/bin/python3.13': (b'SYNTHETIC NOT AN EXECUTABLE\n', True),
                              'licenses/LICENSE.fixture.txt': (b'SYNTHETIC runtime notice\n', False),
                              'provenance.json': (b'{"source":"synthetic"}\n', False)}
        self.data, self.item, self.original_record = make_wheel()

    def prepare(self):
        entries = {}
        for path, (data, executable) in self.runtime_files.items():
            destination = self.runtime / path
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
            destination.chmod(0o755 if executable else 0o644)
            entries[path] = {'bytes': len(data), 'sha256': sha(data), 'executable': executable}
        raw = (json.dumps({'files': entries}, sort_keys=True) + '\n').encode()
        (self.runtime / 'manifest.json').write_bytes(raw)
        (self.runtime / 'manifest.json').chmod(0o644)
        (self.wheelhouse / self.item['filename']).write_bytes(self.data)
        return raw

    def context(self, raw, patches=None):
        stack = ExitStack()
        plan = {'wheels': 1, 'runtime_manifest_destination': 'review/cpython/manifest.json'}
        stack.enter_context(patch.object(installer, 'installation_plan', return_value=(b'{"synthetic":true}\n', plan)))
        stack.enter_context(patch.object(installer.wheels, 'selection', return_value=[self.item]))
        stack.enter_context(patch.object(verifier, 'RUNTIME_SHA', sha(raw)))
        stack.enter_context(patch.object(verifier, 'WHEEL_COUNT', 1))
        for target, name, value in patches or []:
            stack.enter_context(patch.object(target, name, value))
        return stack

    def install(self, patches=None):
        raw = self.prepare()
        with self.context(raw, patches):
            return installer.install(self.runtime, self.wheelhouse, self.output)

    def rejected(self, patches=None):
        report = self.install(patches)
        self.assertFalse(report['assembled'], report)
        self.assertFalse(report['complete_release'])
        self.assertFalse(self.output.exists(), report)
        return report

    def test_preserves_bytes_notices_pth_original_record_and_omits_execution(self):
        report = self.install()
        self.assertTrue(report['assembled'], report)
        self.assertFalse(report['package_code_executed'])
        self.assertEqual(report['runtime_components_satisfied'], [])
        for path, (data, _) in self.runtime_files.items():
            self.assertEqual((self.output / path).read_bytes(), data)
        self.assertEqual((self.output / (verifier.SITE + '/fixture.pth')).read_bytes(), b'import never_execute_fixture\n')
        self.assertEqual((self.output / 'review/fixture-pkg/fixture_pkg-1.0.0.dist-info/RECORD').read_bytes(), self.original_record)
        self.assertFalse((self.output / 'install/bin/fixture-command').exists())
        self.assertNotIn('fixture_pkg', sys.modules)
        self.assertNotIn('never_execute_fixture', sys.modules)
        self.assertFalse(any(self.output.rglob('*.pyc')))

    def test_deterministic_manifest_and_separate_reader(self):
        first = self.install(); self.assertTrue(first['assembled'], first)
        first_raw = (self.output / 'manifest.json').read_bytes()
        self.output = self.base / 'second'
        second = self.install(); self.assertTrue(second['assembled'], second)
        self.assertEqual(first_raw, (self.output / 'manifest.json').read_bytes())
        raw = (self.runtime / 'manifest.json').read_bytes()
        with self.context(raw):
            self.assertTrue(verifier.verify(self.output, second['manifest_sha256'])['verified'])

    def test_header_record_is_relocated_and_original_record_retained(self):
        self.data, self.item, self.original_record = make_wheel([(installer.layout.HEADER, b'SYNTHETIC header\n')], name='igraph')
        report = self.install(); self.assertTrue(report['assembled'], report)
        original = (self.output / 'review/igraph/igraph-1.0.0.dist-info/RECORD').read_bytes()
        installed = (self.output / (verifier.SITE + '/igraph-1.0.0.dist-info/RECORD')).read_bytes()
        self.assertEqual(original, self.original_record)
        self.assertIn(b'igraph-1.0.0.data/headers/igraphmodule_api.h', original)
        self.assertIn(b'../../../include/python3.13/igraph/igraphmodule_api.h', installed)
        self.assertNotIn(b'igraph-1.0.0.data/', installed)
        self.assertEqual((self.output / 'install/include/python3.13/igraph/igraphmodule_api.h').read_bytes(), b'SYNTHETIC header\n')

    def test_wrong_archive_digest_is_rejected_before_zip_access(self):
        self.item['sha256'] = '0' * 64
        with patch.object(installer, 'install_wheel') as wheel:
            self.rejected()
        wheel.assert_not_called()

    def test_unsafe_duplicate_and_colliding_archive_members(self):
        for path in ('../escape', '/absolute', 'a\\b', 'A./file', 'fixture_pkg/__init__.py',
                     'FIXTURE_PKG/__init__.py', 'fixture_pkg/__init__.py/child'):
            with self.subTest(path=path):
                self.data, self.item, self.original_record = make_wheel([(path, b'bad')])
                self.rejected()
        self.data, self.item, self.original_record = make_wheel([('caf\u00e9', b'a'), ('cafe\u0301', b'b')])
        self.rejected()

    def test_special_archive_members_and_unreviewed_spread_reject(self):
        for kind in (stat.S_IFLNK, stat.S_IFIFO):
            member = zipfile.ZipInfo('special'); member.create_system = 3; member.external_attr = (kind | 0o644) << 16
            self.data, self.item, self.original_record = make_wheel([(member, b'target')])
            self.rejected()
        self.data, self.item, self.original_record = make_wheel([('fixture_pkg-1.0.0.data/scripts/run', b'bad')])
        self.rejected()

    def test_source_record_duplicates_omissions_and_wrong_hashes_reject(self):
        changes = [lambda rows: rows + [rows[0]], lambda rows: rows[1:],
                   lambda rows: [[rows[0][0], 'sha256=wrong', rows[0][2]]] + rows[1:],
                   lambda rows: rows[:-1] + [[rows[-1][0], 'sha256=wrong', '1']]]
        for change in changes:
            self.data, self.item, self.original_record = make_wheel(record_change=change)
            self.rejected()

    def test_corrupt_crc_is_rejected_even_with_matching_archive_pin(self):
        data = bytearray(self.data)
        start = data.index(b'PK\x01\x02')
        struct.pack_into('<I', data, start + 16, 0)
        self.data = bytes(data); self.item.update(bytes=len(data), sha256=sha(data))
        self.rejected()

    def test_runtime_and_wheel_collision_preserves_immutable_input(self):
        path = verifier.SITE + '/fixture_pkg/__init__.py'
        self.runtime_files[path] = (b'ORIGINAL runtime file\n', False)
        self.rejected()
        self.assertEqual((self.runtime / path).read_bytes(), b'ORIGINAL runtime file\n')

    def test_existing_destination_and_symlinked_parents_are_not_clobbered(self):
        self.output.mkdir(); (self.output / 'sentinel').write_bytes(b'KEEP')
        result = self.install(); self.assertFalse(result['assembled'])
        self.assertEqual((self.output / 'sentinel').read_bytes(), b'KEEP')
        alias = self.base / 'alias'; alias.symlink_to(self.base, target_is_directory=True)
        self.output = alias / 'fresh'; self.rejected()

    def test_linked_input_file_and_parent_reject(self):
        raw = self.prepare()
        input_file = self.wheelhouse / self.item['filename']; input_file.unlink()
        outside = self.base / 'outside.whl'; outside.write_bytes(self.data)
        for hard in (False, True):
            if hard: input_file.hardlink_to(outside)
            else: input_file.symlink_to(outside)
            with self.context(raw):
                self.assertFalse(installer.install(self.runtime, self.wheelhouse, self.output)['assembled'])
            self.assertFalse(self.output.exists()); input_file.unlink()
        input_file.write_bytes(self.data)
        alias = self.base / 'alias'; alias.symlink_to(self.base, target_is_directory=True)
        with self.context(raw):
            self.assertFalse(installer.install(alias / 'runtime', self.wheelhouse, self.output)['assembled'])

    def test_output_inside_either_immutable_input_is_rejected_before_creation(self):
        for source in (self.runtime, self.wheelhouse):
            self.output = source / 'output'
            result = self.rejected()
            self.assertEqual(result['failure'], 'Output must not be inside an immutable input')

    def test_changed_runtime_source_after_copy_causes_rollback(self):
        original = installer.install_wheel
        def changed(*args, **kwargs):
            value = original(*args, **kwargs)
            (self.runtime / 'licenses/LICENSE.fixture.txt').write_bytes(b'CHANGED')
            return value
        self.rejected([(installer, 'install_wheel', changed)])

    def test_changed_wheel_source_after_inspection_causes_rollback(self):
        original = installer.install_wheel
        def changed(*args, **kwargs):
            value = original(*args, **kwargs)
            (self.wheelhouse / self.item['filename']).write_bytes(b'CHANGED')
            return value
        self.rejected([(installer, 'install_wheel', changed)])

    def test_completed_copy_corruption_is_caught_by_independent_reader(self):
        original = verifier.verify
        def changed(path, digest):
            (path / 'licenses/LICENSE.fixture.txt').write_bytes(b'CHANGED')
            return original(path, digest)
        self.rejected([(verifier, 'verify', changed)])

    def test_cleanup_failure_is_incomplete_and_retains_preceding_failure(self):
        self.item['sha256'] = '0' * 64
        with patch.object(installer.shutil, 'rmtree', side_effect=OSError('/private/unpublishable/path')):
            result = self.install()
        self.assertFalse(result['assembled'])
        self.assertTrue(self.output.exists())
        self.assertIn('cleanup failed', result['failure'])
        self.assertEqual(result['preceding_failure'], 'Wheel digest mismatch')
        self.assertNotIn('unpublishable', json.dumps(result))

    def test_output_limits_fail_without_partial_success(self):
        for target, name, value in ((verifier, 'MAX_TOTAL', 10), (verifier, 'MAX_FILE', 10),
                                    (verifier, 'MAX_FILES', 2), (installer, 'MAX_WHEEL_EXPANDED', 10)):
            with self.subTest(limit=name): self.rejected([(target, name, value)])

    def test_unknown_new_root_identity_retains_incomplete_output_safely(self):
        original_directory, original_stat = installer.files.directory_at, installer.os.fstat
        opened = set()
        def directory(parent, parts, create=False):
            result = original_directory(parent, parts, create)
            if parts == ['output']:
                opened.add(result)
            return result
        def fstat(fd):
            if fd in opened:
                raise OSError('/private/root-identity-unavailable')
            return original_stat(fd)
        result = self.install([(installer.files, 'directory_at', directory), (installer.os, 'fstat', fstat)])
        self.assertFalse(result['assembled'])
        self.assertTrue(self.output.exists())
        self.assertIn('cleanup failed', result['failure'])
        self.assertEqual(result['preceding_failure'], 'Unreadable or invalid installation input/output')
        self.assertNotIn('/private/', json.dumps(result))

    def test_independent_reader_rejects_extra_link_wrong_mode_and_record(self):
        report = self.install(); self.assertTrue(report['assembled'], report)
        raw = (self.runtime / 'manifest.json').read_bytes()
        with self.context(raw):
            self.assertFalse(verifier.verify(self.output, '0' * 64)['verified'])
            extra = self.output / 'extra'; extra.write_bytes(b'extra')
            self.assertFalse(verifier.verify(self.output, report['manifest_sha256'])['verified']); extra.unlink()
            extra.symlink_to(self.runtime)
            self.assertFalse(verifier.verify(self.output, report['manifest_sha256'])['verified']); extra.unlink()
            original_mode = (self.output / 'provenance.json').stat().st_mode
            (self.output / 'provenance.json').chmod(0o600)
            self.assertFalse(verifier.verify(self.output, report['manifest_sha256'])['verified'])
            (self.output / 'provenance.json').chmod(original_mode & 0o777)
            manifest = json.loads((self.output / 'manifest.json').read_bytes())
            record = next(iter(manifest['record_members']))
            corrupt = b'unknown,sha256=wrong,5\n'
            (self.output / record).write_bytes(corrupt)
            manifest['files'][record].update(bytes=len(corrupt), sha256=sha(corrupt))
            new = (json.dumps(manifest) + '\n').encode(); (self.output / 'manifest.json').write_bytes(new)
            self.assertFalse(verifier.verify(self.output, sha(new))['verified'])


if __name__ == '__main__':
    unittest.main()
