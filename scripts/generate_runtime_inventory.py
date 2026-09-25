#!/usr/bin/env python3
"""Generate a reviewed, offline bundle inventory; never infer runtime identity."""
from __future__ import annotations

import argparse
import bisect
import json
import os
from pathlib import Path
import sys
import tempfile

import verify_runtime_bundle as verifier


def require(condition, detail):
    if not condition:
        raise verifier.InvalidInventory(detail)


def declarations(value, policy, target):
    verifier.exact_keys(value, ('schema_version', 'target', 'ocr_languages', 'regions', 'components'), 'ownership plan')
    require(verifier.integer(value['schema_version']) and value['schema_version'] == 1,
            'Unsupported ownership-plan schema')
    require(target in policy['targets'] and value['target'] == target, 'Ownership target mismatch')
    languages = verifier.identifiers(value['ocr_languages'], 'ocr_languages')
    regions = verifier.identifiers(value['regions'], 'regions')
    require(set(policy['minimum_ocr_languages']).issubset(languages), 'Required OCR language declaration missing')
    required = set(policy['common'] + policy['targets'][target])
    required.update('ocr-language:' + language for language in languages)
    required.update('region:' + region for region in regions)
    require(isinstance(value['components'], list) and len(value['components']) <= 2100,
            'Expected bounded component declarations')
    seen, count = set(), 0
    for component in value['components']:
        verifier.exact_keys(component, ('id', 'version', 'paths'), 'component ownership')
        name = component['id']
        require(isinstance(name, str) and name in required and name not in seen,
                'Unknown, repeated or unadvertised component')
        seen.add(name)
        require(isinstance(component['version'], str) and verifier.VERSION.fullmatch(component['version']),
                'Expected explicit pinned component version')
        paths = component['paths']
        require(isinstance(paths, list) and paths, 'Expected nonempty literal paths')
        count += len(paths)
        require(count <= policy['limits']['files'], 'Ownership selector count exceeded')
        keys = [verifier.path_key(path, policy['limits']['path_depth']) for path in paths]
        require(len(set(keys)) == len(keys), 'Duplicate ownership path')
    return value


def ownership(plan, actual, limits):
    names = sorted(actual)
    components, all_owned = [], set()
    total_references = 0
    for component in sorted(plan['components'], key=lambda entry: entry['id']):
        owned = set()
        for selector in component['paths']:
            if selector in actual:
                selected = [selector]
            else:
                # Literal directory expansion, not a glob or a filesystem open.
                # Binary search avoids scanning the whole tree per selector.
                prefix = selector + '/'
                position = bisect.bisect_left(names, prefix)
                selected = []
                while position < len(names) and names[position].startswith(prefix):
                    selected.append(names[position])
                    position += 1
            require(selected, 'Ownership path is absent or contains no regular files')
            require(not owned.intersection(selected), 'Overlapping paths within one component')
            total_references += len(selected)
            require(total_references <= limits['files'] * 2, 'Expanded ownership reference bound exceeded')
            owned.update(selected)
        all_owned.update(owned)
        components.append({'id': component['id'], 'version': component['version'], 'files': sorted(owned)})
    require(all_owned == set(actual), 'Bundle contains files without explicit component ownership')
    return components


def write_bounded(stream, value, maximum):
    count = 0
    encoder = json.JSONEncoder(indent=2, sort_keys=True, ensure_ascii=True, allow_nan=False)
    for part in encoder.iterencode(value):
        data = part.encode('utf-8')
        count += len(data)
        require(count < maximum, 'Generated inventory exceeds JSON limit')
        stream.write(data)
    stream.write(b'\n')


