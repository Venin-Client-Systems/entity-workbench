"""Scorer correctness against synthetic evidence, never live collection."""
import copy
from datetime import datetime, timedelta, timezone
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[2]


def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


scorer = module(ROOT / 'scripts/discovery_benchmark.py', 'discovery_benchmark')
fixtures = module(ROOT / 'fixtures/discovery/make_synthetic_replay.py', 'synthetic_discovery')


class DiscoveryBenchmarkTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name) / 'replay'
        self.run = fixtures.create_replay(self.directory)
        self.benchmark = self.directory / 'benchmark.json'
        self.run_file = self.directory / 'run.json'
        self.evidence = self.directory / 'evidence'

    def score(self):
        self.run_file.write_text(json.dumps(self.run))
        return scorer.score(self.benchmark, self.run_file, self.evidence)

    def rejects(self):
        with self.assertRaises(scorer.InvalidBenchmark):
            self.score()

    def test_public_benchmark_is_frozen_and_unmeasured(self):
        report = scorer.score(scorer.DEFAULT_BENCHMARK)
        self.assertEqual(report['measurement_status'], 'not_run')
        self.assertEqual(report['denominator'], 30)
        self.assertEqual(len(report['not_run_tasks']), 30)
        self.assertIsNone(report['relevant_percent'])
        self.assertFalse(report['thresholds_met'])

    def test_exact_thresholds_and_synthetic_separation(self):
        report = self.score()
        self.assertEqual(report['measurement_status'], 'complete')
        self.assertEqual(report['relevant_percent'], 60)
        self.assertEqual(report['expansion_percent'], 30)
        self.assertTrue(report['thresholds_met'])
        self.assertFalse(report['live_measurement_eligible'])
        self.assertEqual(report['release_gate_decision'], 'not_evaluated')
        self.assertEqual(report['total_requests'], 35)
        self.assertEqual(report['total_task_seconds'], 300)
        self.assertEqual(report['status_counts']['blocked'], 1)
        self.assertEqual(report['status_counts']['quota_exhausted'], 1)

    def test_one_fewer_relevant_result_fails_fixed_denominator(self):
        self.run['results'][17]['labels'].update(relevant_result=False, source_requests=[])
        report = self.score()
        self.assertFalse(report['thresholds_met'])
        self.assertEqual(report['denominator'], 30)
        self.assertAlmostEqual(report['relevant_percent'], 17 * 100 / 30)

    def test_one_fewer_expansion_fails(self):
        self.run['results'][8]['labels'].update(useful_expansion=False, chain=None)
        self.assertFalse(self.score()['thresholds_met'])

    def test_missing_measurement_is_not_zero_results(self):
        self.run['results'].pop()
        report = self.score()
        self.assertEqual(report['measurement_status'], 'incomplete')
        self.assertEqual(report['status_counts']['not_run'], 1)
        self.assertEqual(report['not_run_tasks'], ['discovery-30'])
        self.assertEqual(report['status_counts']['no_results'], 1)
        self.assertIsNone(report['relevant_percent'])
        self.assertFalse(report['thresholds_met'])

    def test_explicit_not_run_and_empty_run(self):
        self.run['results'] = [{'task_id': 'discovery-01', 'status': 'not_run'}]
        report = self.score()
        self.assertEqual(report['measurement_status'], 'not_run')
        self.assertEqual(report['measured_tasks'], 0)
        self.assertEqual(report['status_counts'], {'not_run': 30})

    def test_pending_review_retains_cost_and_missing_label(self):
        self.run['results'][0]['labels'] = None
        report = self.score()
        self.assertEqual(report['measurement_status'], 'incomplete')
        self.assertEqual(report['unlabelled_tasks'], ['discovery-01'])
        self.assertEqual(report['measured_tasks'], 30)
        self.assertEqual(report['labelled_tasks'], 29)
        self.assertEqual(report['total_requests'], 35)

    def test_runner_cannot_review_own_results(self):
        self.run['results'][0]['labels']['reviewer_id'] = self.run['runner_id']
        self.rejects()

    def test_hash_binds_exact_frozen_benchmark(self):
        self.benchmark.write_bytes(self.benchmark.read_bytes() + b' ')
        self.rejects()

    def test_version_one_cannot_reduce_threshold_or_expand_limit(self):
        benchmark = json.loads(self.benchmark.read_text())
        for field, key, value in [('thresholds', 'relevant_percent', 59), ('thresholds', 'expansion_percent', 29),
                                   ('limits', 'max_hops', 3), ('limits', 'max_requests', 51), ('limits', 'max_seconds', 601)]:
            with self.subTest(key=key):
                changed = copy.deepcopy(benchmark)
                changed[field][key] = value
                with self.assertRaises(scorer.InvalidBenchmark):
                    scorer.validate_benchmark(changed)

    def test_denominator_and_publisher_independence(self):
        benchmark = json.loads(self.benchmark.read_text())
        changed = copy.deepcopy(benchmark)
        changed['tasks'].pop()
        with self.assertRaises(scorer.InvalidBenchmark):
            scorer.validate_benchmark(changed)
        for p in benchmark['publishers']:
            p['independence_group'] = 'same-publisher'
        with self.assertRaises(scorer.InvalidBenchmark):
            scorer.validate_benchmark(benchmark)

    def test_synthetic_result_cannot_claim_live_mode(self):
        self.run['mode'] = 'live'
        self.rejects()

    def test_duplicate_and_unknown_task_results(self):
        for value in ('discovery-01', 'unknown-task'):
            with self.subTest(value=value):
                self.run['results'][1]['task_id'] = value
                self.rejects()

    def test_request_time_and_hop_limits(self):
        original = copy.deepcopy(self.run)
        self.run['results'][0]['requests'][0]['hop'] = 3
        self.rejects()
        self.run = copy.deepcopy(original)
        self.run['results'][0]['ended_at'] = '2026-09-24T00:10:01Z'
        self.rejects()
        self.run = copy.deepcopy(original)
        request = self.run['results'][0]['requests'][0]
        self.run['results'][0]['requests'] = [request] * 51
        self.rejects()

    def test_lower_access_limits_are_enforced(self):
        self.run['results'][0]['effective_limits']['max_requests'] = 1
        self.rejects()

    def test_higher_declared_limits_are_rejected(self):
        self.run['results'][0]['effective_limits']['max_requests'] = 51
        self.rejects()

    def test_all_redirects_must_be_within_selected_domain(self):
        self.run['results'][0]['requests'][0]['url'] = 'https://elsewhere.example/page'
        self.rejects()

    def test_blocked_external_redirect_is_retained_without_credit(self):
        result = self.run['results'][25]
        result['status'] = 'blocked'
        first = result['requests'][0]
        first['http_status'] = 302
        refused = copy.deepcopy(first)
        refused.update(url='https://elsewhere.example/', purpose='redirect', parent_request=0,
                       outcome='blocked', original_artifact=None, http_status=None,
                       started_at='2026-09-24T00:25:01Z', ended_at='2026-09-24T00:25:02Z')
        result['requests'].append(refused)
        report = self.score()
        self.assertEqual(report['status_counts']['blocked'], 2)
        self.assertEqual(report['relevant_tasks'], 18)

    def test_urls_cannot_disclose_search_query_or_credentials(self):
        for url in ('https://publisher-01.example/?q=private', 'http://publisher-01.example/',
                    'https://user@publisher-01.example/', 'https://publisher-01.example:8443/',
                    'https://publisher-01.example.attacker.example/'):
            with self.subTest(url=url):
                self.run['results'][0]['requests'][0]['url'] = url
                self.rejects()

    def test_expansion_needs_distinct_ordered_sources(self):
        self.run['results'][0]['labels']['chain']['lead_request'] = 0
        self.rejects()

    def test_expansion_needs_actual_subsequent_hop(self):
        self.run['results'][0]['requests'][1]['hop'] = 0
        self.rejects()

    def test_content_collection_starts_at_frozen_seed(self):
        self.run['results'][0]['requests'][0]['url'] += 'preselected-answer'
        self.rejects()

    def test_links_require_earlier_successful_parent(self):
        self.run['results'][0]['requests'][1]['parent_request'] = 1
        self.rejects()

    def test_access_review_cannot_be_relabeled_as_discovery(self):
        self.run['results'][0]['requests'][0]['purpose'] = 'access_review'
        self.rejects()

    def test_positive_label_needs_successful_body_and_source(self):
        self.run['results'][0]['requests'][0]['http_status'] = 404
        self.rejects()

    def test_blocked_does_not_count_as_relevant(self):
        self.run['results'][0]['status'] = 'blocked'
        self.rejects()

    def test_missing_or_tampered_artifacts_fail(self):
        artifact = self.evidence / self.run['artifacts'][0]['path']
        artifact.write_text('tampered')
        self.rejects()

    def test_unsafe_artifact_paths_fail(self):
        for path in ('../outside.txt', '/outside.txt', 'C:/outside.txt', 'a\\b', './file.txt'):
            with self.subTest(path=path):
                self.run['artifacts'][0]['path'] = path
                self.rejects()

    def test_symlink_artifacts_fail_where_supported(self):
        artifact = self.evidence / self.run['artifacts'][0]['path']
        outside = self.directory / 'outside.txt'
        outside.write_bytes(artifact.read_bytes())
        artifact.unlink()
        try:
            artifact.symlink_to(outside)
        except OSError:
            self.skipTest('Platform does not allow unprivileged symlinks')
        self.rejects()

    def test_evidence_root_reparse_metadata_is_rejected(self):
        real_lstat = Path.lstat

        def root_reparse(path, *args, **kwargs):
            info = real_lstat(path, *args, **kwargs)
            if path == self.evidence:
                return SimpleNamespace(st_mode=info.st_mode, st_file_attributes=0x400)
            return info

        with patch.object(Path, 'lstat', root_reparse):
            self.rejects()

    @unittest.skipUnless(sys.platform == 'win32', 'Windows junction coverage')
    def test_windows_junction_evidence_root_is_rejected(self):
        junction = self.directory / 'junction'
        made = subprocess.run(['cmd', '/c', 'mklink', '/J', str(junction), str(self.evidence)],
                              capture_output=True, text=True, check=False)
        self.assertEqual(made.returncode, 0, made.stdout + made.stderr)
        try:
            with self.assertRaises(scorer.InvalidBenchmark):
                scorer.score(self.benchmark, self.run_file, junction)
        finally:
            junction.rmdir()

    def test_duplicate_json_and_boolean_integer_are_rejected(self):
        self.run_file.write_text('{"a":1,"a":2}')
        with self.assertRaises(scorer.InvalidBenchmark):
            scorer.read_json(self.run_file)
        self.run['results'][0]['requests'][0]['hop'] = True
        self.rejects()

    def test_collection_must_follow_freeze_and_fit_fourteen_days(self):
        self.run['started_at'] = '2026-09-23T00:00:00Z'
        self.rejects()
        self.run['started_at'] = '2026-09-24T00:00:00Z'
        self.run['ended_at'] = '2026-10-10T00:00:00Z'
        self.rejects()

    def test_future_campaign_and_review_cannot_be_scored(self):
        original = copy.deepcopy(self.run)
        future_start = (datetime.now(timezone.utc) + timedelta(days=1)).replace(microsecond=0)
        shift = future_start - datetime(2026, 9, 24, tzinfo=timezone.utc)

        def move_future(record, keys):
            for key in keys:
                original_time = datetime.strptime(record[key], '%Y-%m-%dT%H:%M:%SZ').replace(tzinfo=timezone.utc)
                record[key] = (original_time + shift).strftime('%Y-%m-%dT%H:%M:%SZ')

        move_future(self.run, ('started_at', 'ended_at'))
        for result in self.run['results']:
            move_future(result, ('started_at', 'ended_at'))
            move_future(result['labels'], ('reviewed_at',))
            for request in result['requests']:
                move_future(request, ('started_at', 'ended_at'))
        with self.assertRaisesRegex(scorer.InvalidBenchmark, 'future collection'):
            self.score()
        self.run = original
        future = (datetime.now(timezone.utc) + timedelta(days=1)).strftime('%Y-%m-%dT%H:%M:%SZ')
        self.run['results'][0]['labels']['reviewed_at'] = future
        with self.assertRaisesRegex(scorer.InvalidBenchmark, 'future review'):
            self.score()

    def test_cli_exit_codes_and_optimized_validation(self):
        command = [sys.executable, '-O', str(ROOT / 'scripts/discovery_benchmark.py'),
                   '--benchmark', str(self.benchmark)]
        unmeasured = subprocess.run(command, capture_output=True, text=True, check=False)
        self.assertEqual(unmeasured.returncode, 1)
        self.assertEqual(json.loads(unmeasured.stdout)['measurement_status'], 'not_run')
        command.extend(['--run', str(self.run_file), '--evidence-root', str(self.evidence)])
        complete = subprocess.run(command, capture_output=True, text=True, check=False)
        self.assertEqual(complete.returncode, 0, complete.stdout + complete.stderr)
        future = (datetime.now(timezone.utc) + timedelta(days=1)).strftime('%Y-%m-%dT%H:%M:%SZ')
        self.run['results'][0]['labels']['reviewed_at'] = future
        self.run_file.write_text(json.dumps(self.run))
        future_review = subprocess.run(command, capture_output=True, text=True, check=False)
        self.assertEqual(future_review.returncode, 2)
        self.assertIn('future review', json.loads(future_review.stdout)['error'])
        self.run['results'][0]['labels']['reviewed_at'] = '2026-09-24T01:00:00Z'
        self.run['mode'] = 'live'
        self.run_file.write_text(json.dumps(self.run))
        invalid = subprocess.run(command, capture_output=True, text=True, check=False)
        self.assertEqual(invalid.returncode, 2)
        self.assertFalse(json.loads(invalid.stdout)['valid'])


if __name__ == '__main__':
    unittest.main()
