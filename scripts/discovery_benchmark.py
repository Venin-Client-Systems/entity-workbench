#!/usr/bin/env python3
"""Validate and score discovery evidence offline; never collect or approve a release."""
from __future__ import annotations

import argparse
from collections import Counter
from datetime import datetime, timedelta, timezone
import hashlib
import json
from pathlib import Path
import re
import stat
import sys
import unicodedata
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BENCHMARK = ROOT / 'docs/discovery/public-benchmark.v1.json'
MAX_JSON_BYTES = 8 * 1024 * 1024
MAX_ARTIFACT_BYTES = 64 * 1024 * 1024
MAX_TOTAL_BYTES = 2 * 1024 * 1024 * 1024
HASH = re.compile(r'[0-9a-f]{64}\Z')
REVISION = re.compile(r'[0-9a-f]{40}\Z')
ID = re.compile(r'[a-z0-9][a-z0-9-]{0,79}\Z')
DOMAIN = re.compile(r'(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}\Z')
RESERVED = re.compile(r'(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?\Z', re.IGNORECASE)
STATUSES = {'successful', 'no_results', 'blocked', 'quota_exhausted', 'failed', 'not_run'}
LIMITS = {'max_hops': 2, 'max_requests': 50, 'max_seconds': 600}
THRESHOLDS = {'relevant_percent': 60, 'expansion_percent': 30}


class InvalidBenchmark(ValueError):
    """A bounded, safe-to-display validation error."""


def require(condition, message):
    if not condition:
        raise InvalidBenchmark(message)


def exact(value, names, context):
    require(isinstance(value, dict) and set(value) == set(names.split()),
            context + ': missing or unexpected fields')


def text(value, context, maximum=2000):
    require(isinstance(value, str) and 0 < len(value) <= maximum and bool(value.strip())
            and not any(ord(c) < 32 for c in value), context + ': invalid text')
    return value


def identifier(value):
    require(isinstance(value, str) and ID.fullmatch(value), 'Invalid identifier')
    return value


def integer(value, minimum, maximum, context):
    require(type(value) is int and minimum <= value <= maximum, context + ': invalid integer')


def timestamp(value):
    require(isinstance(value, str) and re.fullmatch(r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z', value),
            'Expected second-resolution UTC timestamp')
    try:
        return datetime.strptime(value, '%Y-%m-%dT%H:%M:%SZ').replace(tzinfo=timezone.utc)
    except ValueError as exc:
        raise InvalidBenchmark('Invalid timestamp') from exc


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'Duplicate JSON member')
        result[key] = value
    return result


def reject_constant(_value):
    raise InvalidBenchmark('Non-finite JSON number')


def read_json(path):
    with Path(path).open('rb') as stream:
        raw = stream.read(MAX_JSON_BYTES + 1)
    require(len(raw) <= MAX_JSON_BYTES, 'JSON exceeds byte limit')
    try:
        value = json.loads(raw, object_pairs_hook=unique_object, parse_constant=reject_constant)
    except (ValueError, UnicodeError, RecursionError) as exc:
        if isinstance(exc, InvalidBenchmark):
            raise
        raise InvalidBenchmark('Invalid JSON') from exc
    return value, hashlib.sha256(raw).hexdigest()


def url(value, domain, blocked=False):
    text(value, 'URL', 4096)
    parsed = urlsplit(value)
    # The benchmark keeps search text local. Collection URLs have no query,
    # fragment, credentials or alternate ports; broker policy remains separate.
    require(value == value.strip() and parsed.scheme == 'https' and parsed.hostname is not None
            and parsed.netloc == parsed.hostname and not parsed.query and not parsed.fragment
            and '\\' not in value and '%' not in parsed.netloc,
            'Expected plain HTTPS collection URL')
    require(blocked or parsed.hostname == domain or parsed.hostname.endswith('.' + domain),
            'URL is outside the analyst-selected publisher domain')
    return value