def generate(bundle_root, plan_path, inventory_path, target):
    report = {'schema_version': 1, 'target': target, 'published': False, 'generated': False, 'complete': False,
              'complete_release': False, 'plan_sha256': None, 'requirements_sha256': None,
              'inventory_sha256': None, 'verification': None, 'failure': None}
    pending = None
    try:
        policy, report['requirements_sha256'] = verifier.read_json(verifier.POLICY_PATH)
        plan, report['plan_sha256'] = verifier.read_json(plan_path)
        declarations(plan, policy, target)
        root = Path(bundle_root).absolute()
        output = Path(inventory_path).absolute()
        require(not os.path.lexists(output), 'Inventory destination already exists')
        resolved_root = root.resolve(strict=True)
        require(not output.parent.resolve(strict=True).joinpath(output.name).is_relative_to(resolved_root),
                'Inventory destination must be outside the bundle')
        require(not Path(plan_path).resolve(strict=True).is_relative_to(resolved_root),
                'Ownership plan must be outside the bundle')

        def reject(code, detail, **_):
            raise verifier.InvalidInventory(detail)

        actual = verifier.scan_bundle(root, policy['limits'], reject)
        require(len(actual) <= policy['limits']['files'], 'Bundle file count exceeded')
        sizes = [info.st_size for info in actual.values()]
        require(all(0 <= size <= policy['limits']['file_bytes'] for size in sizes)
                and sum(sizes) <= policy['limits']['total_bytes'], 'Bundle byte limit exceeded')
        components = ownership(plan, actual, policy['limits'])
        files = []
        for path, before in sorted(actual.items()):
            digest, size = verifier.hash_asset(root, path, before, before.st_size)
            files.append({'path': path, 'size': size, 'sha256': digest})
        inventory = {'schema_version': 1, 'target': target,
                     'ocr_languages': sorted(plan['ocr_languages']), 'regions': sorted(plan['regions']),
                     'components': components, 'files': files}
        with tempfile.NamedTemporaryFile(prefix='.ew-inventory-', suffix='.pending',
                                         dir=output.parent, delete=False) as stream:
            pending = Path(stream.name)
            write_bounded(stream, inventory, policy['limits']['manifest_bytes'])
            stream.flush()
            os.fsync(stream.fileno())
        verification = verifier.verify(root, pending, target)
        report['verification'] = verification
        # A complete file inventory is useful for partial staging. Missing
        # mandatory components stay unpassed; any other defect prevents output.
        require(not verification.get('errors_truncated') and not verification['invalid_components']
                and all(error['code'] == 'missing_components' for error in verification['errors']),
                'Generated inventory failed independent contract verification')
        require(verification['requirements_sha256'] == report['requirements_sha256'],
                'Runtime requirements changed during generation')
        _, plan_after = verifier.read_json(plan_path)
        require(plan_after == report['plan_sha256'], 'Ownership plan changed during generation')
        # Publish closed, flushed bytes without replacing an existing inventory.
        # Hard-link publication is build-output-only, outside the bundle; the
        # temporary name is removed immediately so the final file has one link.
        os.link(pending, output)
        report['published'] = True
        try:
            pending.unlink()
        except OSError as exc:
            raise verifier.InvalidInventory('Inventory publication cleanup failed') from exc
        pending = None
        report['generated'] = True
        report['complete'] = verification['complete']
        report['inventory_sha256'] = verification['inventory_sha256']
    except (verifier.InvalidInventory, OSError, ValueError, TypeError, KeyError, RecursionError) as exc:
        report['failure'] = str(exc) if isinstance(exc, verifier.InvalidInventory) else 'Unreadable plan, policy, bundle or destination'
    finally:
        if pending is not None:
            try:
                pending.unlink()
            except OSError:
                report['failure'] = 'Inventory temporary-file cleanup failed'
                report['generated'] = False
                report['complete'] = False
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--plan', type=Path, required=True)
    parser.add_argument('--inventory', type=Path, required=True)
    parser.add_argument('--target', choices=('windows-x86_64', 'macos-aarch64', 'macos-x86_64'), required=True)
    args = parser.parse_args()
    report = generate(args.bundle, args.plan, args.inventory, args.target)
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report['generated'] and report['complete'] else 1


if __name__ == '__main__':
    sys.exit(main())
