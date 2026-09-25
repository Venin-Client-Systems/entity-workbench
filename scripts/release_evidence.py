"""Validate reviewed release declarations and retained evidence; never run evidence."""
from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import stat

from verify_runtime_bundle import InvalidInventory, is_link, path_key, read_json

ROOT = Path(__file__).resolve().parents[1]
POLICY = ROOT / 'packaging/release-acceptance.v1.json'
CATALOGUE = ROOT / 'fixtures/acceptance/catalogue.v1.json'
HEX40 = re.compile(r'[0-9a-f]{40}\Z')
HEX64 = re.compile(r'[0-9a-f]{64}\Z')
IDENTIFIER = re.compile(r'[a-z0-9][a-z0-9._-]{0,127}\Z')
MAX_RECORDS = 2000
MAX_EVIDENCE_BYTES = 64 * 1024 * 1024
MAX_ARTIFACT_BYTES = 32 * 1024**3

TARGETS = ('windows-x86_64', 'macos-aarch64', 'macos-x86_64')
GATE_KINDS = {'integrated_workflows': 'installed_workflow',
 'windows_appcontainer': 'sandbox_probe',
 'macos_signed_helpers': 'sandbox_probe',
 'all_runtime_dependencies_bundled': 'runtime_inventory',
 'offline_clean_install_three_targets': 'offline_install',
 'hostile_input_and_resource_exhaustion': 'hostile_input',
 'broad_local_web_coverage': 'live_discovery',
 'recovery_and_migration_failures': 'recovery',
 'benchmark_16gb_target': 'performance',
 'dependency_license_and_security_review': 'review',
 'signed_and_notarized_artifacts': 'signature_verification',
 'downloaded_artifact_verification': 'download_verification'}
SCENARIOS = ('clean_offline_installation',
 'discovery_from_incomplete_input',
 'difficult_document_import',
 'identity_ambiguity',
 'statement_reconciliation',
 'proximity_analysis',
 'correction_propagation',
 'hostile_input_and_networking',
 'recovery_and_upgrades',
 'broad_direct_web_local_search')
OS_VERSION = re.compile(r"(?:0|[1-9][0-9]{0,5})\.(?:0|[1-9][0-9]{0,5})\.(?:0|[1-9][0-9]{0,5})\Z")


class InvalidEvidence(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise InvalidEvidence(message)


def keys(value, expected, name):
    require(isinstance(value, dict) and set(value) == set(expected), name + ': unexpected or missing fields')


def text(value, name, maximum=1000):
    require(isinstance(value, str) and 0 < len(value) <= maximum and
            all(ord(c) >= 32 and ord(c) != 127 for c in value), name + ': invalid text')


def identifier(value, name):
    require(isinstance(value, str) and IDENTIFIER.fullmatch(value), name + ': invalid identifier')


def digest(value, expression, name):
    require(isinstance(value, str) and expression.fullmatch(value), name + ': invalid checksum/revision')


def file_reference(value, name, limit):
    keys(value, ('path', 'size', 'sha256'), name)
    path_key(value['path'], 64)
    require(type(value['size']) is int and 0 <= value['size'] <= limit, name + ': invalid byte size')
    digest(value['sha256'], HEX64, name)


def verify_file(root, ref, limit):
    """Read a frozen local tree; this build check is not an adversarial filesystem sandbox."""
    file_reference(ref, 'file', limit)
    root = Path(root)
    require(not is_link(root.lstat()) and root.is_dir(), 'Evidence/artifact root must be a real directory')
    current = root
    for part in ref['path'].split('/'):
        current = current / part
        require(not is_link(current.lstat()), 'Evidence/artifact links and reparse points are forbidden')
    before = current.stat()
    require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1, 'Evidence/artifact must be an ordinary non-hardlinked file')
    require(before.st_size == ref['size'], 'Evidence/artifact size mismatch: ' + ref['path'])
    checksum = hashlib.sha256()
    flags = os.O_RDONLY | getattr(os, 'O_BINARY', 0) | getattr(os, 'O_NOFOLLOW', 0)
    with os.fdopen(os.open(current, flags), 'rb') as stream:
        opened = os.fstat(stream.fileno())
        identity = lambda s: (s.st_dev, s.st_ino, s.st_size, s.st_nlink)
        stamp = lambda s: (*identity(s), s.st_mtime_ns, s.st_ctime_ns)
        require(identity(opened) == identity(before), 'File changed before hashing')
        size = 0
        while block := stream.read(1024 * 1024):
            size += len(block)
            require(size <= ref['size'], 'File grew while hashing')
            checksum.update(block)
        require(stamp(os.fstat(stream.fileno())) == stamp(opened), 'File changed while hashing')
        require(identity(current.lstat()) == identity(opened), 'File replaced while hashing')
    require(size == ref['size'] and checksum.hexdigest() == ref['sha256'], 'Evidence/artifact checksum mismatch: ' + ref['path'])


