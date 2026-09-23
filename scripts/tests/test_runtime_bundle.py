"""Synthetic inventory tests; no runtime downloads or executable launches."""
import copy
import hashlib
import importlib.util
import json
import os
import shutil
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'verify_runtime_bundle.py'
SPEC = importlib.util.spec_from_file_location('runtime_bundle', SCRIPT)
validator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(validator)
POLICY = json.loads(validator.POLICY_PATH.read_text())


class RuntimeInventoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.root = self.directory / 'bundle'
        self.root.mkdir()
        self.inventory = self.directory / 'inventory.json'
        self.manifest = self.fixture('macos-aarch64')

    def fixture(self, target, languages=None, regions=None):
        languages = ['eng'] if languages is None else languages
        regions = [] if regions is None else regions
        ids = POLICY['common'] + POLICY['targets'][target]
        ids += ['ocr-language:' + x for x in languages] + ['region:' + x for x in regions]
        manifest = {'schema_version': 1, 'target': target, 'ocr_languages': languages,
                    'regions': regions, 'components': [], 'files': []}
        for index, name in enumerate(ids):
            path = f'assets/{index:03d}.bin'
            content = f'SYNTHETIC INVENTORY FIXTURE {name}\n'.encode()
            file = self.root / path
            file.parent.mkdir(exist_ok=True)
            file.write_bytes(content)
            version = '21.0.12.1+1' if name == 'java-runtime' else '1.2.3'
            manifest['components'].append({'id': name, 'version': version, 'files': [path]})
            manifest['files'].append({'path': path, 'size': len(content), 'sha256': hashlib.sha256(content).hexdigest()})
        return manifest

    def verify(self):
        self.inventory.write_text(json.dumps(self.manifest), encoding='utf-8')
        return validator.verify(self.root, self.inventory, self.manifest['target'])

    def failed(self, code=None):
        result = self.verify()
        self.assertFalse(result['complete'])
        self.assertTrue(result['errors'])
        if code:
            self.assertIn(code, [e['code'] for e in result['errors']])
        return result

    def test_complete_target_inventories_and_advertised_assets(self):
        for target in POLICY['targets']:
            with self.subTest(target=target):
                shutil.rmtree(self.root)
                self.root.mkdir()
                self.manifest = self.fixture(target, ['eng', 'deu'], ['synthetic-region'])
                result = self.verify()
                self.assertTrue(result['complete'], result)
                self.assertEqual(len(self.manifest['files']), result['verified_files'])
                self.assertEqual(sum(f['size'] for f in self.manifest['files']), result['verified_bytes'])

    def test_empty_development_inventory_is_incomplete(self):
        self.manifest['files'] = []
        self.manifest['components'] = []
        result = self.failed('missing_components')
        self.assertIn('java-runtime', result['missing_components'])
        self.assertIn('ocr-language:eng', result['missing_components'])

    def test_windows_requires_fixed_webview(self):
        self.manifest['target'] = 'windows-x86_64'
        result = self.failed('missing_components')
        self.assertEqual(['webview2-fixed'], result['missing_components'])

    def test_additional_advertised_assets_must_exist(self):
        self.manifest['ocr_languages'].append('fra')
        self.manifest['regions'].append('synthetic-region')
        result = self.failed('missing_components')
        self.assertEqual(['ocr-language:fra', 'region:synthetic-region'], result['missing_components'])

    def test_english_cannot_be_removed_from_advertisement(self):
        self.manifest['ocr_languages'] = []
        self.failed('missing_advertised_language')

    def test_wrong_target(self):
        self.verify()
        result = validator.verify(self.root, self.inventory, 'macos-x86_64')
        self.assertFalse(result['complete'])
        self.assertEqual('invalid_inventory', result['errors'][0]['code'])

    def test_missing_and_changed_file_identify_component(self):
        path = self.manifest['files'][0]['path']
        (self.root / path).unlink()
        result = self.failed('missing_file')
        self.assertIn('application', result['invalid_components'])
        (self.root / path).write_bytes(b'X' * self.manifest['files'][0]['size'])
        self.failed('hash_mismatch')
        (self.root / path).write_bytes(b'X')
        self.failed('size_mismatch')

    def test_unlisted_files_and_undeclared_references(self):
        (self.root / 'extra.bin').write_bytes(b'Unlisted')
        self.failed('unlisted_file')
        self.manifest['files'].pop(0)
        self.failed('missing_file_declaration')

    def test_unowned_file(self):
        self.manifest['components'].pop(0)
        self.failed('unowned_file')

    def test_unsafe_paths(self):
        original = copy.deepcopy(self.manifest)
        for path in ('../escape', '/absolute', 'C:/escape', 'a\\b', 'a//b', './a', 'a/../b',
                     'a\x00b', 'NUL.txt', 'com1', 'a.', 'a ', 'a:stream', 'a\nb', 'a/' * 65 + 'b'):
            with self.subTest(path=path):
                self.manifest = copy.deepcopy(original)
                self.manifest['files'][0]['path'] = path
                self.failed('invalid_inventory')

    def test_duplicate_components_and_file_references(self):
        original = copy.deepcopy(self.manifest)
        self.manifest['components'].append(copy.deepcopy(self.manifest['components'][0]))
        self.failed('invalid_inventory')
        self.manifest = original
        self.manifest['components'][0]['files'] *= 2
        self.failed('invalid_inventory')

    def test_duplicate_or_colliding_file_declarations(self):
        original = copy.deepcopy(self.manifest)
        for path in ('assets/000.bin', 'ASSETS/000.BIN'):
            self.manifest = copy.deepcopy(original)
            entry = copy.deepcopy(self.manifest['files'][0])
            entry['path'] = path
            self.manifest['files'].append(entry)
            self.failed('invalid_inventory')
        self.assertEqual(validator.path_key('caf\u00e9/a', 64), validator.path_key('cafe\u0301/a', 64))

    def test_linked_file_and_directory_are_not_followed(self):
        outside = self.directory / 'outside'
        outside.mkdir()
        (outside / 'secret').write_bytes(b'Synthetic outside file')
        try:
            (self.root / 'linked-directory').symlink_to(outside, target_is_directory=True)
            (self.root / 'linked-file').symlink_to(outside / 'secret')
        except OSError as exc:
            self.skipTest(f'Host does not permit symlink creation: {exc.errno}')
        result = self.failed('unsafe_link')
        self.assertEqual(2, sum(e['code'] == 'unsafe_link' for e in result['errors']))
        self.assertNotIn('secret', json.dumps(result))

    @unittest.skipUnless(os.name == 'nt', 'Windows junction-specific check')
    def test_windows_directory_junction(self):
        outside = self.directory / 'junction-target'
        outside.mkdir()
        junction = self.root / 'junction'
        # Paths come from TemporaryDirectory, never user input or a manifest.
        subprocess.run(['cmd', '/c', 'mklink', '/J', str(junction), str(outside)], check=True, capture_output=True)
        self.failed('unsafe_link')

    def test_hardlink_is_rejected(self):
        os.link(self.root / self.manifest['files'][0]['path'], self.directory / 'hardlink')
        self.failed('unsafe_file')

    def test_linked_root_is_rejected(self):
        alias = self.directory / 'alias'
        try:
            alias.symlink_to(self.root, target_is_directory=True)
        except OSError as exc:
            self.skipTest(f'Host does not permit symlink creation: {exc.errno}')
        self.verify()
        result = validator.verify(alias, self.inventory, 'macos-aarch64')
        self.assertFalse(result['complete'])

    def test_invalid_versions_and_engine_mismatch(self):
        original = copy.deepcopy(self.manifest)
        for version in ('latest', '^1.0.0', '>=1.0', '', None, ['1.0']):
            self.manifest = copy.deepcopy(original)
            self.manifest['components'][0]['version'] = version
            self.failed('invalid_version')
        self.manifest = original
        for c in self.manifest['components']:
            if c['id'] in ('duckdb-spatial', 'playwright-driver', 'java-runtime'):
                c['version'] = '2.0.0'
        result = self.failed('incompatible_version')
        self.assertEqual(3, sum(e['code'] == 'incompatible_version' for e in result['errors']))

    def test_unknown_empty_or_unadvertised_components(self):
        original = copy.deepcopy(self.manifest)
        for name in ('unknown', 'ocr-language:fra', 'webview2-fixed'):
            self.manifest = copy.deepcopy(original)
            self.manifest['components'][0]['id'] = name
            self.failed('invalid_inventory')
        self.manifest = original
        self.manifest['components'][0]['files'] = []
        self.failed('component_files')

    def test_bad_types_schemas_hashes_and_sizes(self):
        original = copy.deepcopy(self.manifest)
        mutations = [('schema_version', 2), ('schema_version', True), ('components', {}),
                     ('files', None), ('ocr_languages', ['eng', 'eng']), ('regions', ['../outside'])]
        for key, value in mutations:
            self.manifest = copy.deepcopy(original)
            self.manifest[key] = value
            self.failed('invalid_inventory')
        for key, value in [('size', -1), ('size', True), ('size', 2**40), ('sha256', 'bad')]:
            self.manifest = copy.deepcopy(original)
            self.manifest['files'][0][key] = value
            self.failed('invalid_inventory')
        self.manifest = original
        self.manifest['unexpected'] = True
        self.failed('invalid_inventory')

    def test_malformed_duplicate_and_oversized_json(self):
        for raw in ('{', '{"schema_version":1,"schema_version":1}', 'x' * (validator.MAX_JSON_BYTES + 1)):
            self.inventory.write_text(raw)
            result = validator.verify(self.root, self.inventory, 'macos-aarch64')
            self.assertFalse(result['complete'])
            self.assertEqual('invalid_inventory', result['errors'][0]['code'])

    def test_bounded_errors(self):
        for i in range(250):
            (self.root / f'unlisted-{i}').write_bytes(b'')
        result = self.failed('unlisted_file')
        self.assertTrue(result['errors_truncated'])
        self.assertEqual(POLICY['limits']['errors'], len(result['errors']))

    def test_unreadable_input_does_not_leak_absolute_paths(self):
        result = validator.verify(self.root, self.directory / 'absent', 'macos-aarch64')
        self.assertFalse(result['complete'])
        self.assertNotIn(str(self.directory), json.dumps(result))

    def test_cli_exit_codes_remain_fail_closed_under_python_optimization(self):
        self.verify()
        args = [sys.executable, '-O', str(SCRIPT), '--bundle', str(self.root),
                '--inventory', str(self.inventory), '--target', 'macos-aarch64']
        complete = subprocess.run(args, text=True, capture_output=True)
        self.assertEqual(0, complete.returncode, complete.stderr)
        self.assertTrue(json.loads(complete.stdout)['complete'])
        (self.root / self.manifest['files'][0]['path']).write_bytes(b'changed')
        incomplete = subprocess.run(args, text=True, capture_output=True)
        self.assertEqual(1, incomplete.returncode)
        self.assertFalse(json.loads(incomplete.stdout)['complete'])


if __name__ == '__main__':
    unittest.main()
