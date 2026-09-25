"""Source-only canonical recipe/receipt tests. No staged interpreter or plugin execution."""
import copy
from contextlib import ExitStack
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import python_canonical_graph_receipts as receipts
import test_python_compatibility as common
import test_python_isolation as runner
import test_python_probe_contracts as legacy
import test_python_engine_recipes as engine_tests
from test_python_engine_recipes import load, successful_engine
from test_python_probe_contracts import identity

graph = load('workers/python/probe/canonical_graph.py', 'canonical_graph_source')
adapter = load('workers/python/graph_path.py', 'graph_adapter_source')
engine = load('workers/python/probe/engine_recipes.py', 'canonical_graph_timing')
compatibility = load('workers/python/probe/compatibility.py', 'canonical_graph_compatibility')


def request():
    value = common.read_json(common.ROOT/'workers/python/fixtures/canonical-graph-cases.v1.json', 64*1024)['cases'][0]['request']
    value.update(nonce=str(uuid.uuid4()), workspace_revision=2, snapshot_sha256='b'*64)
    return value


def response(value):
    result = {key: value for key, value in value.items() if key not in ('nodes','edges','source_id','target_id')}
    result['outcome'] = {'state':'path','nodes':['a','b','c']}
    return json.dumps(result).encode()


def successful_graph(campaign, artifacts):
    native, interpreter = successful_engine(campaign, 'networkx')
    req = request(); raw = json.dumps(req).encode(); result = response(req)
    (artifacts/'captured-request.json').write_bytes(raw)
    (artifacts/'captured-result.json').write_bytes(result)
    capture = dict(nonce=req['nonce'],workspace_revision=2,snapshot_sha256=req['snapshot_sha256'],
                   request_identity=identity(raw),source_id='a',target_id='c',canonical_state_sha256='c'*64)
    native.update(recipe=receipts.RECIPE,canonical_capture=capture,graph_output_identity=identity(result))
    native['assigned_files'] = {name:identity((common.ROOT/source).read_bytes() if source else b'synthetic-assignment')
                                for name,source in receipts.ASSIGNED.items()}
    native['assigned_files']['input/graph-request.json'] = identity(raw)
    worker = native.pop('result');worker.update(recipe=receipts.RECIPE)
    worker['checks'] = dict(versions=common.read_json(common.ROOT/'workers/python/probe/fixture.json',64*1024)['versions'],
        imported_modules=['networkx'],backend_metadata_checked=True,campaign_id=campaign,capture_nonce=req['nonce'],
        request_identity=identity(raw),result_identity=identity(result))
    wrapper=json.dumps(worker).encode();(artifacts/'captured-wrapper.json').write_bytes(wrapper)
    native['wrapper_output_identity']=identity(wrapper)
    native['result'] = {'worker':worker,'canonical':dict(validated=True,canonical_unchanged=True,workspace_revision=2,
        snapshot_sha256=req['snapshot_sha256'],nodes=['a','b','c'],hop_assertion_ids=[['r1','r1-parallel'],['r2']],limitation=receipts.LIMITATION)}
    return native,interpreter