def validate_benchmark(data):
    exact(data, 'schema_version benchmark_id material frozen_at collection_window_days limits thresholds publishers tasks', 'benchmark')
    integer(data['schema_version'], 1, 1, 'schema_version')
    identifier(data['benchmark_id'])
    require(data['material'] in ('public_reference', 'synthetic'), 'Invalid benchmark material')
    timestamp(data['frozen_at'])
    integer(data['collection_window_days'], 14, 14, 'collection_window_days')
    require(data['limits'] == LIMITS and all(type(v) is int for v in data['limits'].values()),
            'The frozen collection limits cannot change within version 1')
    require(data['thresholds'] == THRESHOLDS and all(type(v) is int for v in data['thresholds'].values()),
            'The frozen proposed thresholds cannot change within version 1')
    require(isinstance(data['publishers'], list) and 6 <= len(data['publishers']) <= 30,
            'Expected at least six independent publishers')
    publishers = {}
    groups, domains = set(), set()
    for publisher in data['publishers']:
        exact(publisher, 'id name domain independence_group seed_url access_review', 'publisher')
        name = identifier(publisher['id'])
        require(name not in publishers, 'Duplicate publisher')
        text(publisher['name'], 'Publisher name')
        domain = publisher['domain']
        require(isinstance(domain, str) and DOMAIN.fullmatch(domain), 'Invalid publisher domain')
        require(domain not in domains, 'Duplicate publisher domain')
        require(not any(domain.endswith('.' + d) or d.endswith('.' + domain) for d in domains),
                'Publisher domains must not overlap')
        require(domain.endswith('.example') == (data['material'] == 'synthetic'),
                'Synthetic and public-reference domains must stay separate')
        domains.add(domain)
        groups.add(identifier(publisher['independence_group']))
        url(publisher['seed_url'], domain)
        require(publisher['access_review'] == 'required_before_each_run',
                'Access permission is a run-time review, not a seed assumption')
        publishers[name] = publisher
    require(len(groups) >= 6, 'Expected six independent publisher groups')
    require(isinstance(data['tasks'], list) and len(data['tasks']) == 30, 'The denominator must be exactly 30 tasks')
    tasks = {}
    for task in data['tasks']:
        exact(task, 'id publisher_id input_kind query question relevance_criterion expansion_criterion', 'task')
        name = identifier(task['id'])
        require(name not in tasks, 'Duplicate task')
        require(task['publisher_id'] in publishers, 'Unknown publisher')
        require(task['input_kind'] in ('name', 'organisation', 'domain'), 'Invalid input kind')
        for key in ('query', 'question', 'relevance_criterion', 'expansion_criterion'):
            text(task[key], key)
        tasks[name] = task
    require(set(publishers) == {t['publisher_id'] for t in tasks.values()}, 'Unused publisher')
    require({t['input_kind'] for t in tasks.values()} == {'name', 'organisation', 'domain'},
            'All three input kinds must be represented')
    return tasks, publishers


