"""Only trusted stdlib and mocked audit calls; no candidate or package execution."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import test_python_compatibility as runner

spec = importlib.util.spec_from_file_location('reviewed_import_diagnostics',
    runner.ROOT / 'workers/python/probe/import_diagnostics.py')
source = importlib.util.module_from_spec(spec)
spec.loader.exec_module(source)


class DiagnosticsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.scratch = Path(self.temp.name)
        self.wall = 100_000_000; self.cpu = 50_000_000
        self.diagnostic = source.ImportDiagnostics(self.scratch, clock=lambda: self.wall, cpu=lambda: self.cpu)

    def install(self):
        with patch.object(source.sys, 'addaudithook') as registration:
            self.diagnostic.install()
        registration.assert_called_once_with(self.diagnostic.observe)

    def test_top_clocks_are_elapsed_from_origin_ordered_and_bounded(self):
        self.install()
        for index, module in enumerate(source.IMPORTS):
            for offset, boundary in enumerate(('before', 'after')):
                self.wall += 2_000_000; self.cpu += 1_000_000
                self.diagnostic.checkpoint(module, boundary)
                path = self.scratch / f'import-{index * 2 + offset}.json'
                self.assertLessEqual(path.stat().st_size, 512)
                self.assertEqual(json.loads(path.read_bytes()), {'module': module, 'boundary': boundary,
                    'elapsed_ms': (index * 2 + offset + 1) * 2, 'process_cpu_ms': index * 2 + offset + 1})
        self.diagnostic.finish()
        self.assertFalse(self.diagnostic.active)
        with self.assertRaises(ValueError): self.diagnostic.checkpoint('pyarrow', 'after')
        self.assertEqual(len(list(self.scratch.iterdir())), 12)

    def test_clock_regression_type_overflow_and_wrong_order_refuse_success(self):
        with self.assertRaises(ValueError): self.diagnostic.checkpoint('spacy', 'before')
        self.assertFalse(list(self.scratch.iterdir()))
        self.diagnostic.checkpoint('duckdb', 'before')
        self.wall -= 1_000_000
        with self.assertRaises(ValueError): self.diagnostic.checkpoint('duckdb', 'after')
        self.wall = 100_000_000; self.cpu -= 1_000_000
        with self.assertRaises(ValueError): self.diagnostic.checkpoint('duckdb', 'after')
        self.cpu = 50_000_000; self.wall += 120_001_000_000
        with self.assertRaises(ValueError): self.diagnostic.checkpoint('duckdb', 'after')
        self.wall = 101_000_000.0
        with self.assertRaises(ValueError): self.diagnostic.checkpoint('duckdb', 'after')
        self.assertEqual(len(list(self.scratch.iterdir())), 1)

    def test_audit_has_only_sixteen_first_seen_enums_and_at_most_eight_kib(self):
        self.install()
        for index, module in enumerate(sorted(source.ATTEMPTS)):
            self.wall += 1_000_000; self.cpu += 1_000_000
            self.diagnostic.observe('import', (module, '/private/synthetic', object()))
            self.diagnostic.observe('import', (module, '/private/synthetic', object()))
            value = json.loads((self.scratch / f'import-attempt-{index}.json').read_bytes())
            self.assertEqual(value, {'module': module, 'ordinal': index, 'elapsed_ms': index + 1,
                                    'process_cpu_ms': index + 1})
        self.assertFalse(self.diagnostic.active)
        files = list(self.scratch.iterdir())
        self.assertEqual(len(files), 16)
        self.assertTrue(all(path.stat().st_size <= 512 for path in files))
        self.assertLessEqual(sum(path.stat().st_size for path in files), 8192)
        self.assertNotIn('/private/', ''.join(path.read_text() for path in files))

    def test_noise_inactive_reentrant_and_unsafe_arguments_do_no_io_or_clock_reads(self):
        class Unsafe:
            def __str__(self): raise AssertionError('Do not stringify')
            def __repr__(self): raise AssertionError('Do not format')
            def __hash__(self): raise AssertionError('Do not hash')
        self.install()
        with patch.object(self.diagnostic, 'stamp', side_effect=AssertionError('No clock read')), \
                patch.object(self.diagnostic, 'writer', side_effect=AssertionError('No write')):
            for event, args in [('open', ('numpy',)), ('import', ()), ('import', [Unsafe()]),
                                ('import', (Unsafe(),)), ('import', ('x' * 100_000,)),
                                ('import', ('not_selected',)), (Unsafe(), ('numpy',))]:
                self.diagnostic.observe(event, args)
            self.diagnostic.busy = True
            self.diagnostic.observe('import', ('numpy',))
            self.diagnostic.busy = False; self.diagnostic.active = False
            self.diagnostic.observe('import', ('numpy',))
        self.assertFalse(list(self.scratch.iterdir()))

    def test_reentrancy_drops_nested_diagnostic_events_without_clobber(self):
        self.install()
        def recursive(path, record):
            self.diagnostic.observe('import', ('blis',))
            source.write_record(path, record)
        self.diagnostic.writer = recursive
        self.diagnostic.observe('import', ('numpy',))
        self.assertEqual(self.diagnostic.seen, {'numpy'})
        self.assertEqual(len(list(self.scratch.iterdir())), 1)

    def test_observer_io_failure_does_not_mask_import_exception_and_cannot_pass(self):
        class OriginalImportFailure(Exception): pass
        self.install()
        with patch.object(self.diagnostic, 'writer', side_effect=OSError('private synthetic error')):
            with self.assertRaises(OriginalImportFailure):
                self.diagnostic.observe('import', ('numpy',))
                raise OriginalImportFailure()
        self.assertTrue(self.diagnostic.failed)
        self.assertFalse(self.diagnostic.active)
        self.diagnostic.top_count = 12
        with self.assertRaisesRegex(ValueError, '^fixed-import-diagnostic-contract$'): self.diagnostic.finish()
        self.assertFalse(list(self.scratch.iterdir()))

    def test_writer_refuses_oversize_and_existing_file_without_modification(self):
        path = self.scratch / 'fixed.json'
        source.write_record(path, {'fixed': 1})
        before = path.read_bytes()
        with self.assertRaises(FileExistsError): source.write_record(path, {'fixed': 2})
        with self.assertRaises(ValueError): source.write_record(self.scratch / 'large.json', {'fixed': 'x' * 513})
        self.assertEqual(path.read_bytes(), before)
        self.assertFalse((self.scratch / 'large.json').exists())


class ReaderTests(unittest.TestCase):
    def valid(self):
        return {'valid': True,
                'checkpoints': [{'module': 'duckdb', 'boundary': 'before', 'elapsed_ms': 3, 'process_cpu_ms': 1},
                                {'module': 'duckdb', 'boundary': 'after', 'elapsed_ms': 5, 'process_cpu_ms': 2}],
                'attempts': [{'module': 'numpy', 'ordinal': 0, 'elapsed_ms': 4, 'process_cpu_ms': 1}]}

    def test_runner_rejects_extra_names_fields_clocks_gaps_counts_and_duplicate_attempts(self):
        last = {'module': 'duckdb', 'boundary': 'after'}
        runner.validate_import_diagnostics(self.valid(), last)
        changes = [lambda d: d.update(extra='private'), lambda d: d.update(valid=1),
            lambda d: d['checkpoints'][1].update(elapsed_ms=2),
            lambda d: d['checkpoints'][1].update(process_cpu_ms=0),
            lambda d: d['checkpoints'][1].update(elapsed_ms=True),
            lambda d: d['checkpoints'][1].update(process_cpu_ms=120001),
            lambda d: d['checkpoints'][1].update(module='spacy'),
            lambda d: d['checkpoints'].append({'module': 'private'}),
            lambda d: d['attempts'][0].update(module=['numpy']),
            lambda d: d['attempts'][0].update(module='/private/sentinel'),
            lambda d: d['attempts'][0].update(ordinal=True),
            lambda d: d['attempts'][0].update(ordinal=1),
            lambda d: d['attempts'][0].update(path='/private/sentinel'),
            lambda d: d['attempts'].append({**d['attempts'][0], 'ordinal': 1}),
            lambda d: d.update(attempts=d['attempts'] * 17),
            lambda d: d.update(checkpoints=d['checkpoints'] * 7)]
        for change in changes:
            value = self.valid(); change(value)
            with self.assertRaises(runner.ProbeFailure): runner.validate_import_diagnostics(value, last)
        with self.assertRaises(runner.ProbeFailure): runner.validate_import_diagnostics(self.valid(), None)

    def test_absent_events_and_invalid_stream_with_safe_prefix_remain_distinct(self):
        runner.validate_import_diagnostics(None, None)
        runner.validate_import_diagnostics({'valid': True, 'checkpoints': [], 'attempts': []}, None)
        partial = self.valid(); partial['valid'] = False
        runner.validate_import_diagnostics(partial, {'module': 'duckdb', 'boundary': 'after'})


if __name__ == '__main__': unittest.main()
