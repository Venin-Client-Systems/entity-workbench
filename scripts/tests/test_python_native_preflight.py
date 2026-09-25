"""Synthetic static metadata only; no staged Python/package execution."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import inspect_python_native_layout as preflight


def native(path, commands):
    return {'locations': [path], 'commands': [dict(command=kind, value=value) for kind, value in commands]}


class StaticMetadataTests(unittest.TestCase):
    def test_only_reviewed_header_spread_is_accepted(self):
        self.assertEqual(preflight.proposed_location(preflight.HEADER),
                         'install/include/python3.13/igraph/igraphmodule_api.h')
        self.assertEqual(preflight.proposed_location('package/module.py'), preflight.SITE + 'package/module.py')
        for path in ('other.data/scripts/run', '../escape', '/absolute', 'a/../b', 'a\\b', 'a\x00b'):
            with self.subTest(path=path), self.assertRaises(ValueError):
                preflight.proposed_location(path)

    def test_entry_point_case_is_preserved_and_never_evaluated(self):
        value = preflight.entry_points(b'[plugins]\nMixed.Name = example.module:Thing\n')
        self.assertEqual(value, {'plugins': {'Mixed.Name': 'example.module:Thing'}})

    def test_defaults_duplicate_names_and_oversize_hooks_reject(self):
        for data in (b'[DEFAULT]\nx=y\n[plugins]\nx=z\n', b'[plugins]\nx=a\nx=b\n', b'x' * (1024**2 + 1)):
            with self.subTest(data=data[:20]), self.assertRaises((ValueError, preflight.configparser.Error)):
                preflight.entry_points(data)

    def test_loader_command_parser_excludes_arbitrary_tool_prefix(self):
        parsed = preflight.load_commands(b'/private/build/path/input:\nLoad command 0\n cmd LC_LOAD_DYLIB\n name @rpath/local.dylib (offset 24)\n')
        self.assertEqual(parsed, [{'command': 'LC_LOAD_DYLIB', 'value': '@rpath/local.dylib'}])

    def test_malformed_and_control_loader_fields_reject(self):
        for data in (b'no load commands', b'Load command 0\n cmd LC_RPATH\n',
                     b'Load command 0\n cmd LC_RPATH\n path @loader_path/\x00bad (offset 12)\n',
                     b'x' * (2 * 1024**2 + 1)):
            with self.subTest(data=data[:20]), self.assertRaises(ValueError):
                preflight.load_commands(data)

    def test_local_system_weak_and_library_identity_are_distinct(self):
        values = [native(preflight.EXECUTABLE, [('LC_RPATH', '@executable_path/../lib'),
                                               ('LC_LOAD_DYLIB', '@rpath/library.dylib')]),
                  native('install/lib/library.dylib', [('LC_ID_DYLIB', '/not/a/load/dependency'),
                         ('LC_LOAD_WEAK_DYLIB', '/System/Library/Frameworks/Example.framework/Example')])]
        result = preflight.resolve(values)
        self.assertEqual(result['edge_counts'], {'local': 1, 'os-library': 1})
        self.assertEqual(len(result['edges']), 2)
        self.assertEqual(result['library_identities'][0]['identity'], '/not/a/load/dependency')

    def test_missing_and_ambiguous_dependencies_remain_unresolved(self):
        values = [native(preflight.EXECUTABLE, [('LC_RPATH', '@executable_path/../lib')]),
                  native('install/lib/sub/extension.so', [('LC_RPATH', '@loader_path'),
                         ('LC_LOAD_DYLIB', '@rpath/duplicate.dylib'), ('LC_LOAD_DYLIB', '/outside/unknown')]),
                  native('install/lib/duplicate.dylib', []), native('install/lib/sub/duplicate.dylib', [])]
        result = preflight.resolve(values)
        self.assertEqual(result['edge_counts'], {'ambiguous': 1, 'unresolved': 1})

    def test_parent_escape_and_unreviewed_rpath_are_rejected(self):
        with self.assertRaises(ValueError):
            preflight.expand('@loader_path/../../../../outside', 'install/lib/library')
        with self.assertRaises(ValueError):
            preflight.resolve([native(preflight.EXECUTABLE, [('LC_RPATH', '/outside')])])

    @unittest.skipUnless(os.name == 'posix', 'POSIX report permissions')
    def test_no_clobber_report(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'report.json'
            preflight.write_json(path, {'complete_release': False})
            initial = path.read_bytes()
            with self.assertRaises(FileExistsError):
                preflight.write_json(path, {})
            self.assertEqual(path.read_bytes(), initial)

    @unittest.skipUnless(os.name == 'posix', 'POSIX scratch permissions')
    def test_tool_failure_and_oversize_output_cannot_publish_output(self):
        with tempfile.TemporaryDirectory() as directory:
            scratch = Path(directory)
            for status, content in ((1, b'error'), (0, b'x' * 9)):
                def run(*args, **kwargs):
                    kwargs['stdout'].write(content)
                    return subprocess.CompletedProcess(args[0], status)
                with patch.object(preflight.subprocess, 'run', side_effect=run), self.assertRaises(ValueError):
                    preflight.run_tool(['synthetic-tool'], scratch, 8)
                self.assertFalse((scratch / 'tool-output.tmp').exists())

    @unittest.skipUnless(os.name == 'posix', 'POSIX scratch permissions')
    def test_tool_timeout_retains_failure_and_removes_temporary_output(self):
        with tempfile.TemporaryDirectory() as directory:
            scratch = Path(directory)
            with patch.object(preflight.subprocess, 'run', side_effect=subprocess.TimeoutExpired('tool', 10)), \
                    self.assertRaises(subprocess.TimeoutExpired):
                preflight.run_tool(['synthetic-tool'], scratch, 8)
            self.assertFalse((scratch / 'tool-output.tmp').exists())


if __name__ == '__main__':
    unittest.main()