class CanonicalSourceTests(unittest.TestCase):
    def entry(self, **changes):
        value = SimpleNamespace(name=graph.LOOPBACK[0],value=graph.LOOPBACK[1],
            dist=SimpleNamespace(metadata={'Name':'networkx'},version='3.6.1'),load=Mock(side_effect=AssertionError('must not load')))
        for key,item in changes.items():setattr(value,key,item)
        return value

    def guard(self, info=(), backends=None):
        backends = [self.entry()] if backends is None else backends
        modules = {key:value for key,value in sys.modules.items() if key!='networkx'}
        with patch.dict(sys.modules,modules,clear=True), patch.dict(os.environ,{},clear=True), \
             patch.object(graph.importlib.metadata,'entry_points',side_effect=lambda group:info if group=='networkx.backend_info' else backends):
            graph.preimport_backends()
        for item in (*info,*backends): item.load.assert_not_called()

    def test_metadata_control_passes_without_loading_and_rejects_backend_plugins_before_import(self):
        self.guard()
        malicious=self.entry(name='private-provider')
        with self.assertRaises(ValueError):self.guard(info=[malicious])
        malicious.load.assert_not_called()
        for entries in ([],[self.entry(),self.entry()], [self.entry(name='other')],
                        [self.entry(value='arbitrary:call')], [self.entry(dist=None)],
                        [self.entry(dist=SimpleNamespace(metadata={'Name':'different'},version='3.6.1'))],
                        [self.entry(dist=SimpleNamespace(metadata={'Name':'networkx'},version='9'))]):
            with self.assertRaises(ValueError):self.guard(backends=entries)
            for item in entries:item.load.assert_not_called()
        with patch.dict(sys.modules,{'networkx':object()}), patch.object(graph.importlib.metadata,'entry_points') as metadata:
            with self.assertRaises(ValueError):graph.preimport_backends()
            metadata.assert_not_called()
        with patch.dict(os.environ,{'NETWORKX_AUTOMATIC_BACKENDS':'malicious'}), patch.dict(sys.modules,{'networkx':None}):
            # Remove the module so environment refusal, rather than prior-import refusal, is exercised.
            del sys.modules['networkx']
            with patch.object(graph.importlib.metadata,'entry_points') as metadata:
                with self.assertRaises(ValueError):graph.preimport_backends()
                metadata.assert_not_called()

    def execute_fake(self, bad_request=None, bad_assignment=None, guard_error=False, operation_error=False, clobber=False):
        temporary=tempfile.TemporaryDirectory();self.addCleanup(temporary.cleanup)
        job=Path(temporary.name).resolve()
        for child in ('code','input','scratch'): (job/child).mkdir()
        prefix=job/'prefix';site=prefix/'install/lib/python3.13/site-packages'
        req=request();raw=json.dumps(req).encode() if bad_request is None else bad_request
        (job/'input/graph-request.json').write_bytes(raw)
        assignment=dict(schema_version=1,job_id=str(uuid.uuid4()),prefix=str(prefix),manifest_sha256=common.MANIFEST,
                        campaign_id=str(uuid.uuid4()),capture_nonce=req['nonce'],request_identity=identity(raw))
        if bad_assignment:bad_assignment(assignment)
        fixture=common.read_json(common.ROOT/'workers/python/probe/fixture.json',64*1024)
        phases=[];result_bytes=response(req)
        if clobber:(job/'scratch/graph-result.json').write_bytes(b'preserve')
        fake_adapter=SimpleNamespace(__file__=str(job/'code/graph_path.py'),parse_request=adapter.parse_request,
            MAX_RESULT_BYTES=adapter.MAX_RESULT_BYTES,execute=Mock(side_effect=RuntimeError('original-operation') if operation_error else None,return_value=result_bytes))
        nx=SimpleNamespace(__version__='3.6.1',__file__=str(site/'networkx/__init__.py'),
                           utils=SimpleNamespace(backends=SimpleNamespace(backends={},_loaded_backends={},backend_info={'networkx':{}})))
        with patch.dict(sys.modules,{'compatibility':compatibility,'engine_recipes':engine,'graph_path':fake_adapter}), \
             patch.object(compatibility,'distribution_versions',return_value=(fixture['versions'],[])) as versions, \
             patch.object(graph,'preimport_backends',side_effect=ValueError('metadata-denied') if guard_error else None) as guard, \
             patch.object(graph.importlib,'import_module',return_value=nx) as imported:
            try: outcome=graph.execute(assignment,fixture,prefix,job,phases.append)
            except Exception as error: outcome=error
        return outcome,phases,job,imported,guard,versions

    def test_validated_input_precedes_imports_and_complete_operation_is_hash_bound(self):
        result,phases,job,imported,guard,versions=self.execute_fake()
        self.assertEqual(phases,['versions','metadata','imports','init-ready','operation'])
        imported.assert_called_once_with('networkx');guard.assert_called_once()
        self.assertEqual(len(versions.call_args.args[1]),58)
        self.assertEqual(result['result_identity'],identity((job/'scratch/graph-result.json').read_bytes()))
        self.assertEqual(len(list((job/'scratch').glob('engine-time-*.json'))),4)

    def test_invalid_request_identity_and_plugin_metadata_never_import(self):
        bads=[b'{}',b' '*65537,b'{"schema_version":1,"schema_version":1}']
        for raw in bads:
            outcome,_,job,imported,guard,_=self.execute_fake(bad_request=raw)
            self.assertIsInstance(outcome,ValueError);imported.assert_not_called();guard.assert_not_called()
            self.assertFalse((job/'scratch/graph-result.json').exists())
        for mutate in [lambda a:a.update(capture_nonce=str(uuid.uuid4())),lambda a:a['request_identity'].update(sha256='a'*64),
                       lambda a:a.update(schema_version=True),lambda a:a.update(extra=True)]:
            outcome,_,_,imported,guard,_=self.execute_fake(bad_assignment=mutate)
            self.assertIsInstance(outcome,ValueError);imported.assert_not_called();guard.assert_not_called()
        outcome,phases,_,imported,_,_=self.execute_fake(guard_error=True)
        self.assertIsInstance(outcome,ValueError);self.assertEqual(phases,['versions','metadata']);imported.assert_not_called()

    def test_failed_operation_or_existing_output_never_emits_after(self):
        for flag in ('operation_error','clobber'):
            outcome,phases,job,_,_,_=self.execute_fake(**{flag:True})
            self.assertIsInstance(outcome,RuntimeError if flag=='operation_error' else FileExistsError)
            self.assertEqual(phases[-1],'operation')
            self.assertFalse((job/'scratch/engine-time-3.json').exists())
            if flag=='clobber':self.assertEqual((job/'scratch/graph-result.json').read_bytes(),b'preserve')


class CanonicalReceiptTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup);self.artifacts=Path(self.temp.name)
        self.campaign=str(uuid.uuid4());self.native,self.interpreter=successful_graph(self.campaign,self.artifacts)

    def test_success_requires_host_bindings_and_parallel_assertions_without_relaxing_old_receipts(self):
        receipts.accept(self.native,self.campaign,self.interpreter,self.artifacts)
        changes=[lambda n:n.update(campaign_id=str(uuid.uuid4())),lambda n:n.update(job_id=str(uuid.uuid4())),
                 lambda n:n['canonical_capture'].update(nonce=str(uuid.uuid4())),lambda n:n['canonical_capture'].update(workspace_revision=True),
                 lambda n:n['result']['worker']['checks'].update(campaign_id=str(uuid.uuid4())),
                 lambda n:n['result']['canonical']['hop_assertion_ids'][0].pop(),
                 lambda n:n['result']['canonical'].update(canonical_unchanged=False),
                 lambda n:n['assigned_files']['code/graph_path.py'].update(sha256='a'*64),
                 lambda n:n.update(termination_state='unconfirmed'),lambda n:n.update(extra='arbitrary'),
                 lambda n:n['result']['worker']['checks'].update(backend_metadata_checked=False)]
        for change in changes:
            bad=copy.deepcopy(self.native);change(bad)
            with self.assertRaises(common.ProbeFailure):receipts.accept(bad,self.campaign,self.interpreter,self.artifacts)
        with self.assertRaises(common.ProbeFailure):common.failure_summary(self.native,self.campaign)

    def test_raw_duplicate_altered_and_oversize_retained_files_cannot_pass(self):
        path=self.artifacts/'captured-result.json';original=path.read_bytes()
        for raw in (original+b' ',b' '*131073,original.replace(b'"schema_version": 1',b'"schema_version": 1,"schema_version": 1')):
            path.write_bytes(raw)
            with self.assertRaises((common.ProbeFailure,ValueError)):receipts.accept(self.native,self.campaign,self.interpreter,self.artifacts)
        path.write_bytes(original)
        # Even internally consistent hashes cannot turn a substituted path into the fixed expected response.
        raw=original.replace(b'"b"',b'"d"');path.write_bytes(raw)
        self.native['graph_output_identity']=identity(raw)
        self.native['result']['worker']['checks']['result_identity']=identity(raw)
        with self.assertRaises((common.ProbeFailure,ValueError)):receipts.accept(self.native,self.campaign,self.interpreter,self.artifacts)

    def test_failure_summary_preserves_safe_capture_but_not_unchecked_result(self):
        self.native.update(passed=False,failure='quota-exhausted',quota_kind='wall-time',result={'private':'not retained'})
        value=receipts.summary(self.native,self.campaign)
        self.assertNotIn('result',value);self.assertEqual(value['canonical_capture'],self.native['canonical_capture'])
        self.assertEqual(value['failure'],'quota-exhausted')


