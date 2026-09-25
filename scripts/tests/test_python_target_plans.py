"""Offline target-selection checks; these never import a candidate package."""
from pathlib import Path
import copy
import json
import sys
import tomllib
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import verify_python_target_plans as plans


class TargetPlanTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.packages = plans.package_map(tomllib.loads((plans.ROOT / 'workers/python/uv.lock').read_text()))
        cls.plans = {target: json.loads((plans.ROOT / f'packaging/plans/python-{target}-wheels.v1.json').read_text())
                     for target in plans.TARGETS}

    def test_complete_target_sets_match_reviewed_closure_and_keep_windows_markers(self):
        intel = plans.dependency_names(self.packages, 'macos-x86_64')
        windows = plans.dependency_names(self.packages, 'windows-x86_64')
        self.assertEqual(windows - intel, {'colorama', 'tzdata'})
        self.assertFalse(intel - windows)
        self.assertNotIn('pytest', windows)
        for target, count in [('macos-x86_64', 58), ('windows-x86_64', 60)]:
            self.assertEqual(plans.verify(target)['packages'], count)
        arm64 = json.loads((plans.ROOT / 'packaging/plans/python-macos-arm64-wheels.v1.json').read_text())
        self.assertEqual(intel, {p['name'] for p in arm64})

    def test_missing_windows_only_dependency_and_wrong_target_wheel_fail(self):
        for name in ('colorama', 'tzdata'):
            changed = [p for p in self.plans['windows-x86_64'] if p['name'] != name]
            with self.assertRaises(plans.PlanError):
                plans.validate_plan(changed, self.packages, 'windows-x86_64')
        changed = copy.deepcopy(self.plans['macos-x86_64'])
        index = next(i for i, p in enumerate(changed) if p['name'] == 'blis')
        changed[index] = next(p for p in self.plans['windows-x86_64'] if p['name'] == 'blis')
        with self.assertRaises(plans.PlanError):
            plans.validate_plan(changed, self.packages, 'macos-x86_64')

    def test_duplicate_unknown_extra_or_unlocked_artifact_is_rejected(self):
        for mutate in [lambda p: p.__setitem__(1, copy.deepcopy(p[0])),
                       lambda p: p[0].update(bytes=True), lambda p: p[0].update(bytes=p[0]['bytes'] + 1),
                       lambda p: p[0].update(sha256='0' * 64), lambda p: p[0].update(extra='x'),
                       lambda p: p[0].update(version='0'), lambda p: p[0].update(name='pytest'),
                       lambda p: p[0].update(url=p[0]['url'] + '?unreviewed=1'),
                       lambda p: p[0].update(url=p[0]['url'].replace('files.pythonhosted.org', 'example.org')),
                       lambda p: p[0].update(filename='../' + p[0]['filename'])]:
            changed = copy.deepcopy(self.plans['windows-x86_64'])
            mutate(changed)
            with self.assertRaises(plans.PlanError):
                plans.validate_plan(changed, self.packages, 'windows-x86_64')

    def test_new_dependency_marker_or_missing_locked_package_needs_review(self):
        changed = copy.deepcopy(self.packages)
        changed['click']['dependencies'][0]['marker'] = "python_version >= '3.13'"
        with self.assertRaises(plans.PlanError):
            plans.dependency_names(changed, 'windows-x86_64')
        changed = copy.deepcopy(self.packages)
        del changed['colorama']
        with self.assertRaises(plans.PlanError):
            plans.dependency_names(changed, 'windows-x86_64')
        with self.assertRaises(plans.PlanError):
            plans.package_map({'package': [self.packages[plans.ROOT_PACKAGE]] * 2})

    def test_locked_artifact_still_requires_every_tag_to_be_reviewed_and_python3_capable(self):
        for tag in ('py2-none-any', 'py3.py4-none-any', 'cp313-cp313-macosx_11_0_arm64'):
            changed = copy.deepcopy(self.plans['windows-x86_64'])
            item = changed[0]
            item['filename'] = f"altair-{item['version']}-{tag}.whl"
            item['url'] = item['url'].rsplit('/', 1)[0] + '/' + item['filename']
            packages = copy.deepcopy(self.packages)
            packages['altair']['wheels'] = [{'url': item['url'], 'size': item['bytes'],
                                             'hash': 'sha256:' + item['sha256']}]
            with self.assertRaises(plans.PlanError):
                plans.validate_plan(changed, packages, 'windows-x86_64')


if __name__ == '__main__':
    unittest.main()