def validate_policy(policy):
    keys(policy, ('schema_version', 'targets', 'gates', 'scenarios', 'support_matrix'), 'policy')
    require(type(policy['schema_version']) is int and policy['schema_version'] == 1, 'Unsupported acceptance policy')
    require(policy['targets'] == list(TARGETS), 'Policy must retain all three targets')
    require(isinstance(policy['gates'], dict) and set(policy['gates']) == set(GATE_KINDS), 'Policy must retain all twelve gates')
    require(isinstance(policy['scenarios'], dict) and set(policy['scenarios']) == set(SCENARIOS), 'Policy must retain all ten scenarios')
    coverage, _ = read_json(ROOT / 'docs/delivery/gate-coverage.json')
    require(set(coverage['gates']) == set(GATE_KINDS), 'Gate ownership coverage is incomplete')
    for name, rule in policy['gates'].items():
        keys(rule, ('issues', 'targets', 'evidence_kind', 'procedure', 'procedure_id', 'procedure_version', 'max_age_days', 'reviewer_role'), 'gate rule')
        expected = list(TARGETS[:1]) if name == 'windows_appcontainer' else list(TARGETS[1:]) if name == 'macos_signed_helpers' else list(TARGETS)
        require(rule['targets'] == expected, 'Policy has empty, duplicate or wrong gate targets')
        require(rule['evidence_kind'] == GATE_KINDS[name], 'Policy evidence kind does not match gate')
        require(rule['issues'] and rule['issues'] == coverage['gates'][name] and len(set(rule['issues'])) == len(rule['issues']), 'Gate owner mapping differs from delivery coverage')
        identifier(rule['procedure_id'], 'required procedure id')
        text(rule['procedure_version'], 'required procedure version', 80)
        text(rule['procedure'], 'required procedure')
        require(type(rule['max_age_days']) is int and 1 <= rule['max_age_days'] <= 30, 'Evidence freshness must be between one and thirty days')
        expected_role = 'security-reviewer' if name in ('windows_appcontainer', 'macos_signed_helpers', 'hostile_input_and_resource_exhaustion', 'dependency_license_and_security_review') else 'release-verifier'
        require(rule['reviewer_role'] == expected_role, 'Gate reviewer role is inappropriate')
    for gates in policy['scenarios'].values():
        require(isinstance(gates, list) and gates and len(gates) == len(set(gates)) and set(gates) <= set(GATE_KINDS), 'Invalid scenario gate mapping')
    require(isinstance(policy['support_matrix'], dict) and set(policy['support_matrix']) == set(TARGETS), 'Invalid support matrix')
    for target, support in policy['support_matrix'].items():
        keys(support, ('minimum_version', 'tested_versions', 'decision_reference'), 'support boundary')
        if support['minimum_version'] is None:
            require(support['decision_reference'] is None and support['tested_versions'] == [], 'Undecided support boundary cannot claim a decision or test matrix')
        else:
            require(isinstance(support['minimum_version'], str) and OS_VERSION.fullmatch(support['minimum_version']), 'Invalid minimum OS version')
            identifier(support['decision_reference'], 'support decision reference')
            if target.startswith('windows'):
                require(support['minimum_version'].startswith('11.'), 'Windows 11 is the required platform')
            versions = support['tested_versions']
            require(isinstance(versions, list) and 0 < len(versions) <= 20 and
                    all(isinstance(v, str) and OS_VERSION.fullmatch(v) for v in versions), 'Invalid required OS test versions')
            require(len(set(versions)) == len(versions) and support['minimum_version'] in versions, 'OS test matrix must include the minimum without duplicates')
            for version in versions:
                require(tuple(map(int, version.split('.'))) >= tuple(map(int, support['minimum_version'].split('.'))), 'Test version is below the support minimum')
                if target.startswith('windows'):
                    require(version.startswith('11.'), 'Windows test version must be Windows 11')


