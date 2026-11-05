import copy
from collections import defaultdict
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools/diagnostics'))

from test_rust_intent_collision_probe import sample,database_with_spin
from test_forecast_contact_geometry import csv_rows
from batting_intent import frozen_planner
from rust_execution_contact_audit import compare_execution
import finite_collision_contract
import rust_collision_response_audit as audit

FIXTURE=ROOT/'tests/fixtures/finite_collision_contract.json'


def recorded(seq=1,strategy='let_batter_decide',preference='contact_first',clock_invalid=False,spin=(2200.,0.,.9)):
    state=sample(seq,strategy,preference,clock_invalid)
    rows,error=compare_execution(state,1,seq,frozen_planner(),defaultdict(float));assert error is None
    state['intent_collision']=audit.reference_diagnostics(state,rows,spin,defaultdict(float))
    return state


class RustCollisionResponseTest(unittest.TestCase):
    def test_fixture_matches_current_frozen_python_reference(self):
        self.assertEqual(json.loads(FIXTURE.read_text()),json.loads(json.dumps(finite_collision_contract.cases())))

    def test_selected_intent_does_not_change_plans_or_read_results(self):
        for strategy,preference,expected in [('let_batter_decide','line_drive','line_drive'),
            ('put_ball_in_play','line_drive','contact_first'),('seek_line_drive','contact_first','line_drive')]:
            state=recorded(strategy=strategy,preference=preference);before=copy.deepcopy(state)
            rows,error=audit.compare_collision(state,1,1,(2200.,0.,.9),frozen_planner(),defaultdict(float))
            self.assertIsNone(error);self.assertEqual([r['intent'] for r in rows if r['selected']],[expected])
            self.assertEqual(state,before)
            self.assertTrue(all(r['successful'] for r in rows))

    def test_corrupt_response_control_input_and_selection_are_rejected(self):
        for mutate in [lambda p:p.update(selected=not p['selected']),
            lambda p:p['evaluation']['response'].update(outgoing_world_ms=[99,99,99]),
            lambda p:p['evaluation']['control'].update(spin_rpm=0.),
            lambda p:p['evaluation']['input']['parameters'].update(mu=.3),
            lambda p:p.update(invalid_reason='ignored_error')]:
            state=recorded();mutate(state['intent_collision']['probes'][0])
            with self.assertRaises(ValueError):
                audit.compare_collision(state,1,1,(2200.,0.,.9),frozen_planner(),defaultdict(float))
        state=recorded()
        with self.assertRaises(ValueError):audit.compare_collision(state,1,1,(2300.,0.,.9),frozen_planner(),defaultdict(float))

    def test_invalid_spin_and_clock_are_explicit_without_fallback(self):
        for clock,spin,status in [(True,(2200.,0.,.9),'not_evaluated:execution_starts_before_plan'),
            (False,(2200.,0.,1.1),'not_evaluated:invalid_pitch_spin')]:
            state=recorded(clock_invalid=clock,spin=spin)
            rows,error=audit.compare_collision(state,1,1,spin,frozen_planner(),defaultdict(float))
            self.assertIsNone(error);self.assertTrue(all(r['collision_status']==status for r in rows))
            for p in state['intent_collision']['probes']:
                self.assertIsNone(p['evaluation']['input']);self.assertIsNone(p['evaluation']['control'])
            state['intent_collision']['probes'][0]['evaluation']=recorded()['intent_collision']['probes'][0]['evaluation']
            with self.assertRaises(ValueError):audit.compare_collision(state,1,1,spin,frozen_planner(),defaultdict(float))

    def test_readonly_pipeline_old_rows_and_environment_cli(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);db=root/'test data.db'
            database_with_spin(db,[(1,recorded(),None),(2,sample(seq=2),None),
                (3,recorded(seq=3,clock_invalid=True),None)])
            before=hashlib.sha256(db.read_bytes()).hexdigest()
            result=audit.run(db,1,1,root/'out')
            self.assertEqual(result['matched_states'],2);self.assertEqual(result['matched_plans'],4)
            self.assertEqual(result['matched_responses'],2);self.assertEqual(result['missing_intent_collision_states'],1)
            self.assertEqual(before,hashlib.sha256(db.read_bytes()).hexdigest())
            self.assertLess(max(float(r['max_error']) for r in csv_rows(root/'out/rust_collision_response_checks.csv')),1e-8)
            cmd=[sys.executable,str(ROOT/'tools/diagnostics/rust_collision_response_audit.py'),
                '--first-game-id','1','--last-game-id','1','--output-dir',str(root/'cli')]
            p=subprocess.run(cmd,env=dict(os.environ,SABERBB_DB=str(db)),capture_output=True,text=True)
            self.assertEqual(p.returncode,0,p.stderr)
            p=subprocess.run(cmd,env=dict(os.environ,SABERBB_DB=''),capture_output=True,text=True)
            self.assertNotEqual(p.returncode,0);self.assertIn('SABERBB_DB is not set',p.stderr)
            with self.assertRaises(FileExistsError):audit.run(db,1,1,root/'out')


if __name__=='__main__':unittest.main()
