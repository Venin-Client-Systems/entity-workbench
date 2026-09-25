"""Synthetic offline inventory production; never executable-runtime evidence."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
import generate_runtime_inventory as producer

verifier = producer.verifier
POLICY = json.loads(verifier.POLICY_PATH.read_text())


class InventoryProducerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.bundle = self.directory / 'bundle'
        self.bundle.mkdir()
        self.plan_path = self.directory / 'ownership.json'
        self.inventory = self.directory / 'inventory.json'
        self.plan = self.fixture('macos-aarch64')

    def fixture(self, target):
        names = POLICY['common'] + POLICY['targets'][target] + ['ocr-language:eng']
        components = []
        for index, name in enumerate(names):
            relative = f'assets/{index:03d}.bin'
            path = self.bundle / relative
            path.parent.mkdir(exist_ok=True)
            path.write_bytes(f'SYNTHETIC inventory input {name}\n'.encode())
            components.append({'id': name, 'version': '21.0.12.1' if name == 'java-runtime' else '1.2.3',
                               'paths': [relative]})
        return {'schema_version': 1, 'target': target, 'ocr_languages': ['eng'], 'regions': [],
                'components': components}

    def generate(self, target=None):
        self.plan_path.write_text(json.dumps(self.plan))
        return producer.generate(self.bundle, self.plan_path, self.inventory, target or self.plan['target'])

    def rejected(self, detail=None):
        report = self.generate()
        self.assertFalse(report['generated'], report)
        self.assertFalse(report['complete'])
        self.assertFalse(report['complete_release'])
        self.assertFalse(self.inventory.exists())
        self.assertIsNotNone(report['failure'])
        if detail:
            self.assertIn(detail, report['failure'])
        self.assertFalse(list(self.directory.glob('.ew-inventory-*')))
        return report

    def test_all_targets_and_deterministic_output_pass_the_existing_verifier(self):
        for target in POLICY['targets']:
            with self.subTest(target=target):
                self.plan = self.fixture(target)
                self.inventory = self.directory / f'{target}.json'
                report = self.generate()
                self.assertTrue(report['generated'], report)
                self.assertTrue(report['complete'], report)
                self.assertFalse(report['complete_release'])
                first = self.inventory.read_bytes()
                self.assertEqual(report['inventory_sha256'], verifier.read_json(self.inventory)[1])
                self.assertTrue(verifier.verify(self.bundle, self.inventory, target)['complete'])
                self.plan['components'].reverse()
                self.inventory = self.directory / f'{target}-again.json'
                self.assertTrue(self.generate()['complete'])
                self.assertEqual(self.inventory.read_bytes(), first)
                for path in (self.bundle / 'assets').iterdir():
                    path.unlink()

    def test_partial_staging_is_inventoried_but_stays_incomplete(self):
        removed = self.plan['components'].pop()
        (self.bundle / removed['paths'][0]).unlink()
        report = self.generate()
        self.assertTrue(report['generated'], report)
        self.assertFalse(report['complete'])
        self.assertFalse(report['complete_release'])
        self.assertEqual(report['verification']['missing_components'], ['ocr-language:eng'])
        self.assertEqual([row['code'] for row in report['verification']['errors']], ['missing_components'])

    def test_literal_directories_expand_every_present_file_and_allow_explicit_shared_ownership(self):
        self.plan['components'] = [{'id': 'java-runtime', 'version': '21.0.12.1', 'paths': ['assets']},
                                   {'id': 'document-parser', 'version': '0.1.0', 'paths': ['assets/000.bin']}]
        (self.bundle / 'assets/new-support-file').write_bytes(b'SYNTHETIC support file')
        report = self.generate()
        self.assertTrue(report['generated'], report)
        self.assertFalse(report['complete'])
        inventory = json.loads(self.inventory.read_bytes())
        components = {row['id']: row for row in inventory['components']}
        self.assertIn('assets/new-support-file', components['java-runtime']['files'])
        self.assertEqual(components['document-parser']['files'], ['assets/000.bin'])
        self.assertEqual(len(inventory['files']), len(list((self.bundle / 'assets').iterdir())))

    def test_missing_unowned_glob_traversal_and_overlapping_selectors_fail(self):
        original = copy.deepcopy(self.plan)
        (self.bundle / 'empty-directory').mkdir()
        for selector in ['', 'missing', 'empty-directory', 'assets/*.bin', '../outside', '/absolute', 'assets/../assets/000.bin', 'assets/000.bin:stream']:
            with self.subTest(selector=selector):
                self.plan = copy.deepcopy(original)
                self.plan['components'][0]['paths'] = [selector]
                self.rejected()
        self.plan = copy.deepcopy(original)
        self.plan['components'][0]['paths'] = ['assets', 'assets/000.bin']
        self.rejected('Overlapping')
        self.plan = original
        (self.bundle / 'unowned').write_bytes(b'SYNTHETIC unowned')
        self.rejected('without explicit')

    def test_versions_roles_and_advertisements_are_never_inferred(self):
        original = copy.deepcopy(self.plan)
        for key, value in [('schema_version', True), ('target', 'unsupported'), ('ocr_languages', []), ('regions', ['bad/region'])]:
            self.plan = copy.deepcopy(original)
            self.plan[key] = value
            self.rejected()
        for key, value in [('version', 'latest'), ('id', 'undeclared-component'), ('paths', []), ('paths', ['assets/000.bin', 'ASSETS/000.BIN'])]:
            self.plan = copy.deepcopy(original)
            self.plan['components'][0][key] = value
            self.rejected()
        self.plan = copy.deepcopy(original)
        next(row for row in self.plan['components'] if row['id'] == 'java-runtime')['version'] = '17.0.0'
        report = self.rejected('contract verification')
        self.assertIn('java-runtime', report['verification']['invalid_components'])
        self.plan = original
        next(row for row in self.plan['components'] if row['id'] == 'duckdb-spatial')['version'] = '9.0.0'
        self.rejected('contract verification')

    def test_no_clobber_and_destination_must_stay_outside_bundle(self):
        self.inventory.write_bytes(b'prior reviewed bytes')
        report = self.generate()
        self.assertFalse(report['generated'])
        self.assertEqual(self.inventory.read_bytes(), b'prior reviewed bytes')
        self.inventory = self.bundle / 'inventory.json'
        self.rejected('outside')
        self.inventory = self.directory / 'new.json'
        self.plan_path = self.bundle / 'ownership.json'
        self.rejected('outside')

    def test_symlinked_parent_cannot_disguise_output_or_plan_inside_bundle(self):
        alias = self.directory / 'bundle-alias'
        try:
            alias.symlink_to(self.bundle, target_is_directory=True)
        except OSError as exc:
            self.skipTest(f'Symlink creation unavailable: {exc.errno}')
        self.inventory = alias / 'inventory.json'
        self.rejected('outside')
        self.inventory = self.directory / 'outside-inventory.json'
        self.plan_path = alias / 'ownership.json'
        self.rejected('outside')

    @unittest.skipUnless(os.name == 'nt', 'Windows junction-specific check')
    def test_junction_parent_cannot_disguise_output_or_plan_inside_bundle(self):
        alias = self.directory / 'bundle-junction'
        subprocess.run(['cmd', '/c', 'mklink', '/J', str(alias), str(self.bundle)], check=True, capture_output=True)
        self.inventory = alias / 'inventory.json'
        self.rejected('outside')
        self.inventory = self.directory / 'outside-inventory.json'
        self.plan_path = alias / 'ownership.json'
        self.rejected('outside')

    def test_hardlinks_symlinks_and_reparse_metadata_are_rejected(self):
        target = self.bundle / self.plan['components'][0]['paths'][0]
        link = self.directory / 'hardlink'
        os.link(target, link)
        self.rejected('non-hardlinked')
        link.unlink()
        real_stat = verifier.os.stat
        class Reparse:
            st_file_attributes = 0x400
            def __init__(self, info):
                self.info = info
            def __getattr__(self, name):
                return getattr(self.info, name)
        def reparse(path, *args, **kwargs):
            result = real_stat(path, *args, **kwargs)
            return Reparse(result) if Path(path) == target else result
        with patch.object(verifier.os, 'stat', side_effect=reparse):
            self.rejected('reparse')
        try:
            target.unlink()
            target.symlink_to(self.directory / 'outside')
        except OSError as exc:
            self.skipTest(f'Symlink creation unavailable: {exc.errno}')
        self.rejected('reparse')

    def test_byte_and_expansion_limits_fail_before_output(self):
        policy = copy.deepcopy(POLICY)
        policy['limits']['total_bytes'] = 1
        local_policy = self.directory / 'policy.json'
        local_policy.write_text(json.dumps(policy))
        with patch.object(verifier, 'POLICY_PATH', local_policy):
            self.rejected('byte limit')
        self.plan['components'] = [{'id': name, 'version': '1.0.0', 'paths': ['assets']}
                                   for name in ['application', 'ui-assets', 'document-parser']]
        actual = verifier.scan_bundle(self.bundle, POLICY['limits'], lambda *a, **k: self.fail('unexpected scan error'))
        limits = dict(POLICY['limits'], files=len(actual))
        with self.assertRaises(verifier.InvalidInventory):
            producer.ownership(self.plan, actual, limits)

    def test_encoded_inventory_limit_rejects_and_removes_pending_file(self):
        policy = copy.deepcopy(POLICY)
        policy['limits']['manifest_bytes'] = 128
        local_policy = self.directory / 'small-policy.json'
        local_policy.write_text(json.dumps(policy))
        with patch.object(verifier, 'POLICY_PATH', local_policy):
            self.rejected('JSON limit')

    def test_mutation_between_hash_and_verification_does_not_publish(self):
        target = self.bundle / self.plan['components'][0]['paths'][0]
        original = verifier.verify
        def changed(*args):
            target.write_bytes(b'X' * target.stat().st_size)
            return original(*args)
        with patch.object(verifier, 'verify', side_effect=changed):
            report = self.rejected('contract verification')
        self.assertIn('hash_mismatch', [row['code'] for row in report['verification']['errors']])

    def test_changed_plan_or_publication_race_refuses_success(self):
        original = verifier.verify
        def changed(*args):
            result = original(*args)
            self.plan_path.write_text('{}')
            return result
        with patch.object(verifier, 'verify', side_effect=changed):
            self.rejected('changed during generation')
        real_link = os.link
        def concurrent(source, destination):
            Path(destination).write_bytes(b'concurrent existing inventory')
            return real_link(source, destination)
        with patch.object(producer.os, 'link', side_effect=concurrent):
            report = self.generate()
        self.assertFalse(report['generated'])
        self.assertEqual(self.inventory.read_bytes(), b'concurrent existing inventory')
        self.assertFalse(list(self.directory.glob('.ew-inventory-*')))

    def test_postpublication_cleanup_failure_cannot_claim_success(self):
        real_unlink = Path.unlink
        def failed(path, *args, **kwargs):
            if path.name.startswith('.ew-inventory-'):
                raise PermissionError('synthetic cleanup failure')
            return real_unlink(path, *args, **kwargs)
        with patch.object(Path, 'unlink', autospec=True, side_effect=failed):
            report = self.generate()
        self.assertTrue(report['published'])
        self.assertFalse(report['generated'])
        self.assertFalse(report['complete'])
        self.assertFalse(report['complete_release'])
        self.assertEqual(report['failure'], 'Inventory temporary-file cleanup failed')
        self.assertTrue(self.inventory.exists())
        self.assertEqual(len(list(self.directory.glob('.ew-inventory-*'))), 1)

    def test_malformed_plan_and_errors_do_not_export_private_paths(self):
        for raw in ['{', '{"schema_version":1,"schema_version":1}', ' ' * (verifier.MAX_JSON_BYTES + 1)]:
            self.plan_path.write_text(raw)
            report = producer.generate(self.bundle, self.plan_path, self.inventory, 'macos-aarch64')
            self.assertFalse(report['generated'])
            self.assertFalse(self.inventory.exists())
            self.assertNotIn(str(self.directory), json.dumps(report))
        report = producer.generate(self.bundle, self.directory / 'missing', self.inventory, 'macos-aarch64')
        self.assertFalse(report['generated'])
        self.assertNotIn(str(self.directory), json.dumps(report))

    def test_cli_remains_incomplete_under_optimization_and_does_not_overwrite(self):
        self.plan['components'] = [{'id': 'java-runtime', 'version': '21.0.12.1', 'paths': ['assets']}]
        self.plan_path.write_text(json.dumps(self.plan))
        command = [sys.executable, '-O', str(SCRIPTS / 'generate_runtime_inventory.py'), '--bundle', str(self.bundle),
                   '--plan', str(self.plan_path), '--inventory', str(self.inventory), '--target', 'macos-aarch64']
        result = subprocess.run(command, capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertTrue(json.loads(result.stdout)['generated'])
        self.assertFalse(json.loads(result.stdout)['complete'])
        existing = self.inventory.read_bytes()
        repeated = subprocess.run(command, capture_output=True, text=True, check=False)
        self.assertEqual(repeated.returncode, 1)
        self.assertFalse(json.loads(repeated.stdout)['generated'])
        self.assertEqual(self.inventory.read_bytes(), existing)


if __name__ == '__main__':
    unittest.main()