def utc_time(value):
    text(value, 'UTC timestamp', 30)
    require(re.fullmatch(r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z', value), 'Timestamp must be UTC to seconds')
    observed = datetime.fromisoformat(value.replace('Z', '+00:00'))
    require(observed <= datetime.now(timezone.utc), 'Evidence date is in the future')
    return observed


def observation_digest(record):
    payload = {k: v for k, v in record.items() if k != 'review'}
    return hashlib.sha256(json.dumps(payload, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode()).hexdigest()


def verify_review(record, evidence_root, observed):
    review = record['review']
    keys(review, ('status', 'role', 'reference', 'receipt'), 'review')
    require(review['status'] in ('accepted', 'pending', 'rejected'), 'Invalid review status')
    if review['status'] == 'pending':
        require(all(review[k] is None for k in ('role', 'reference', 'receipt')), 'Pending review must not claim a reviewer')
        return
    require(review['role'] in ('maintainer', 'security-reviewer', 'release-verifier'), 'Invalid reviewer role')
    identifier(review['reference'], 'review reference')
    verify_file(evidence_root, review['receipt'], MAX_EVIDENCE_BYTES)
    receipt, _ = read_json(Path(evidence_root) / review['receipt']['path'])
    keys(receipt, ('schema_version', 'reference', 'evidence_id', 'observation_sha256', 'decision', 'reviewer_role', 'reviewed_at', 'reason'), 'review receipt')
    require(type(receipt['schema_version']) is int and receipt['schema_version'] == 1, 'Unsupported review receipt')
    require(receipt['reference'] == review['reference'] and receipt['evidence_id'] == record['id'], 'Review receipt references different evidence')
    require(receipt['observation_sha256'] == observation_digest(record), 'Review receipt is not bound to this observation')
    require(receipt['decision'] == review['status'] and receipt['reviewer_role'] == review['role'], 'Review decision/role mismatch')
    require(utc_time(receipt['reviewed_at']) >= observed, 'Review predates observation')
    text(receipt['reason'], 'review reason', 2000)


def validate_catalogue(path=CATALOGUE, policy_path=POLICY):
    policy, _ = read_json(policy_path)
    validate_policy(policy)
    catalogue, _ = read_json(path)
    keys(catalogue, ('schema_version', 'synthetic', 'scenarios', 'fixtures'), 'fixture catalogue')
    require(type(catalogue['schema_version']) is int and catalogue['schema_version'] == 1 and catalogue['synthetic'] is True,
            'Unsupported or non-synthetic fixture catalogue')
    require(isinstance(catalogue['scenarios'], dict) and set(catalogue['scenarios']) == set(policy['scenarios']),
            'All ten acceptance scenarios must be mapped')
    require(isinstance(catalogue['fixtures'], list) and 0 < len(catalogue['fixtures']) <= 100, 'Invalid fixture list')
    ids = set()
    paths = set()
    for fixture in catalogue['fixtures']:
        keys(fixture, ('id', 'kind', 'file', 'expected_assertions'), 'fixture')
        identifier(fixture['id'], 'fixture id')
        require(fixture['id'] not in ids, 'Duplicate fixture id')
        ids.add(fixture['id'])
        identifier(fixture['kind'], 'fixture kind')
        require(isinstance(fixture['expected_assertions'], list) and 0 < len(fixture['expected_assertions']) <= 30,
                'Expected fixture assertions required')
        for assertion in fixture['expected_assertions']:
            text(assertion, 'fixture assertion')
        verify_file(Path(path).parent, fixture['file'], MAX_EVIDENCE_BYTES)
        key = path_key(fixture['file']['path'], 64)
        require(key not in paths, 'Duplicate fixture path')
        paths.add(key)
    for scenario, row in catalogue['scenarios'].items():
        keys(row, ('fixtures', 'procedure', 'expected_result'), 'scenario')
        require(isinstance(row['fixtures'], list) and row['fixtures'] and all(x in ids for x in row['fixtures']),
                'Unknown or empty scenario fixtures: ' + scenario)
        text(row['procedure'], 'scenario procedure', 3000)
        text(row['expected_result'], 'scenario result', 3000)
    return len(ids)


def candidate_context(value, policy):
    if value is None:
        return None
    keys(value, ('id', 'source_revision', 'policy_sha256', 'fixture_catalogue_sha256', 'artifacts'), 'candidate')
    identifier(value['id'], 'candidate id')
    digest(value['source_revision'], HEX40, 'candidate revision')
    digest(value['policy_sha256'], HEX64, 'candidate policy')
    digest(value['fixture_catalogue_sha256'], HEX64, 'candidate fixtures')
    require(isinstance(value['artifacts'], dict) and set(value['artifacts']) == set(policy['targets']),
            'Candidate must identify all three artifacts')
    paths = set()
    for ref in value['artifacts'].values():
        file_reference(ref, 'candidate artifact', MAX_ARTIFACT_BYTES)
        key = path_key(ref['path'], 64)
        require(key not in paths, 'Candidate artifact paths collide')
        paths.add(key)
    return value


def evidence_record(record, policy, evidence_root):
    keys(record, ('schema_version', 'id', 'gate', 'target', 'source_revision', 'artifact_sha256',
                  'policy_sha256', 'fixture_catalogue_sha256', 'observed_at', 'environment', 'kind', 'procedure', 'result', 'review', 'attachments'), 'evidence')
    require(type(record['schema_version']) is int and record['schema_version'] == 1, 'Unsupported evidence schema')
    identifier(record['id'], 'evidence id')
    require(isinstance(record['gate'], str) and record['gate'] in policy['gates'], 'Unknown evidence gate')
    require(record['target'] in policy['gates'][record['gate']]['targets'], 'Wrong platform for evidence gate')
    digest(record['source_revision'], HEX40, 'evidence revision')
    digest(record['artifact_sha256'], HEX64, 'evidence artifact')
    digest(record['policy_sha256'], HEX64, 'observation policy')
    digest(record['fixture_catalogue_sha256'], HEX64, 'observation fixtures')
    observed = utc_time(record['observed_at'])
    env = record['environment']
    keys(env, ('os_family', 'os_version', 'architecture', 'memory_gib', 'context'), 'environment')
    family, arch = record['target'].split('-')
    require(env['os_family'] == family and env['architecture'] == arch, 'Evidence target/environment mismatch')
    require(isinstance(env['os_version'], str) and OS_VERSION.fullmatch(env['os_version']), 'OS version must be a normalized numeric triple')
    require(type(env['memory_gib']) is int and 1 <= env['memory_gib'] <= 4096, 'Invalid memory size')
    require(env['context'] in ('installed', 'clean_install', 'hosted_ci', 'development'), 'Invalid environment context')
    require(record['kind'] in {p['evidence_kind'] for p in policy['gates'].values()} | {'source_check'}, 'Unknown evidence kind')
    keys(record['procedure'], ('id', 'version', 'invocation'), 'procedure')
    identifier(record['procedure']['id'], 'procedure id')
    text(record['procedure']['version'], 'procedure version', 80)
    text(record['procedure']['invocation'], 'procedure invocation', 3000)
    require(record['result'] in ('passed', 'failed', 'blocked', 'not_run'), 'Invalid evidence result')
    verify_review(record, evidence_root, observed)
    refs = record['attachments']
    require(isinstance(refs, list) and 0 < len(refs) <= 20, 'Retained evidence attachments required')
    paths = set()
    for ref in refs:
        verify_file(evidence_root, ref, MAX_EVIDENCE_BYTES)
        key = path_key(ref['path'], 64)
        require(key not in paths, 'Duplicate evidence attachment')
        paths.add(key)
    return observed


def qualifies(record, policy):
    rule = policy['gates'][record['gate']]
    env = record['environment']
    support = policy['support_matrix'][record['target']]['minimum_version']
    if support is None or tuple(map(int, env['os_version'].split('.'))) < tuple(map(int, support.split('.'))):
        return False
    if record['review']['role'] != rule['reviewer_role']:
        return False
    if record['procedure']['id'] != rule['procedure_id'] or record['procedure']['version'] != rule['procedure_version']:
        return False
    if (datetime.now(timezone.utc) - utc_time(record['observed_at'])).total_seconds() > rule['max_age_days'] * 86400:
        return False
    if record['result'] != 'passed' or record['review']['status'] != 'accepted':
        return False
    if record['kind'] != rule['evidence_kind'] or env['context'] not in ('installed', 'clean_install'):
        return False
    if env['os_family'] == 'windows' and not env['os_version'].startswith('11.'):
        return False
    if record['gate'] in ('offline_clean_install_three_targets', 'downloaded_artifact_verification') and env['context'] != 'clean_install':
        return False
    if record['gate'] == 'benchmark_16gb_target' and env['memory_gib'] != 16:
        return False
    return True


def target_passes(gate, target, selected, policy, candidate):
    """Require the agreed OS matrix and retain failures on other observed supported versions."""
    support = policy['support_matrix'][target]
    if candidate is None or support['minimum_version'] is None:
        return False
    minimum = tuple(map(int, support['minimum_version'].split('.')))
    versions = set(support['tested_versions'])
    for observed_gate, observed_target, version in selected:
        if observed_gate == gate and observed_target == target and tuple(map(int, version.split('.'))) >= minimum:
            versions.add(version)
    for version in versions:
        observation = selected.get((gate, target, version))
        if observation is None:
            return False
        record = observation[1]
        if any(record[field] != candidate[field] for field in ('policy_sha256', 'fixture_catalogue_sha256')):
            return False
        if not qualifies(record, policy):
            return False
    return True


def evaluate(ledger_path, evidence_root, artifact_root=None, policy_only=False):
    report = {'schema_version': 1, 'scope': 'policy_only' if policy_only else 'candidate_verification',
              'policy_valid': False, 'complete_release': False, 'candidate_bytes_verified': False,
              'passed_gates': [], 'unpassed': [], 'retained_records': 0, 'noncurrent_records': [], 'errors': []}
    try:
        policy, report['policy_sha256'] = read_json(POLICY)
        validate_policy(policy)
        _, report['fixture_catalogue_sha256'] = read_json(CATALOGUE)
        ledger, report['ledger_sha256'] = read_json(ledger_path)
        keys(ledger, ('schema_version', 'complete_release', 'candidate', 'gates', 'evidence'), 'ledger')
        require(type(ledger['schema_version']) is int and ledger['schema_version'] == 2, 'Unsupported ledger schema')
        require(type(ledger['complete_release']) is bool, 'Release claim must be Boolean')
        require(isinstance(ledger['gates'], dict) and set(ledger['gates']) == set(policy['gates']), 'Release gates do not match policy')
        require(all(type(v) is bool for v in ledger['gates'].values()), 'Gate claims must be Boolean')
        candidate = candidate_context(ledger['candidate'], policy)
        if candidate:
            require(candidate['policy_sha256'] == report['policy_sha256'] and candidate['fixture_catalogue_sha256'] == report['fixture_catalogue_sha256'], 'Candidate acceptance-policy/fixture binding is stale')
        records = ledger['evidence']
        require(isinstance(records, list) and len(records) <= MAX_RECORDS, 'Evidence list exceeds bound or is invalid')
        total = 0
        for record in records:
            require(isinstance(record, dict) and isinstance(record.get('attachments'), list) and len(record['attachments']) <= 20, 'Invalid evidence attachments')
            for ref in record['attachments'] + ([record['review']['receipt']] if isinstance(record.get('review'), dict) and record['review'].get('receipt') is not None else []):
                file_reference(ref, 'evidence attachment', MAX_EVIDENCE_BYTES)
                total += ref['size']
                require(total <= 1024**3, 'Evidence exceeds aggregate one GiB bound')
        ids = set()
        observations = set()
        selected = {}
        for record in records:
            observed = evidence_record(record, policy, evidence_root)
            require(record['id'] not in ids, 'Duplicate evidence id')
            ids.add(record['id'])
            observation = (*tuple(record[k] for k in ('gate', 'target', 'source_revision', 'artifact_sha256', 'observed_at')), record['environment']['os_version'])
            require(observation not in observations, 'Ambiguous duplicate evidence observation')
            observations.add(observation)
            same_bytes = candidate and record['source_revision'] == candidate['source_revision'] and record['artifact_sha256'] == candidate['artifacts'][record['target']]['sha256']
            current = same_bytes and record['policy_sha256'] == report['policy_sha256'] and record['fixture_catalogue_sha256'] == report['fixture_catalogue_sha256']
            if not current:
                report['noncurrent_records'].append(record['id'])
            if same_bytes:
                pair = (record['gate'], record['target'], record['environment']['os_version'])
                if pair not in selected or observed > selected[pair][0]:
                    selected[pair] = (observed, record)
        report['retained_records'] = len(ids)
        for gate, rule in policy['gates'].items():
            passed = all(target_passes(gate, target, selected, policy, candidate) for target in rule['targets'])
            report['passed_gates' if passed else 'unpassed'].append(gate)
            require(ledger['gates'][gate] == passed, 'Gate claim lacks current reviewed evidence or hides a later outcome: ' + gate)
        claimed_complete = candidate is not None and not report['unpassed']
        require(ledger['complete_release'] == claimed_complete, 'Complete-release claim conflicts with evidence coverage')
        report['fixture_count'] = validate_catalogue(CATALOGUE, POLICY)
        report['policy_valid'] = True
        if not policy_only and candidate:
            require(artifact_root is not None, 'Candidate verification requires a local artifact root')
            for ref in candidate['artifacts'].values():
                verify_file(artifact_root, ref, MAX_ARTIFACT_BYTES)
            report['candidate_bytes_verified'] = True
            report['complete_release'] = claimed_complete
    except (InvalidEvidence, InvalidInventory, OSError, ValueError, KeyError, TypeError, RecursionError) as exc:
        detail = str(exc) if isinstance(exc, (InvalidEvidence, InvalidInventory)) else 'Unreadable or malformed release evidence'
        report['errors'].append(detail)
        report['complete_release'] = False
    return report
