import copy
from collections import defaultdict
import hashlib
import json
import math
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools/diagnostics'))

from test_rust_execution_contact_audit import sample as execution_sample
from test_rust_intent_plan_audit import database
from test_forecast_contact_geometry import csv_rows
from batting_intent import frozen_planner
from intent_execution_contract import evaluate, motion_for
from swing_execution import SwingPlan
from rust_execution_contact_audit import compare_execution
from rotating_bat_probe import norm
import rust_intent_collision_probe as probe


def sample(seq=1, strategy='let_batter_decide', preference='contact_first', clock_invalid=False):
    state = execution_sample(seq,strategy,preference,clock_invalid)
    f = state['observed_intent_plans']['estimate']['forecast']
    state['input'].update(reference_time_sec_since_release=f['time_sec'],ball_position_world_m=f['position_world_m'])
    if not clock_invalid:
        for p,r in zip(state['observed_intent_plans']['intent_plans']['plans'],state['execution_contact']['probes']):
            samples,result = evaluate(state['input'],SwingPlan.from_saved(p['plan']['forecast_plan']))
            r.update(samples=samples,search=result)
    return state


def database_with_spin(path,states,spin=(2200.,0.,.9)):
    database(path,states)
    db = sqlite3.connect(path)
    db.execute('CREATE TABLE player_game_pitching(game_id,count_seq,spin_rate,spin_angle,spin_efficiency)')
    db.executemany('INSERT INTO player_game_pitching VALUES(1,?,?,?,?)',[(seq,*spin) for seq,_,_ in states])
    db.commit(); db.close()


