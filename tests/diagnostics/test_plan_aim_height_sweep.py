import copy
import json
import math
import tempfile
import unittest
from collections import defaultdict
from pathlib import Path
from unittest.mock import patch

from test_contact_state_probe import state
from test_swing_prediction_probe import prediction
from test_bat_pivot_orientation_probe import source_db
from test_forecast_contact_geometry import csv_rows
from planned_axis import plan_axis_swing
from swing_execution import SwingPlan,PlanExecutionMotion,ExecutionDeviation
from bat_pivot_orientation_probe import evaluate
from rotating_bat_probe import add,sub,mul,norm
import plan_tilt_sweep as sweep
import plan_tilt_direction_audit as directions


class AimHeightTest(unittest.TestCase):
    def test_forecast_and_intended_aim_are_distinct_without_changing_velocity(self):
        for h in (-1,1):
            for yaw in (-10.,0.,10.):
                p=copy.deepcopy(prediction()['input']);p.update(handedness_sign=h,execution_angle_error_deg=2.)
                original=copy.deepcopy(p);base=plan_axis_swing(p,.9,.16,yaw)
                for z in (-.01,0.,.01):
                    changed=plan_axis_swing(p,.9,.16,yaw,(0.,0.,z))
                    self.assertEqual(changed['forecast_position_world_m'],base['forecast_position_world_m'])
                    self.assertEqual(changed['nominal_time_sec'],base['nominal_time_sec'])
                    self.assertEqual(changed['planned_aim_point_world_m'],add(base['forecast_position_world_m'],(0,0,z)))
                    for f in ('pivot_world_m','ideal_point_world_m','actual_point_world_m'):
                        self.assertEqual(changed[f],add(base[f],(0,0,z)))
                    for f in ('ideal_velocity_world_ms','actual_velocity_world_ms','ideal_axis_world','actual_axis_world',
                              'ideal_omega_world_rad_s','actual_omega_world_rad_s','execution_rotation_world_rad'):
                        self.assertEqual(changed[f],base[f])
                self.assertEqual(p,original)

    def test_start_pose_and_forward_trajectory_translate_by_the_intended_offset(self):
        p=copy.deepcopy(prediction()['input']);p.update(planned_bat_angle_deg=15.,execution_angle_error_deg=1.)
        base=plan_axis_swing(p,.9,.16,10.)
        b=PlanExecutionMotion(state(),SwingPlan.from_saved(base),ExecutionDeviation())
        for z in (-.01,.01):
            saved=plan_axis_swing(p,.9,.16,10.,(0,0,z));plan=SwingPlan.from_saved(saved)
            m=PlanExecutionMotion(state(),plan,ExecutionDeviation())
            for t in (-.09,-.04,0,.03,.09):
                point,axis=m.pose(t,0);bp,ba=b.pose(t,0)
                self.assertLess(norm(sub(point,add(bp,(0,0,z)))),1e-12)
                self.assertLess(norm(sub(axis,ba)),1e-12)
                self.assertLess(norm(sub(m.velocity_at_phase(t,0),b.velocity_at_phase(t,0))),1e-12)
            self.assertLess(norm(sub(m.pose(-.09,0)[0],plan.start_point_world_m)),1e-12)

    def test_negative_aim_is_below_ball_and_creates_upward_normal(self):
        p=copy.deepcopy(prediction()['input']);p.update(planned_bat_angle_deg=0.,planned_attack_angle_deg=0.,execution_angle_error_deg=0.,bat_speed_ms=35.)
        p['forecast']=dict(position_world_m=[0.,0.,.9],time_sec=.4)
        s=state();s['input'].update(ball_position_world_m=[0.,0.,.9],ball_velocity_world_ms=[0.,-40.,0.],reference_time_sec_since_release=.4)
        row=dict(window_start_ms=-60.,window_end_ms=60.,lever_m=.9,half_length_m=.16,effective_radius_m=.07)
        for z in (-.01,.01):
            saved=plan_axis_swing(p,.9,.16,0.,(0,0,z));plan=SwingPlan.from_saved(saved)
            motion=PlanExecutionMotion(s,plan,ExecutionDeviation())
            r=evaluate(row,s,saved,defaultdict(float),motion)
            self.assertEqual(r['status'],'contact')
            self.assertLess(r['normal_z']*z,0.)
            self.assertLess(r['vla_deg']*z,0.)

    def test_sweep_zero_offset_matches_previous_and_cached_audit_accepts_version3(self):
        import bat_pivot_orientation_probe as physics
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,db=source_db(root);before=sweep.digest(db)
            with patch.object(physics,'search',wraps=physics.search) as calls:
                result=sweep.run(source,db,root/'aim',angles=(0.,15.),sample_size=0,yaws=(0.,),aim_z_offsets=(-.01,0.,.01))
            self.assertEqual(calls.call_count,14);self.assertEqual(result['maximum_searches'],14)
            self.assertEqual(before,sweep.digest(db))
            sweep.run(source,db,root/'old',angles=(0.,15.),sample_size=0,yaws=(0.,))
            old=[json.loads(x) for x in (root/'old'/'plan_tilt_states.jsonl').read_text().splitlines()]
            new=[json.loads(x) for x in (root/'aim'/'plan_tilt_states.jsonl').read_text().splitlines()]
            for r in new:
                if not r['scenario'].endswith('_aim_z_0mm'):continue
                name=r['scenario'].replace('_aim_z_0mm','')
                o=next(o for o in old if (o['scenario'],o['game_id'],o['count_seq'])==(name,r['game_id'],r['count_seq']))
                self.assertEqual(r['contact'],o['contact']);self.assertEqual(r['responses'],o['responses'])
            meta=json.loads((root/'aim'/'plan_tilt_metadata.json').read_text())
            self.assertEqual(meta['version'],3);self.assertEqual(len(meta['scenarios']),6)
            self.assertEqual(directions.run([root/'aim'],root/'audit')['new_searches'],0)
            kin=csv_rows(root/'aim'/'plan_tilt_kinematics.csv')
            for r in kin:
                if r['scenario']=='saved_tilt':continue
                z=meta['scenarios'][r['scenario']]['aim_z_offset_m']
                self.assertAlmostEqual(float(r['avg_planned_aim_z_offset_m']),z)
                self.assertLess(float(r['avg_reference_aim_error_m']),.01)
            # Declared aim must agree with the cached plan, not just summary outcomes.
            r=next(r for r in new if r['scenario'].endswith('_aim_z_-10mm'))
            r['forecast_plan']['planned_aim_offset_world_m'][2]=.01
            (root/'aim'/'plan_tilt_states.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in new))
            with self.assertRaisesRegex(ValueError,'declared_aim_offset'):directions.run([root/'aim'],root/'bad')

    def test_invalid_offsets_fail_before_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,db=source_db(root)
            for zs in ((),(0.,0.),(float('nan'),),(float('inf'),)):
                with self.subTest(zs=zs),self.assertRaises(ValueError):sweep.run(source,db,root/'bad',aim_z_offsets=zs)
                self.assertFalse((root/'bad').exists())
        for offset in ((0,0),[0,0,float('nan')],[0,0,True]):
            with self.assertRaises(ValueError):plan_axis_swing(prediction()['input'],.9,.16,aim_offset_world_m=offset)

    def test_aim_does_not_change_clock_eligibility(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,db=source_db(root)
            path=source/'observation_update_inputs.jsonl';rows=[json.loads(x) for x in path.read_text().splitlines()]
            for r in rows:
                if r['observations']:r['observations'][-1]['time_sec']=r['plan']['nominal_time_sec']-.001
            path.write_text(''.join(json.dumps(r)+'\n' for r in rows))
            with patch('bat_pivot_orientation_probe.search',side_effect=AssertionError('searched')):
                result=sweep.run(source,db,root/'out',angles=(0.,15.),yaws=(0.,),aim_z_offsets=(-.01,0.,.01))
            self.assertEqual(result['new_searches'],0)
            directions.run([root/'out'],root/'audit')
            for r in csv_rows(root/'out'/'plan_tilt_kinematics.csv'):
                self.assertEqual(r['evaluable_trials'],'0');self.assertEqual(r['avg_planned_aim_z_m'],'')


if __name__=='__main__':unittest.main()
