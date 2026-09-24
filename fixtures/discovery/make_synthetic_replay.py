#!/usr/bin/env python3
"""Write entirely fabricated scorer evidence; this never demonstrates discovery."""
from __future__ import annotations

import argparse
from datetime import datetime, timedelta, timezone
import hashlib
import json
from pathlib import Path

BENCHMARK = Path(__file__).with_name('synthetic-benchmark.v1.json')


def create_replay(destination):
    destination = Path(destination)
    destination.mkdir(parents=True, exist_ok=False)
    evidence = destination / 'evidence'
    evidence.mkdir()
    raw = BENCHMARK.read_bytes()
    benchmark = json.loads(raw)
    (destination / 'benchmark.json').write_bytes(raw)
    run = {'schema_version': 1, 'benchmark_sha256': hashlib.sha256(raw).hexdigest(),
           'run_id': 'synthetic-replay-v1', 'mode': 'synthetic',
           'started_at': '2026-09-25T00:00:00Z', 'ended_at': '2026-09-25T00:30:00Z',
           'app_revision': '0' * 40, 'app_version': '0.0.0-synthetic', 'platform': 'macos-aarch64',
           'runner_id': 'synthetic-runner', 'corpus_reset_artifact': 'synthetic-reset',
           'artifacts': [], 'results': []}

    def artifact(name, kind, content):
        raw_content = ('SYNTHETIC TEST FIXTURE. Not live evidence.\n' + content + '\n').encode()
        path = name + '.txt'
        (evidence / path).write_bytes(raw_content)
        run['artifacts'].append({'id': name, 'kind': kind, 'path': path,
                                 'sha256': hashlib.sha256(raw_content).hexdigest()})
        return name

    artifact('synthetic-reset', 'corpus_reset', 'Fictional application corpus reset receipt.')
    publishers = {p['id']: p for p in benchmark['publishers']}
    start = datetime(2026, 9, 25, tzinfo=timezone.utc)

    def stamp(time):
        return time.strftime('%Y-%m-%dT%H:%M:%SZ')

    for index, task in enumerate(benchmark['tasks']):
        task_id = task['id']
        task_start = start + timedelta(minutes=index)
        task_end = task_start + timedelta(seconds=10)
        base = 'https://' + publishers[task['publisher_id']]['domain']
        relevant, expansion = index < 18, index < 9
        status = 'successful' if index < 26 else ('no_results', 'blocked', 'failed', 'quota_exhausted')[index - 26]
        result = {'task_id': task_id, 'status': status, 'started_at': stamp(task_start),
                  'ended_at': stamp(task_end), 'stop_reason': 'Synthetic scenario only',
                  'effective_limits': {'max_hops': 2, 'max_requests': 50, 'max_seconds': 600},
                  'corpus_reset_artifact': artifact(task_id + '-reset', 'corpus_reset',
                                                   'Fictional per-task empty-corpus receipt.'),
                  'access_review_artifact': artifact(task_id + '-access', 'access_review',
                                                     'Fictional access review; no domain was contacted.'),
                  'local_search_artifact': artifact(task_id + '-search', 'local_search',
                                                    'Fictional local query: ' + task['query']),
                  'ui_capture_artifact': artifact(task_id + '-ui', 'ui_capture',
                                                  'Fictional application view receipt, not a screenshot.'),
                  'requests': [], 'labels': None}
        for number in range(2 if expansion else 1 if status == 'successful' else 0):
            body = ('Fictional Programme. Project identifier: invented-project. ' if number == 0
                    else 'Fictional project details. Identifier: invented-project. ')
            result['requests'].append({'url': base + '/' if number == 0 else base + '/' + task_id + '/' + str(number),
                                       'method': 'GET', 'hop': number,
                                       'purpose': 'seed' if number == 0 else 'link',
                                       'parent_request': None if number == 0 else 0,
                                       'started_at': stamp(task_start + timedelta(seconds=number)),
                                       'ended_at': stamp(task_start + timedelta(seconds=number + 1)),
                                       'outcome': 'fetched', 'http_status': 200,
                                       'original_artifact': artifact(task_id + '-original-' + str(number), 'original', body)})
        result['labels'] = {'reviewer_id': 'synthetic-reviewer', 'reviewed_at': '2026-09-26T00:00:00Z',
                            'relevant_result': relevant, 'useful_expansion': expansion,
                            'rationale': 'Fabricated label for scorer verification only.',
                            'source_requests': list(range(len(result['requests']))) if relevant else [],
                            'chain': {'identifier': 'invented-project', 'identifier_anchor': 'Text after Project identifier',
                                      'source_request': 0, 'lead_request': 1,
                                      'lead_anchor': 'Fictional project details'} if expansion else None}
        run['results'].append(result)
    (destination / 'run.json').write_text(json.dumps(run, indent=2) + '\n')
    return run


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path, help='A new directory for fabricated evidence')
    create_replay(parser.parse_args().output)