class IntentCollisionTest(unittest.TestCase):
    def audited(self,state):
        return compare_execution(state,1,1,frozen_planner(),defaultdict(float))[0]

    def test_bridge_uses_contact_time_pulse_and_fixed_geometry(self):
        state = sample(); before = copy.deepcopy(state)
        for r in self.audited(state):
            prepared,reason = probe.prepare_contact(state,r)
            self.assertIsNone(reason)
            self.assertEqual(prepared['geometry'].region,'side')
            saved = next(p for p in state['observed_intent_plans']['intent_plans']['plans'] if p['intent']==r['intent'])
            plan = SwingPlan.from_saved(saved['plan']['forecast_plan'])
            phase = state['input']['reference_time_sec_since_release']+r['time_offset_sec']-plan.nominal_time_sec
            u = (phase+plan.anchor_fraction*plan.duration_sec)/plan.duration_sec
            gain = math.sin(math.pi*u)**2
            for actual,peak in zip(prepared['omega'],plan.peak_omega_world_rad_s):
                self.assertAlmostEqual(actual,peak*gain,places=12)
            self.assertLess(norm(prepared['omega']),norm(plan.peak_omega_world_rad_s))
            self.assertAlmostEqual(prepared['source_z_m'],.71+prepared['geometry'].closest_axial_m)
            checks = defaultdict(float)
            result = probe.response(prepared,(0.,0.,0.),checks)
            self.assertEqual(result['collision_status'],'impulse_contact')
            self.assertLess(max(checks.values()),1e-7)
            self.assertGreaterEqual(result['total_loss_j'],0.)
            self.assertLessEqual(result['system_after_j'],result['system_before_j']+1e-7)
        self.assertEqual(state,before)

    def test_eligibility_keeps_caps_curve_exclusions_and_nonapproach_explicit(self):
        state = sample(); row = self.audited(state)[0]
        for mutate,reason in [
            (lambda g:g.update(axial_m=.20,closest_axial_m=.16,contact_region='cap'), 'unsupported_cap'),
            (lambda g:g.update(axial_m=.15,closest_axial_m=.15), 'out_of_curve'),
            (lambda g:g.update(bat_velocity=[0.,-1000.,0.]), 'non_approaching')]:
            changed = copy.deepcopy(state)
            g = changed['execution_contact']['probes'][0]['search']['geometry']
            mutate(g)
            prepared,error = probe.prepare_contact(changed,row)
            self.assertIsNone(prepared); self.assertEqual(error,'not_evaluated:'+reason)
        prepared,error = probe.prepare_contact(state,dict(row,status='unresolved'))
        self.assertIsNone(prepared); self.assertEqual(error,'not_evaluated:unresolved')

    def test_all_fair_denominators_and_paired_common_cohort(self):
        aggregate = probe.Aggregate()
        def row(intent,geometry,fair,vla,selected):
            status = 'contact' if geometry else 'no_contact'
            result = dict(collision_status='impulse_contact' if geometry else 'not_evaluated:no_contact',
                          ev_ms=40.,vla_deg=vla,hla_deg=0. if fair else 90.,spin_rpm=2000.)
            return dict(intent=intent,selected=selected,strategy='let_batter_decide',preferred_intent='contact_first',
                forecast_height_band_m='.9..<1.2',status=status,**result,**probe.flags(dict(status=status),result))
        aggregate.add([row('contact_first',True,True,-1.,True),row('line_drive',True,True,20.,False)])
        aggregate.add([row('contact_first',False,False,0.,True),row('line_drive',True,False,60.,False)])
        summary = next(r for r in aggregate.summaries() if r['scope']=='all_plans' and r['stratum']=='overall' and r['intent']=='contact_first')
        self.assertEqual(summary['geometry_pct_per_searched'],50.)
        self.assertEqual(summary['downward_pct_among_fair'],100.)
        self.assertEqual(summary['downward_pct_per_searched'],50.)
        pair = next(r for r in aggregate.paired_rows() if r['stratum']=='overall')
        self.assertEqual(pair['geometry_gained'],1); self.assertEqual(pair['geometry_lost'],0)
        self.assertEqual(pair['geometry_net_events_per_pitch_pp'],50.)
        self.assertEqual(pair['common_fair'],1)
        self.assertEqual(pair['common_fair_downward_delta_pp'],-100.)
        self.assertEqual(pair['common_fair_mean_delta_vla_deg'],21.)

    def test_readonly_pipeline_selection_status_and_cli_environment(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); db=root/'test with space.db'
            old=sample(seq=3);old.pop('execution_contact')
            database_with_spin(db,[(1,sample(strategy='seek_line_drive'),None),
                (2,sample(seq=2,clock_invalid=True),None),(3,old,None)])
            before=hashlib.sha256(db.read_bytes()).hexdigest()
            result=probe.run(db,1,1,root/'out')
            self.assertEqual(result['matched_states'],2)
            self.assertEqual(result['matched_plans'],4)
            self.assertEqual(result['response_events'],2)
            self.assertEqual(result['solver_calls'],4)
            self.assertEqual(result['replayed_searches'],2)
            self.assertEqual(result['not_comparable:missing_execution_contact_states'],1)
            self.assertEqual(before,hashlib.sha256(db.read_bytes()).hexdigest())
            details=csv_rows(root/'out/rust_intent_collision_details.csv')
            self.assertEqual([r['intent'] for r in details if r['count_seq']=='1' and r['selected']=='True'],['line_drive'])
            self.assertEqual(sum(r['collision_status']=='not_evaluated:execution_starts_before_plan' for r in details),2)
            metadata=json.loads((root/'out/rust_intent_collision_metadata.json').read_text())
            self.assertEqual(metadata['model'],probe.MODEL)
            self.assertEqual(metadata['incoming_spin']['gyro_sign'],0)
            command=[sys.executable,str(ROOT/'tools/diagnostics/rust_intent_collision_probe.py'),
                '--first-game-id','1','--last-game-id','1','--output-dir',str(root/'cli')]
            cli=subprocess.run(command,env=dict(os.environ,SABERBB_DB=str(db)),capture_output=True,text=True)
            self.assertEqual(cli.returncode,0,cli.stderr)
            self.assertEqual(csv_rows(root/'out/rust_intent_collision_summary.csv'),csv_rows(root/'cli/rust_intent_collision_summary.csv'))
            missing=subprocess.run(command,env=dict(os.environ,SABERBB_DB=''),capture_output=True,text=True)
            self.assertNotEqual(missing.returncode,0); self.assertIn('SABERBB_DB is not set',missing.stderr)
            with self.assertRaises(FileExistsError):probe.run(db,1,1,root/'out')

    def test_corrupt_saved_contact_is_rejected_before_collision(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);state=sample()
            state['execution_contact']['probes'][0]['search']['geometry']['normal']=[1.,0.,0.]
            database_with_spin(root/'db',[(1,state,None)])
            with patch.object(probe,'response',side_effect=AssertionError('collision must wait for audit')):
                with self.assertRaises(ValueError):probe.run(root/'db',1,1,root/'out')
            self.assertFalse((root/'out').exists())

    def test_bad_spin_never_falls_back_and_missing_duplicate_keys_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); db=root/'db';database_with_spin(db,[(1,sample(),None)],spin=(2200.,0.,1.1))
            with self.assertRaisesRegex(ValueError,'spin_efficiency_out_of_range'):
                probe.run(db,1,1,root/'error')
            self.assertFalse((root/'error').exists())
            result=probe.run(db,1,1,root/'exclude',invalid_spin_policy='exclude')
            self.assertEqual(result['invalid_spin_pitches'],1)
            self.assertEqual(result['collision_status:not_evaluated:invalid_pitch_spin'],2)
            self.assertEqual(result['response_events'],0)
            summary=csv_rows(root/'exclude/rust_intent_collision_summary.csv')
            self.assertTrue(all(r['mean_fair_ev_ms']=='' for r in summary))
            self.assertEqual(len(csv_rows(root/'exclude/rust_intent_collision_invalid_spin.csv')),1)
            for mode in ('missing','duplicate'):
                db2=root/mode;database_with_spin(db2,[(1,sample(),None)])
                con=sqlite3.connect(db2)
                if mode=='missing':con.execute('DELETE FROM player_game_pitching')
                else:con.execute('INSERT INTO player_game_pitching SELECT * FROM player_game_pitching')
                con.commit();con.close()
                with self.assertRaisesRegex(ValueError,'key_missing_or_duplicate'):
                    probe.run(db2,1,1,root/(mode+'_out'))


if __name__=='__main__':unittest.main()