def verify_artifacts(entries, root):
    require(isinstance(entries, list) and len(entries) <= 1800, 'Artifact list exceeds limit')
    root = Path(root)
    root_info = root.lstat()
    require(stat.S_ISDIR(root_info.st_mode) and not unsafe_link(root_info),
            'Evidence root must be a real directory without reparse points')
    artifacts, paths, total = {}, set(), 0
    for entry in entries:
        exact(entry, 'id kind path sha256', 'artifact')
        name = identifier(entry['id'])
        require(name not in artifacts, 'Duplicate artifact identifier')
        require(entry['kind'] in ('original', 'access_review', 'local_search', 'ui_capture', 'corpus_reset'),
                'Unknown artifact kind')
        path = text(entry['path'], 'Artifact path', 1024)
        parts = path.split('/')
        require(all(p not in ('', '.', '..') and not p.endswith((' ', '.')) and not RESERVED.fullmatch(p) for p in parts)
                and not any(c in path for c in '\\:*?"<>|') and len(parts) <= 12,
                'Unsafe artifact path')
        path_key = unicodedata.normalize('NFC', path).casefold()
        require(path_key not in paths, 'Duplicate artifact path')
        paths.add(path_key)
        target = root
        for part in parts:
            target /= part
            info = target.lstat()
            require(not unsafe_link(info), 'Artifact links are forbidden')
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_size <= MAX_ARTIFACT_BYTES,
                'Expected bounded regular artifact file')
        total += info.st_size
        require(total <= MAX_TOTAL_BYTES, 'Evidence exceeds total byte limit')
        require(isinstance(entry['sha256'], str) and HASH.fullmatch(entry['sha256']), 'Invalid artifact SHA-256')
        digest = hashlib.sha256()
        count = 0
        with target.open('rb') as stream:
            while block := stream.read(1024 * 1024):
                count += len(block)
                require(count <= info.st_size, 'Artifact changed while hashing')
                digest.update(block)
        after = target.lstat()
        require((info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns) ==
                (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
                and count == info.st_size, 'Artifact changed while hashing')
        require(digest.hexdigest() == entry['sha256'], 'Artifact hash mismatch')
        artifacts[name] = entry
    return artifacts


def unsafe_link(info):
    return stat.S_ISLNK(info.st_mode) or bool(
        getattr(info, 'st_file_attributes', 0) & getattr(stat, 'FILE_ATTRIBUTE_REPARSE_POINT', 0x400)
    )


def artifact_ref(value, kind, artifacts):
    require(isinstance(value, str) and value in artifacts and artifacts[value]['kind'] == kind,
            'Missing artifact or incorrect artifact kind')


def score(benchmark_path, run_path=None, evidence_root=None):
    checked_at = datetime.now(timezone.utc)
    benchmark, digest = read_json(benchmark_path)
    tasks, publishers = validate_benchmark(benchmark)
    report = {'schema_version': 1, 'benchmark_id': benchmark['benchmark_id'], 'benchmark_sha256': digest,
              'valid': True, 'measurement_status': 'not_run', 'mode': None, 'denominator': 30,
              'measured_tasks': 0, 'labelled_tasks': 0, 'relevant_tasks': 0, 'expansion_tasks': 0,
              'relevant_percent': None, 'expansion_percent': None, 'thresholds_met': False,
              'live_measurement_eligible': False, 'release_gate_decision': 'not_evaluated',
              'thresholds': THRESHOLDS.copy(), 'status_counts': {'not_run': 30},
              'not_run_tasks': sorted(tasks), 'unlabelled_tasks': [],
              'total_requests': 0, 'total_task_seconds': 0, 'max_task_requests': 0, 'max_task_seconds': 0}
    if run_path is None:
        return report
    require(evidence_root is not None, 'An evidence root is required for scoring')
    run, run_digest = read_json(run_path)
    exact(run, 'schema_version benchmark_sha256 run_id mode started_at ended_at app_revision app_version platform runner_id corpus_reset_artifact artifacts results', 'run')
    integer(run['schema_version'], 1, 1, 'schema_version')
    require(run['benchmark_sha256'] == digest, 'Run does not reference the exact frozen benchmark bytes')
    require(run['mode'] in ('live', 'synthetic') and
            (run['mode'] == 'synthetic') == (benchmark['material'] == 'synthetic'),
            'Live measurements and synthetic replays must stay separate')
    identifier(run['run_id'])
    runner = identifier(run['runner_id'])
    require(isinstance(run['app_revision'], str) and REVISION.fullmatch(run['app_revision']), 'Invalid application revision')
    text(run['app_version'], 'Application version', 128)
    require(run['platform'] in ('windows-x86_64', 'macos-aarch64', 'macos-x86_64'), 'Unsupported application platform')
    start, end = timestamp(run['started_at']), timestamp(run['ended_at'])
    require(timestamp(benchmark['frozen_at']) <= start <= end <= start + timedelta(days=14),
            'Run must follow the freeze and finish within the collection window')
    require(end <= checked_at, 'Run cannot claim future collection')
    artifacts = verify_artifacts(run['artifacts'], evidence_root)
    artifact_ref(run['corpus_reset_artifact'], 'corpus_reset', artifacts)
    require(isinstance(run['results'], list) and len(run['results']) <= 30, 'Result list exceeds denominator')
    seen, measured = set(), set()
    status_counts = Counter()
    for result in run['results']:
        require(isinstance(result, dict), 'Invalid result')
        task_id = result.get('task_id')
        require(isinstance(task_id, str) and task_id in tasks and task_id not in seen, 'Unknown or duplicate task result')
        seen.add(task_id)
        status = result.get('status')
        require(isinstance(status, str) and status in STATUSES, 'Invalid result status')
        if status == 'not_run':
            exact(result, 'task_id status', 'not-run result')
            continue
        exact(result, 'task_id status started_at ended_at stop_reason effective_limits corpus_reset_artifact access_review_artifact local_search_artifact ui_capture_artifact requests labels', 'result')
        measured.add(task_id)
        status_counts[status] += 1
        text(result['stop_reason'], 'Stop reason')
        task_start, task_end = timestamp(result['started_at']), timestamp(result['ended_at'])
        require(start <= task_start <= task_end <= end, 'Task dates fall outside run')
        effective = result['effective_limits']
        exact(effective, 'max_hops max_requests max_seconds', 'effective limits')
        for key, ceiling in LIMITS.items():
            integer(effective[key], 1 if key == 'max_seconds' else 0, ceiling, key)
        seconds = int((task_end - task_start).total_seconds())
        require(seconds <= effective['max_seconds'], 'Task exceeded effective time bound')
        for key, kind in (('corpus_reset_artifact', 'corpus_reset'), ('access_review_artifact', 'access_review'), ('local_search_artifact', 'local_search'), ('ui_capture_artifact', 'ui_capture')):
            artifact_ref(result[key], kind, artifacts)
        requests = result['requests']
        require(isinstance(requests, list) and len(requests) <= effective['max_requests'], 'Task exceeded effective request bound')
        publisher = publishers[tasks[task_id]['publisher_id']]
        domain = publisher['domain']
        previous_end = task_start
        for request_index, request in enumerate(requests):
            exact(request, 'url method purpose parent_request hop started_at ended_at outcome http_status original_artifact', 'request')
            url(request['url'], domain, blocked=request['outcome'] == 'blocked')
            require(request['method'] in ('GET', 'HEAD'), 'Unsupported request method')
            integer(request['hop'], 0, effective['max_hops'], 'request hop')
            purpose, parent = request['purpose'], request['parent_request']
            require(purpose in ('access_review', 'seed', 'link', 'redirect'), 'Invalid request purpose')
            if purpose in ('access_review', 'seed'):
                require(parent is None and request['hop'] == 0, 'Initial request cannot claim a parent or expansion hop')
                if purpose == 'seed':
                    require(request['url'] == publisher['seed_url'], 'Content collection must start at the frozen seed')
            else:
                integer(parent, 0, request_index - 1, 'parent request')
                previous = requests[parent]
                require(previous['purpose'] != 'access_review' and previous['outcome'] == 'fetched',
                        'Content discovery cannot originate from an access-review request')
                increment = 1 if purpose == 'link' else 0
                require(request['hop'] == previous['hop'] + increment, 'Hop does not match the parent chain')
                require((purpose == 'link' and previous['method'] == 'GET' and 200 <= previous['http_status'] < 300)
                        or (purpose == 'redirect' and 300 <= previous['http_status'] < 400),
                        'Parent response cannot justify the next request')
            request_start, request_end = timestamp(request['started_at']), timestamp(request['ended_at'])
            require(previous_end <= request_start <= request_end <= task_end, 'Invalid serial request chronology')
            previous_end = request_end
            require(request['outcome'] in ('fetched', 'blocked', 'failed'), 'Invalid request outcome')
            if request['http_status'] is not None:
                integer(request['http_status'], 100, 599, 'HTTP status')
            if request['outcome'] == 'fetched':
                require(request['http_status'] is not None, 'Fetched request lacks HTTP status')
                artifact_ref(request['original_artifact'], 'original', artifacts)
            else:
                require(request['original_artifact'] is None, 'Unfetched request cannot claim an original')
        report['total_requests'] += len(requests)
        report['total_task_seconds'] += seconds
        report['max_task_requests'] = max(report['max_task_requests'], len(requests))
        report['max_task_seconds'] = max(report['max_task_seconds'], seconds)
        labels = result['labels']
        if labels is None:
            report['unlabelled_tasks'].append(task_id)
            continue
        exact(labels, 'reviewer_id reviewed_at relevant_result useful_expansion rationale source_requests chain', 'labels')
        require(identifier(labels['reviewer_id']) != runner, 'Labels require a reviewer independent of the runner')
        require(task_end <= timestamp(labels['reviewed_at']) <= checked_at,
                'Labels must follow collection and cannot claim future review')
        require(type(labels['relevant_result']) is bool and type(labels['useful_expansion']) is bool, 'Labels must be Boolean')
        text(labels['rationale'], 'Review rationale')
        sources = labels['source_requests']
        require(isinstance(sources, list) and len(sources) <= 50, 'Invalid source references')
        for index in sources:
            integer(index, 0, len(requests) - 1, 'source request index')
            require(requests[index]['purpose'] != 'access_review'
                    and requests[index]['outcome'] == 'fetched' and requests[index]['method'] == 'GET'
                    and 200 <= requests[index]['http_status'] < 300, 'Relevant source must be a successful GET')
        require(len(set(sources)) == len(sources), 'Duplicate source request index')
        relevant = labels['relevant_result']
        expansion = labels['useful_expansion']
        require(not relevant or (status == 'successful' and bool(sources)), 'Relevant label needs a successful, source-backed result')
        require(relevant or not sources, 'Irrelevant labels cannot nominate supporting sources')
        require(not expansion or relevant, 'Useful expansion requires a relevant result')
        chain = labels['chain']
        if expansion:
            exact(chain, 'identifier identifier_anchor source_request lead_request lead_anchor', 'expansion chain')
            for key in ('identifier', 'identifier_anchor', 'lead_anchor'):
                text(chain[key], key)
            first, lead = chain['source_request'], chain['lead_request']
            require(type(first) is int and type(lead) is int and first in sources and lead in sources and first < lead,
                    'Expansion must reference distinct ordered source requests')
            require(requests[first]['url'] != requests[lead]['url'] and
                    requests[first]['hop'] < requests[lead]['hop'], 'Expansion needs a new URL at a subsequent hop')
            ancestor = requests[lead]['parent_request']
            while ancestor is not None and ancestor != first:
                ancestor = requests[ancestor]['parent_request']
            require(ancestor == first, 'Further lead must descend from the identifier source')
        else:
            require(chain is None, 'Non-expansion label cannot claim a chain')
        report['labelled_tasks'] += 1
        report['relevant_tasks'] += int(relevant)
        report['expansion_tasks'] += int(expansion)
    report['mode'] = run['mode']
    report['run_sha256'] = run_digest
    report['measured_tasks'] = len(measured)
    report['not_run_tasks'] = sorted(set(tasks) - measured)
    report['unlabelled_tasks'].sort()
    status_counts['not_run'] = 30 - len(measured)
    report['status_counts'] = dict(sorted(status_counts.items()))
    if measured:
        report['measurement_status'] = 'incomplete'
    if len(measured) == report['labelled_tasks'] == 30:
        report['measurement_status'] = 'complete'
        report['relevant_percent'] = report['relevant_tasks'] * 100 / 30
        report['expansion_percent'] = report['expansion_tasks'] * 100 / 30
        report['thresholds_met'] = report['relevant_tasks'] >= 18 and report['expansion_tasks'] >= 9
        report['live_measurement_eligible'] = report['thresholds_met'] and run['mode'] == 'live'
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--benchmark', type=Path, default=DEFAULT_BENCHMARK)
    parser.add_argument('--run', type=Path)
    parser.add_argument('--evidence-root', type=Path)
    args = parser.parse_args()
    try:
        report = score(args.benchmark, args.run, args.evidence_root)
    except (InvalidBenchmark, OSError, ValueError, TypeError, KeyError, RecursionError) as exc:
        detail = str(exc) if isinstance(exc, InvalidBenchmark) else 'Unreadable or malformed benchmark evidence'
        report = {'schema_version': 1, 'valid': False, 'error': detail, 'release_gate_decision': 'not_evaluated'}
    print(json.dumps(report, indent=2, sort_keys=True))
    if not report['valid']:
        return 2
    return 0 if report['measurement_status'] == 'complete' and report['thresholds_met'] else 1


if __name__ == '__main__':
    sys.exit(main())
