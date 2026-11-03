import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_contact_state_probe import state
from test_swing_prediction_probe import prediction
from test_bat_pivot_orientation_probe import source_db
from test_forecast_contact_geometry import csv_rows
from swing_prediction_probe import plan_swing
from swing_execution import SwingPlan,ExecutionDeviation,PlanExecutionMotion,SwingExecution,ExecutionMotion
from rotating_bat_probe import add,sub,mul,norm
import swing_execution_probe as probe


class SwingPlanExecutionTest(unittest.TestCase):
    def test_planned_short_reaches_target_but_shortfall_keeps_full_start(self):
        saved=plan_swing(prediction()['input'],.9,.16)
        full=SwingPlan.from_saved(saved);short=SwingPlan.from_saved(saved,.85)
        a=PlanExecutionMotion(state(),full,ExecutionDeviation())
        b=PlanExecutionMotion(state(),short,ExecutionDeviation())
        c=PlanExecutionMotion(state(),full,ExecutionDeviation(.85))
        self.assertGreater(norm(sub(full.start_point_world_m,short.start_point_world_m)),.05)
        for m in (a,b):self.assertLess(norm(sub(m.pose(0,0)[0],saved['actual_point_world_m'])),1e-12)
        start=-full.anchor_fraction*full.duration_sec
        for x,y in zip(a.pose(start,0),c.pose(start,0)):
            self.assertLess(norm(sub(x,y)),1e-12)
        self.assertGreater(norm(sub(c.pose(0,0)[0],saved['actual_point_world_m'])),.05)
        self.assertNotEqual(c.pose(0,0)[1],saved['actual_axis_world'])

    def test_no_deviation_matches_previous_intended_effort_model(self):
        saved=plan_swing(prediction()['input'],.9,.16)
        for effort in (1.,.85):
            for fraction in (.35,.5,.65):
                e=SwingExecution(1,2,saved['nominal_time_sec'],effort,.18,fraction,.005)
                old=ExecutionMotion(state(),saved,e)
                new=PlanExecutionMotion(state(),SwingPlan.from_saved(saved,effort,.18,fraction),ExecutionDeviation(1.,.005),1,2)
                for t in (-.15,-.04,0,.03,.15):
                    for x,y in zip(old.pose(t,.003),new.pose(t,.003)):self.assertLess(norm(sub(x,y)),1e-12)
                    self.assertLess(norm(sub(old.velocity_at_phase(t,.003),new.velocity_at_phase(t,.003))),1e-11)

    def test_deviation_velocity_derivative_bound_and_delayed_start(self):
        saved=plan_swing(prediction()['input'],.9,.16);plan=SwingPlan.from_saved(saved)
        before=copy.deepcopy(plan)
        for ratio in (1.,.85):
            for offset in (0.,.005):
                m=PlanExecutionMotion(state(),plan,ExecutionDeviation(ratio,offset))
                for t in (-.12,-.05,0,.04,.12):
                    for axial in (-.16,0,.16):
                        def point(t):
                            p,a=m.pose(t,.002);return add(p,mul(a,axial))
                        h=1e-7;v=mul(sub(point(t+h),point(t-h)),1/(2*h))
                        self.assertLess(norm(sub(v,m.velocity_at_phase(t,.002,axial))),3e-7)
                        self.assertLessEqual(norm(sub(m.ball_velocity,v)),m.speed_bound+1e-6)
        delayed=PlanExecutionMotion(state(),plan,ExecutionDeviation(1.,.005))
        control=PlanExecutionMotion(state(),plan,ExecutionDeviation())
        for x,y in zip(delayed.pose(.005,0),control.pose(0,0)):self.assertLess(norm(sub(x,y)),1e-12)
        self.assertEqual(plan,before)

    def test_probe_controls_reused_and_deviation_cohorts_partition(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);src,path=source_db(root);digest=hashlib.sha256(path.read_bytes()).hexdigest()
            from bat_pivot_orientation_probe import search
            with patch('bat_pivot_orientation_probe.search',wraps=search) as calls:
                counts=probe.run(src,path,root/'out',execution_ratios=(1.,.85))
            self.assertEqual(calls.call_count,8);self.assertEqual(counts['new_searches'],8)
            self.assertEqual(digest,hashlib.sha256(path.read_bytes()).hexdigest())
            rows=csv_rows(root/'out'/'swing_execution_summary.csv')
            self.assertEqual(len({r['scenario'] for r in rows}),4)
            pairs=csv_rows(root/'out'/'swing_execution_deviation_pairs.csv')
            self.assertEqual(len({r['scenario'] for r in pairs}),2)
            kinematics=csv_rows(root/'out'/'swing_execution_kinematics.csv')
            for r in kinematics:
                if '_execution_0.85_' in r['scenario']:
                    self.assertGreater(float(r['avg_nominal_position_error_m']),.05)
                else:self.assertLess(float(r['avg_nominal_position_error_m']),1e-12)
            transitions=csv_rows(root/'out'/'swing_execution_deviation_transitions.csv')
            for scenario in {r['scenario'] for r in transitions}:
                for model in probe.MODEL_NAMES:
                    cells=[r for r in transitions if r['scenario']==scenario and r['scope']=='all_swings' and r['stratum']=='overall' and r['model']==model]
                    self.assertEqual(sum(int(r['trials']) for r in cells),2)
            states=[json.loads(x) for x in (root/'out'/'swing_execution_states.jsonl').read_text().splitlines()]
            for r in states:
                self.assertIn('start_pivot_world_m',r['plan']);self.assertIn('acceleration_ratio',r['execution_deviation'])

    def test_missing_plan_control_added_and_invalid_ratio_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);src,path=source_db(root)
            counts=probe.run(src,path,root/'out',efforts=(1.,),execution_ratios=(.85,),offsets=(.005,))
            self.assertEqual(counts['new_searches'],4)
            meta=json.loads((root/'out'/'swing_execution_metadata.json').read_text())
            self.assertEqual(len(meta['added_plan_controls']),1)
            with self.assertRaises(ValueError):probe.run(src,path,root/'bad',execution_ratios=(1.1,))
            self.assertFalse((root/'bad').exists())


if __name__=='__main__':unittest.main()