@unittest.skipUnless(os.name=='posix','POSIX-only runner fixtures')
class CanonicalRunnerTests(unittest.TestCase):
    setUp=legacy.ProbeRunnerTests.setUp
    patches=engine_tests.EngineRunnerTests.patches
    def run_case(self, termination='confirmed', timeout=False, failed=False):
        verify=self.patches()
        def relocate(prefix,destination):
            destination.mkdir();(destination/'manifest.json').write_bytes((prefix/'manifest.json').read_bytes())
            return {'copied':True}
        def run(command,path,seconds,environment):
            self.assertEqual(seconds,120);self.assertIn(receipts.TEST,command)
            self.assertEqual(Path(environment['WORKBENCH_TEST_PYTHON_PREFIX']),self.artifacts/'moved-prefix')
            native,_=successful_graph(environment['WORKBENCH_TEST_PYTHON_CAMPAIGN'],self.artifacts)
            native.update(termination_state=termination)
            if failed:native.update(passed=False,failure='quota-exhausted',quota_kind='wall-time',result=None)
            (self.artifacts/'native-report.json').write_text(json.dumps(native))
            if timeout:raise subprocess.TimeoutExpired('fixed',120)
            return SimpleNamespace(returncode=1 if failed else 0)
        with patch.object(runner.relocation,'relocate',side_effect=relocate), \
             patch.object(common,'run_logged',side_effect=run) as launch, \
             patch.object(receipts,'accept',wraps=receipts.accept) as accept:
            result=runner.observe(self.prefix,self.artifacts,'canonical-graph')
        launch.assert_called_once()
        return result,verify,accept

    def test_fresh_relocated_success_validates_after_both_integrity_checks(self):
        result,verify,accept=self.run_case()
        self.assertTrue(result['passed']);self.assertFalse(result['complete_release']);self.assertEqual(verify.call_count,3)
        accept.assert_called_once();self.assertEqual(result['native']['result']['canonical']['hop_assertion_ids'][0],['r1','r1-parallel'])

    def test_unconfirmed_termination_and_outer_timeout_never_read_outputs_or_post_verify(self):
        for timeout in (False,True):
            with self.subTest(timeout=timeout):
                self.setUp()
                result,verify,accept=self.run_case(termination='unconfirmed',timeout=timeout,failed=True)
                self.assertFalse(result['passed']);self.assertEqual(result['termination_state'],'unverified')
                verify.assert_called_once();accept.assert_not_called();self.assertNotIn('post_selected_inventory',result)

    def test_confirmed_failure_is_retained_and_does_not_retry(self):
        result,verify,_=self.run_case(failed=True)
        self.assertFalse(result['passed']);self.assertEqual(result['native']['failure'],'quota-exhausted')
        self.assertEqual(verify.call_count,3);self.assertEqual(result['termination_state'],'confirmed')


if __name__=='__main__':unittest.main()
