import copy
import hashlib
import json
import math
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_bat_pivot_orientation_probe import source_db
from test_contact_state_probe import state
from test_forecast_contact_geometry import csv_rows
from test_swing_prediction_probe import prediction
from swing_prediction_probe import plan_swing
from swing_execution import SwingExecution,ExecutionMotion
from rotating_bat_probe import add,mul
import swing_execution_probe as probe


class SwingExecutionTest(unittest.TestCase):
    def test_pulse_integral_derivative_boundaries_and_state_validation(self):
        s=SwingExecution(1,2,.4)
        self.assertAlmostEqual(s.start_time_sec,.31)
        self.assertAlmostEqual(s.gain(0),1.)
        for q in (-.15,-.09,-.04,0,.04,.09,.15):
            h=1e-7;derivative=(s.integral(q+h)-s.integral(q-h))/(2*h)
            self.assertAlmostEqual(derivative,s.gain(q),places=7)
        self.assertEqual(s.gain(-.1),0);self.assertEqual(s.gain(.1),0)
        self.assertAlmostEqual(s.integral(.2),s.duration_sec/2)
        for kw in [dict(effort=0),dict(effort=1.1),dict(duration_sec=-1),dict(anchor_fraction=1),dict(reference_time_sec=.01)]:
            args=dict(game_id=1,count_seq=1,reference_time_sec=.4);args.update(kw)
            with self.assertRaises(ValueError):SwingExecution(**args)

    def test_pose_derivative_matches_point_velocity_and_global_bound(self):
        for side in (-1.,1.):
            p=prediction()['input'];p['handedness_sign']=side
            plan=plan_swing(p,.9,.16);before=copy.deepcopy(plan)
            for fraction in (.35,.5,.65):
                e=SwingExecution(1,2,plan['nominal_time_sec'],.85,.18,fraction,.004)
                m=ExecutionMotion(state(),plan,e);delay=.007
                for t in (-.15,-.05,0,.03,.15):
                    for axial in (-.16,0,.16):
                        h=1e-7
                        def point(t):
                            center,a=m.pose(t,delay);return add(center,mul(a,axial))
                        velocity=mul(probe.sub(point(t+h),point(t-h)),1/(2*h))
                        expected=m.velocity_at_phase(t,delay,axial)
                        self.assertLess(probe.norm(probe.sub(velocity,expected)),2e-7)
                        relative=probe.norm(probe.sub(m.ball_velocity,expected))
                        self.assertLessEqual(relative,m.speed_bound+1e-10)
                self.assertEqual(plan,before)

    def test_effort_scales_nominal_speed_and_delay_moves_pose_once(self):
        plan=plan_swing(prediction()['input'],.9,.16);s=state()
        fast=ExecutionMotion(s,plan,SwingExecution(1,2,plan['nominal_time_sec']))
        slow=ExecutionMotion(s,plan,SwingExecution(1,2,plan['nominal_time_sec'],effort=.85))
        for x,y in zip(fast.pose(0,0)[0],slow.pose(0,0)[0]):self.assertAlmostEqual(x,y)
        for x,y in zip(fast.velocity_at_phase(0,0),slow.velocity_at_phase(0,0)):
            self.assertAlmostEqual(.85*x,y)
        delayed=ExecutionMotion(s,plan,SwingExecution(1,2,plan['nominal_time_sec'],start_offset_sec=.004))
        for t in (-.02,0,.03):
            for x,y in zip(delayed.pose(t,.005),fast.pose(t-.004,.005)):
                for a,b in zip(x,y):self.assertAlmostEqual(a,b)
        self.assertNotEqual(delayed.pose(0,0),fast.pose(0,0))

    def test_long_pulse_converges_to_constant_motion(self):
        from bat_pivot_orientation_probe import motion_for
        plan=plan_swing(prediction()['input'],.9,.16);plan['nominal_time_sec']=100.
        baseline=motion_for(state(),plan)
        profiled=ExecutionMotion(state(),plan,SwingExecution(1,2,100.,duration_sec=100.))
        for t in (-.01,0,.01):
            for a,b in zip(baseline.pose(t,0),profiled.pose(t,0)):
                self.assertLess(probe.norm(probe.sub(a,b)),2e-7)
            self.assertLess(probe.norm(probe.sub(baseline.evaluate(t,0)['bat_velocity'],
                profiled.evaluate(t,0)['bat_velocity'])),1e-5)

    def test_saved_control_one_search_per_scenario_readonly_and_cohorts(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,path=source_db(root)
            digest=hashlib.sha256(path.read_bytes()).hexdigest()
            from bat_pivot_orientation_probe import search
            with patch('bat_pivot_orientation_probe.search',wraps=search) as calls:
                c=probe.run(source,path,root/'out')
            self.assertEqual(c['source_selected_trials'],2);self.assertEqual(calls.call_count,4)
            self.assertEqual(digest,hashlib.sha256(path.read_bytes()).hexdigest())
            rows=csv_rows(root/'out'/'swing_execution_summary.csv')
            for scenario in {r['scenario'] for r in rows}:
                overall=[r for r in rows if r['scenario']==scenario and r['scope']=='all_swings' and r['stratum']=='overall']
                self.assertEqual(len(overall),6);self.assertTrue(all(r['trials']=='2' for r in overall))
            checks=csv_rows(root/'out'/'swing_execution_checks.csv')
            self.assertLess(max(float(r['max_error']) for r in checks),1e-7)
            states=[json.loads(x) for x in (root/'out'/'swing_execution_states.jsonl').read_text().splitlines()]
            self.assertEqual(len(states),4)
            # Collision rows must record instantaneous angular velocity, not peak omega.
            for r in states:
                if r['instantaneous_entry_omega_world_rad_s'] is not None:
                    self.assertLessEqual(probe.norm(r['instantaneous_entry_omega_world_rad_s']),
                        r['execution']['effort']*probe.norm(r['base_omega_world_rad_s'])+1e-10)
                    for actual,base in zip(r['instantaneous_entry_omega_world_rad_s'],r['base_omega_world_rad_s']):
                        self.assertAlmostEqual(actual,base*r['execution']['effort']*r['entry_gain'])
            with self.assertRaises(FileExistsError):probe.run(source,path,root/'out')

    def test_invalid_scenario_fails_before_writing(self):
        with tempfile.TemporaryDirectory() as tmp:
            out=Path(tmp)/'out'
            for kw in [dict(efforts=[]),dict(efforts=(1,1)),dict(duration=-1),dict(fractions=(1.,)),dict(efforts=(1.1,))]:
                with self.assertRaises(ValueError):probe.run('missing','missing',out,**kw)
                self.assertFalse(out.exists())

    def test_execution_before_plan_is_unevaluated_and_kept_in_denominator(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,path=source_db(root)
            with patch('bat_pivot_orientation_probe.search',side_effect=AssertionError('acausal search')):
                counts=probe.run(source,path,root/'out',duration=.32)
            self.assertEqual(counts['new_searches'],0)
            rows=csv_rows(root/'out'/'swing_execution_summary.csv')
            execution=[r for r in rows if r['orientation']=='execution']
            self.assertTrue(execution)
            self.assertTrue(all(r['geometry_contacts']=='0' and int(r['trials'])>0 for r in execution))
            self.assertTrue(all(r['evaluable_trials']=='0' and r['contact_fraction_of_evaluable_trials']=='' for r in rows))
            transitions=csv_rows(root/'out'/'swing_execution_transitions.csv')
            self.assertTrue(all(r['after_search_status']=='execution_starts_before_plan' for r in transitions))

    def test_partial_clock_exclusion_uses_same_contact_rate_denominator(self):
        from test_speed_axial_audit import contact
        from bat_pivot_orientation_probe import Aggregate,MODEL_NAMES
        a=Aggregate('saved_constant','execution');plan=plan_swing(prediction()['input'],.9,.16)
        r=contact();plans=dict(saved_constant=plan,execution=plan)
        unavailable={m:dict(collision_status='not_evaluated:out_of_curve',ev_ms=None) for m in MODEL_NAMES}
        a.add(r,plans,dict(saved_constant=r,execution=r),dict(saved_constant=unavailable,execution=unavailable))
        invalid=dict(r,status='execution_starts_before_plan')
        missing={m:dict(collision_status='not_evaluated:execution_starts_before_plan',ev_ms=None) for m in MODEL_NAMES}
        a.add(r,plans,dict(saved_constant=r,execution=invalid),dict(saved_constant=unavailable,execution=missing))
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);probe.write_aggregates(root,{'test':a})
            rows=csv_rows(root/'swing_execution_summary.csv')
            self.assertTrue(all(r['evaluable_trials']=='1' and r['evaluable_geometry_contacts']=='1'
                and r['contact_fraction_of_evaluable_trials']=='1.0' for r in rows))


if __name__=='__main__':unittest.main()
