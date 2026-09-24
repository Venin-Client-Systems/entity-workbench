"""Exercise the actual scanner against disposable local Git indexes."""
from pathlib import Path
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

SOURCE = Path(__file__).with_name('audit_public.py')
RESTRICTED = 'SYNTHETIC_RESTRICTED_AUDIT_FIXTURE'


class PublicAuditTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        root = Path(self.temporary.name)
        self.repo = root / 'repo'
        self.repo.mkdir()
        (self.repo / 'scripts').mkdir()
        shutil.copyfile(SOURCE, self.repo / 'scripts' / 'audit_public.py')
        self.exclusions = root / 'synthetic-exclusions.txt'
        self.exclusions.write_text(RESTRICTED + '\n', encoding='utf-8')
        self.hooks = root / 'empty-hooks'
        self.hooks.mkdir()
        self.git('init', '--quiet')
        self.write('record.txt', 'Synthetic public record\n')
        self.write('removed.txt', RESTRICTED + '\n')
        self.git('add', '--all')
        self.git(
            '-c', 'user.name=Synthetic Audit Test',
            '-c', 'user.email=synthetic@example.invalid',
            '-c', 'commit.gpgsign=false',
            '-c', 'core.hooksPath=' + str(self.hooks),
            'commit', '--quiet', '-m', 'Synthetic baseline',
        )

    def git(self, *arguments):
        return subprocess.check_output(
            ['git', *arguments], cwd=self.repo, text=True,
            stderr=subprocess.STDOUT,
        )

    def write(self, name, content):
        (self.repo / name).write_text(content, encoding='utf-8')

    def audit(self):
        return subprocess.run(
            [sys.executable, str(self.repo / 'scripts' / 'audit_public.py'),
             str(self.exclusions)],
            cwd=self.repo, text=True, capture_output=True, check=False,
        )

    def assert_private_failure(self, result, name):
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(name + ': private exclusion matched', result.stdout)
        self.assertNotIn('Traceback', result.stderr)
        self.assertNotIn('Public audit passed', result.stdout)

    def test_pure_deletion_has_no_surviving_blob_and_passes(self):
        self.git('rm', '--quiet', 'removed.txt')
        result = self.audit()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, 'Public audit passed for 0 staged files\n')

    def test_deletion_does_not_hide_a_restricted_staged_addition(self):
        self.git('rm', '--quiet', 'removed.txt')
        self.write('added.txt', RESTRICTED + '\n')
        self.git('add', 'added.txt')
        self.assert_private_failure(self.audit(), 'added.txt')

    def test_public_working_copy_cannot_hide_restricted_staged_modification(self):
        self.write('record.txt', RESTRICTED + '\n')
        self.git('add', 'record.txt')
        self.write('record.txt', 'Synthetic public record\n')
        self.assertNotIn(RESTRICTED, (self.repo / 'record.txt').read_text())
        self.assertIn(RESTRICTED, self.git('show', ':record.txt'))
        self.assert_private_failure(self.audit(), 'record.txt')

    def test_unstaged_restricted_text_does_not_replace_public_staged_bytes(self):
        self.write('record.txt', 'Reviewed synthetic public revision\n')
        self.git('add', 'record.txt')
        self.write('record.txt', RESTRICTED + '\n')
        result = self.audit()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, 'Public audit passed for 1 staged files\n')

    def test_rename_scans_surviving_destination_content(self):
        self.git('mv', 'removed.txt', 'renamed.txt')
        status = self.git('diff', '--cached', '--name-status', '--find-renames')
        self.assertIn('R100\tremoved.txt\trenamed.txt', status)
        self.assert_private_failure(self.audit(), 'renamed.txt')

    @unittest.skipIf(os.name == 'nt', 'Symlink creation may require Windows privileges')
    def test_type_change_scans_the_staged_link_blob(self):
        path = self.repo / 'record.txt'
        path.unlink()
        path.symlink_to(RESTRICTED)
        self.git('add', 'record.txt')
        self.assertIn('T\trecord.txt', self.git('diff', '--cached', '--name-status'))
        self.assert_private_failure(self.audit(), 'record.txt')


if __name__ == '__main__':
    unittest.main()
