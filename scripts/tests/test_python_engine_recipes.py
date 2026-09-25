"""Stdlib-only source/receipt checks; no candidate or third-party package execution."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import python_engine_receipts as receipts
import test_python_compatibility as common
import test_python_isolation as runner
import test_python_probe_contracts as legacy
from test_python_probe_contracts import successful_receipt, identity


def load(relative, name):
    spec = importlib.util.spec_from_file_location(name, common.ROOT / relative)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


sys.modules['runtime_support'] = load('workers/python/runtime_support.py', 'runtime_support')

engine = load('workers/python/probe/engine_recipes.py', 'fixed_engine_source')
compatibility = load('workers/python/probe/compatibility.py', 'fixed_compatibility_source')


def successful_engine(campaign, case):
    native, interpreter = successful_receipt(campaign)
    native.update(recipe=receipts.RECIPES[case], import_diagnostics=None, last_import_checkpoint=None,
                  engine_diagnostics={'valid': True, 'checkpoints': [dict(stage=stage,boundary=boundary,
                    elapsed_ms=i,process_cpu_ms=i) for i,(stage,boundary) in enumerate(engine.TIMES)]})
    native['assigned_files'] = {name: identity((common.ROOT / source).read_bytes() if source else b'fixed-assignment')
                                for name,source in receipts.assigned(case).items()}
    native['result'].update(recipe=receipts.RECIPES[case], checks=receipts.expected(case))
    return native,interpreter


class EngineSourceTests(unittest.TestCase):
    def test_timing_writer_requires_fixed_order_monotonic_bounds_and_no_clobber(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            with patch.object(engine.time,'monotonic_ns',side_effect=[0,1_000_000,2_000_000,1_000_000]), \
                 patch.object(engine.time,'process_time_ns',side_effect=[0,1_000_000,1_000_000,1_000_000]):
                timer=engine.Timings(root)
                timer.checkpoint(*engine.TIMES[0]); timer.checkpoint(*engine.TIMES[1])
                with self.assertRaises(ValueError): timer.checkpoint(*engine.TIMES[2])
            self.assertEqual(len(list(root.iterdir())),2)
            timer=engine.Timings(root)
            with self.assertRaises(FileExistsError): timer.checkpoint(*engine.TIMES[0])
            with self.assertRaises(ValueError): timer.checkpoint('private','path')
        for clock in (-1,120_001_000_000):
            with tempfile.TemporaryDirectory() as directory, \
                 patch.object(engine.time,'monotonic_ns',side_effect=[0,clock]):
                timer=engine.Timings(Path(directory))
                with self.assertRaises(ValueError): timer.checkpoint(*engine.TIMES[0])
                self.assertEqual(list(Path(directory).iterdir()),[])

    def execute_fake(self, case, operation=None, fail_import=False):
        temporary=tempfile.TemporaryDirectory();self.addCleanup(temporary.cleanup)
        job=Path(temporary.name).resolve();(job/'scratch').mkdir();(job/'code').mkdir()
        prefix=job/'prefix';site=prefix/'install/lib/python3.13/site-packages'
        fixture=common.read_json(common.ROOT/'workers/python/probe/fixture.json',64*1024)
        expected=common.read_json(common.ROOT/'workers/python/probe/expected.json',64*1024)
        phases=[]
        wanted=receipts.expected(case)
        operation = operation if operation is not None else {k:v for k,v in wanted.items() if k not in ('versions','imported_modules')}
        def fake_import(name):
            if fail_import: raise RuntimeError('synthetic original import failure')
            return SimpleNamespace(__file__=str(site/(name+'.py')),set_cpu_count=lambda _:None,set_io_thread_count=lambda _:None)
        adapter=SimpleNamespace(__file__=str(job/'code/transaction_totals.py'))
        with patch.dict(sys.modules,{'compatibility':compatibility,'transaction_totals':adapter}), \
             patch.object(compatibility,'distribution_versions',return_value=(fixture['versions'],[])) as metadata, \
             patch.object(engine.importlib,'import_module',side_effect=fake_import) as imports, \
             patch.object(compatibility,'graph_checks',return_value=operation), \
             patch.object(compatibility,'transaction_checks',return_value=operation):
            try:
                result=engine.execute(receipts.RECIPES[case],fixture,expected,prefix,job,phases.append)
            except Exception as error:
                result=error
        return result,phases,job,imports,metadata

    def test_each_recipe_verifies_all_versions_and_only_loads_its_fixed_operation_imports(self):
        for case in receipts.RECIPES:
            result,phases,job,imports,metadata=self.execute_fake(case)
            self.assertEqual(result,receipts.expected(case))
            self.assertEqual(phases,['versions','imports','init-ready','operation'])
            self.assertEqual([c.args[0] for c in imports.call_args_list],receipts.IMPORTS[case])
            self.assertEqual(len(metadata.call_args.args[1]),58)
            self.assertEqual(len(list((job/'scratch').iterdir())),4)

    def test_wrong_assertion_never_emits_operation_after_or_a_result(self):
        for case in receipts.RECIPES:
            result,phases,job,_,_=self.execute_fake(case,{'unexpected':True})
            self.assertIsInstance(result,ValueError)
            self.assertEqual(phases[-1],'operation')
            self.assertFalse((job/'scratch/engine-time-3.json').exists())
            self.assertFalse((job/'scratch/result.json').exists())

    def test_import_failure_preserves_original_error_and_never_claims_ready(self):
        result,phases,job,_,_=self.execute_fake('networkx',fail_import=True)
        self.assertIsInstance(result,RuntimeError)
        self.assertEqual(str(result),'synthetic original import failure')
        self.assertEqual(phases,['versions','imports'])
        self.assertEqual(len(list((job/'scratch').iterdir())),1)


class EngineReceiptTests(unittest.TestCase):
    def test_success_binds_original_fields_and_disallows_cross_recipe_or_combined_claims(self):
        for case in receipts.RECIPES:
            campaign=str(uuid.uuid4());native,interpreter=successful_engine(campaign,case)
            receipts.accept(native,campaign,interpreter,case)
            mutations=[lambda n:n.update(recipe='python-compatibility-v1'),
                       lambda n:n['result']['checks'].update(phrase_empty=True),
                       lambda n:n['result']['checks']['versions'].pop('spacy'),
                       lambda n:n['result'].update(recipe='python-compatibility-v1'),
                       lambda n:n.update(termination_state='unverified'),
                       lambda n:n.update(campaign_id=str(uuid.uuid4())),
                       lambda n:n['assigned_files']['code/engine_recipes.py'].update(sha256='a'*64),
                       lambda n:n['engine_diagnostics'].update(valid=False),
                       lambda n:n['engine_diagnostics']['checkpoints'].pop()]
            for change in mutations:
                n=copy.deepcopy(native);change(n)
                with self.assertRaises(common.ProbeFailure):receipts.accept(n,campaign,interpreter,case)
            with self.assertRaises(common.ProbeFailure):common.failure_summary(native,campaign)
        native,interpreter=successful_receipt(str(uuid.uuid4()))
        common.accept_native(native,native['campaign_id'],interpreter)
        with self.assertRaises(common.ProbeFailure):receipts.summary(native,native['campaign_id'],'networkx')

    def test_partial_failure_keeps_ordered_diagnostics_and_rejects_unsafe_metadata(self):
        campaign=str(uuid.uuid4());n,_=successful_engine(campaign,'networkx')
        n.update(passed=False,failure='quota-exhausted',quota_kind='wall-time',result={'private':'path'})
        n['engine_diagnostics'].update(valid=False,checkpoints=n['engine_diagnostics']['checkpoints'][:1])
        summary=receipts.summary(n,campaign,'networkx')
        self.assertEqual(summary['failure'],'quota-exhausted');self.assertNotIn('result',summary)
        for change in [lambda r:r.update(stage='private'),lambda r:r.update(elapsed_ms=True),
                       lambda r:r.update(process_cpu_ms=-1),lambda r:r.update(extra='private')]:
            bad=copy.deepcopy(n);change(bad['engine_diagnostics']['checkpoints'][0])
            with self.assertRaises(common.ProbeFailure):receipts.summary(bad,campaign,'networkx')
        for records in [[],n['engine_diagnostics']['checkpoints']]:
            common.validate_engine_diagnostics({'valid':False,'checkpoints':records})


@unittest.skipUnless(os.name=='posix','POSIX-only runner fixtures')
class EngineRunnerTests(unittest.TestCase):
    setUp=legacy.ProbeRunnerTests.setUp
    def patches(self):
        from contextlib import ExitStack
        stack=ExitStack();self.addCleanup(stack.close)
        stack.enter_context(patch.object(runner,'source_identity',return_value={'commit':'synthetic'}))
        stack.enter_context(patch.object(common,'build_binary',return_value=self.binary))
        stack.enter_context(patch.object(runner.platform,'system',return_value='Darwin'))
        stack.enter_context(patch.object(runner.platform,'machine',return_value='arm64'))
        stack.enter_context(patch.object(runner.platform,'mac_ver',return_value=('26.0.1',(),'')))
        verify=stack.enter_context(patch.object(common.installed,'verify',return_value={'verified':True}))
        return verify

    def test_exact_native_selection_and_failure_survive_post_integrity(self):
        verify=self.patches()
        def fail(command,path,timeout,environment):
            self.assertEqual(timeout,120);self.assertIn(receipts.TESTS['networkx'],command)
            n,_=successful_engine(environment['WORKBENCH_TEST_PYTHON_CAMPAIGN'],'networkx')
            n.update(passed=False,phase='confined-compatibility',failure='quota-exhausted',quota_kind='wall-time',exit_code=None,result=None)
            n['engine_diagnostics'].update(valid=False,checkpoints=n['engine_diagnostics']['checkpoints'][:1])
            (self.artifacts/'native-report.json').write_text(json.dumps(n))
            return SimpleNamespace(returncode=1)
        with patch.object(common,'run_logged',side_effect=fail) as launch:
            report=runner.observe(self.prefix,self.artifacts,'networkx')
        launch.assert_called_once();self.assertEqual(verify.call_count,3)
        self.assertFalse(report['passed']);self.assertEqual(report['native']['quota_kind'],'wall-time')
        self.assertEqual(report['termination_state'],'confirmed')
        self.assertEqual(report['post_original_inventory'],report['post_selected_inventory'])

    def test_success_has_one_recipe_process_and_unchanged_prefix(self):
        self.patches()
        def run(command,path,timeout,environment):
            self.assertIn(receipts.TESTS['transactions'],command)
            n,_=successful_engine(environment['WORKBENCH_TEST_PYTHON_CAMPAIGN'],'transactions')
            (self.artifacts/'native-report.json').write_text(json.dumps(n))
            return SimpleNamespace(returncode=0)
        with patch.object(common,'run_logged',side_effect=run) as launch:
            report=runner.observe(self.prefix,self.artifacts,'transactions')
        launch.assert_called_once();self.assertTrue(report['passed']);self.assertFalse(report['complete_release'])

    def test_unconfirmed_native_receipt_cannot_trigger_post_verification(self):
        verify=self.patches()
        def run(command,path,timeout,environment):
            n,_=successful_engine(environment['WORKBENCH_TEST_PYTHON_CAMPAIGN'],'networkx')
            n.update(passed=False,termination_state='unconfirmed',result=None)
            (self.artifacts/'native-report.json').write_text(json.dumps(n))
            return SimpleNamespace(returncode=1)
        with patch.object(common,'run_logged',side_effect=run) as launch:
            report=runner.observe(self.prefix,self.artifacts,'networkx')
        launch.assert_called_once();verify.assert_called_once()
        self.assertFalse(report['passed']);self.assertEqual(report['termination_state'],'unverified')
        self.assertNotIn('post_original_inventory',report)

    def test_outer_timeout_does_not_retry_or_post_verify(self):
        import subprocess
        verify=self.patches()
        def timeout(command,path,seconds,environment):
            n,_=successful_engine(environment['WORKBENCH_TEST_PYTHON_CAMPAIGN'],'transactions')
            n.update(passed=False,termination_state='unconfirmed',result=None)
            (self.artifacts/'native-report.json').write_text(json.dumps(n))
            raise subprocess.TimeoutExpired('fixed',120)
        with patch.object(common,'run_logged',side_effect=timeout) as launch:
            report=runner.observe(self.prefix,self.artifacts,'transactions')
        launch.assert_called_once();verify.assert_called_once()
        self.assertFalse(report['passed']);self.assertEqual(report['termination_state'],'unverified')
        self.assertNotIn('post_original_inventory',report)
        self.assertEqual(report['native']['recipe'],'python-transactions-v1')


if __name__=='__main__':unittest.main()
