"""Host-only campaign acceptance regressions. No interpreter prefix or child execution."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0,str(SCRIPTS))
import python_graph_coordinator_receipts as receipt
spec=importlib.util.spec_from_file_location('graph_campaign_runner',SCRIPTS/'test_graph_coordinator_native.py')
runner=importlib.util.module_from_spec(spec);spec.loader.exec_module(runner)

CAMPAIGN='6441fd17-abc9-43e0-9d1a-1a3c75c8a432'

def observer():
    return {'execution_calls':1,'launch_count':1,'pid':123,'launch_ms':100,'live_observed_ms':101,
            'cancellation_seen_ms':None,'stop_ms':200,'termination':'confirmed','cleanup':'confirmed',
            'profile_sha256':None,'assigned_files':None,'pre_inventory_verified':True,'post_inventory_verified':True,'assignment_id':CAMPAIGN,
            'capture_nonce':CAMPAIGN,'request_identity':receipt.identity(b'{}'),
            'wrapper_identity':receipt.identity(b'{}'),'result_identity':receipt.identity(b'{}')}

def envelope():
    return {'schema_version':1,'campaign_id':CAMPAIGN,'case':'ordinary_retry','passed':True,'failure':None,
            'observation':observer(),'result':{},'complete_release':False}

def snapshot():
    return {'tables':{'schema':[['table','records','records','fixture']], 'version':[[5]],'meta':[[1,2]],
        'records':[[1,'entity','a','{}'],[2,'processing_job','job','queued']], 'history':[],
        'events':[[1,1,'import','time'],[2,2,'queue','time']], 'derivative_objects':[],
        'sqlite_sequence':[['events',2],['records',2]]}, 'originals':{'a'*64:[1,'b'*64]}}

class CoordinatorCampaignTests(unittest.TestCase):
    def test_unknown_termination_and_cleanup_failure_gate_every_post_read(self):
        for key,value in [('termination','unverified'),('termination','unconfirmed'),('cleanup','failed')]:
            item=envelope();item['observation'][key]=value
            with patch.object(receipt.common,'read_json',side_effect=AssertionError('read')),patch.object(receipt,'raw',side_effect=AssertionError('raw')):
                with self.assertRaises(receipt.common.ProbeFailure): receipt.accept(item,CAMPAIGN,'ordinary_retry',Path('/unused'))

    def test_exact_campaign_and_observer_shape_reject_substitution(self):
        item=envelope();self.assertTrue(receipt.summary(item,CAMPAIGN,'ordinary_retry'))
        for field,value in [('campaign_id','another'),('case','unreachable'),('schema_version',True),('complete_release',True)]:
            changed=copy.deepcopy(item);changed[field]=value
            with self.assertRaises(receipt.common.ProbeFailure): receipt.summary(changed,CAMPAIGN,'ordinary_retry')
        item['observation']['extra']=True
        with self.assertRaises(receipt.common.ProbeFailure): receipt.summary(item,CAMPAIGN,'ordinary_retry')

    def test_complete_table_delta_and_autoincrement_update_gaps(self):
        before=snapshot();after=copy.deepcopy(before)
        after['tables']['records'][1][3]='completed'
        after['tables']['records'].append([4,'graph_analysis','result','{}'])
        after['tables']['history']=[[1,'processing_job','job','queued',3]]
        after['tables']['events'].append([3,3,'processing.graph_finish','time'])
        after['tables']['meta']=[[1,3]]
        after['tables']['sqlite_sequence']=[['events',3],['history',1],['records',4]]
        receipt.canonical_delta(before,after,[['processing_job','job']],'result')
        for table in receipt.TABLES:
            changed=copy.deepcopy(after)
            if table=='meta': changed['tables'][table]=[[1,99]]
            elif not changed['tables'][table]: changed['tables'][table]=[['corrupted']]
            else: changed['tables'][table][0][-1]='corrupted'
            with self.assertRaises((receipt.common.ProbeFailure,TypeError,IndexError)):
                receipt.canonical_delta(before,changed,[['processing_job','job']],'result')

    def test_detached_allowed_job_and_inserted_record_bodies_are_rejected(self):
        before=snapshot();after=copy.deepcopy(before)
        queued={'id':'job','state':'queued'};finished={'id':'job','state':'completed'};record={'id':'result','value':'frozen'}
        before['tables']['records'][1][3]=json.dumps(queued)
        after['tables']['records'][1][3]=json.dumps(finished)
        after['tables']['records'].append([4,'graph_analysis','result',json.dumps(record)])
        result={'case':'unreachable','queued':queued,'finished':finished,'record_id':'result'}
        receipt.bind_canonical(before,after,result,record)
        for row_index in (1,2):
            changed=copy.deepcopy(after)
            body=json.loads(changed['tables']['records'][row_index][3]);body['substitution']=True
            changed['tables']['records'][row_index][3]=json.dumps(body)
            with self.assertRaises(receipt.common.ProbeFailure): receipt.bind_canonical(before,changed,result,record)
        for mutation in ('duplicate','wrong-key'):
            changed=copy.deepcopy(after)
            if mutation=='duplicate':changed['tables']['records'].append(changed['tables']['records'][1])
            else:changed['tables']['records'][1][3]=json.dumps({'id':'other','state':'completed'})
            with self.assertRaises(receipt.common.ProbeFailure):receipt.bind_canonical(before,changed,result,record)

    def test_snapshot_identity_binds_bodies_but_not_dictionary_insertion_order(self):
        original=snapshot();changed=copy.deepcopy(original)
        changed['tables']=dict(reversed(list(changed['tables'].items())))
        self.assertEqual(receipt.snapshot_identity(original),receipt.snapshot_identity(changed))
        changed['tables']['records'][0][3]='altered'
        self.assertNotEqual(receipt.snapshot_identity(original),receipt.snapshot_identity(changed))
        self.assertFalse(receipt.strict_equal({'schema_version':1},{'schema_version':True}))

    def test_raw_result_is_byte_bound_not_merely_json_equivalent(self):
        with tempfile.TemporaryDirectory() as root:
            path=Path(root)/'result.json';path.write_bytes(b'{ "x":1}')
            with self.assertRaises(receipt.common.ProbeFailure): receipt.raw(path,100,receipt.identity(b'{"x":1}'))

    # These two runner controls use the real POSIX-only private receipt writer.
    # Pure receipt validation above remains portable; no Windows ACL claim is made.
    @unittest.skipUnless(os.name == 'posix', 'Mac runner receipt persistence requires POSIX file modes')
    def test_source_metadata_failure_retains_initial_failed_report_without_execution(self):
        with tempfile.TemporaryDirectory() as root, patch.object(runner,'source_identity',side_effect=OSError('fixture')),patch.object(runner,'build',side_effect=AssertionError('build')):
            report=runner.run(Path('/unused'),Path(root))
            saved=json.loads((Path(root)/'observation.json').read_text())
            self.assertFalse(report['passed']);self.assertFalse(saved['candidate_started']);self.assertEqual(saved['cases'],[])
            self.assertEqual(saved['failure'],'campaign-input-or-receipt-unavailable')
            self.assertEqual((Path(root)/'observation.json').stat().st_mode & 0o777, 0o600)

    @unittest.skipUnless(os.name == 'posix', 'Mac runner receipt persistence requires POSIX file modes')
    def test_outer_native_timeout_does_not_read_receipts_or_prefix_afterward(self):
        import subprocess
        with tempfile.TemporaryDirectory() as root:
            directory=Path(root);binary=directory/'fake-test';binary.write_bytes(b'host-test')
            with patch.object(runner,'source_identity',return_value={'commit':'fixture'}),patch.object(runner.platform,'system',return_value='Darwin'),patch.object(runner.platform,'machine',return_value='arm64'),patch.object(runner.common.installed,'verify',return_value={'verified':True}) as inventory,patch.object(runner.relocation,'relocate',return_value={'copied':True}),patch.object(runner,'build',return_value=binary),patch.object(runner.common,'run_logged',side_effect=subprocess.TimeoutExpired('fixed',900)),patch.object(runner.common,'read_json',side_effect=AssertionError('post read')):
                report=runner.run(Path('/unused'),directory)
                self.assertEqual(report['termination'],'unverified');self.assertEqual(inventory.call_count,1)
                self.assertEqual(report['failure'],'native-termination-unverified')

if __name__=='__main__':unittest.main()
