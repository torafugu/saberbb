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
from swing_prediction_probe import plan_swing
from planned_axis import plan_axis_swing
from swing_execution import SwingPlan,PlanExecutionMotion,ExecutionDeviation
from rotating_bat_probe import sub,mul,norm,dot
import plan_tilt_sweep as sweep
import plan_tilt_direction_audit as directions


class TiltYawTest(unittest.TestCase):
    def test_zero_yaw_is_exact_legacy_identity(self):
        for side in (-1,1):
            for angle in (0.,15.,35.):
                p=copy.deepcopy(prediction()['input']);p.update(handedness_sign=side,planned_bat_angle_deg=angle,execution_angle_error_deg=2.)
                self.assertEqual(plan_axis_swing(p,.9,.16,0.),plan_swing(p,.9,.16))

    def test_yaw_preserves_target_velocity_tilt_and_local_execution_error(self):
        for side in (-1,1):
            for yaw in (-10.,0.,10.):
                p=copy.deepcopy(prediction()['input']);p.update(handedness_sign=side,planned_bat_angle_deg=15.,execution_angle_error_deg=2.)
                old=plan_swing(p,.9,.16);new=plan_axis_swing(p,.9,.16,yaw)
                self.assertEqual(new['forecast_position_world_m'],old['forecast_position_world_m'])
                self.assertEqual(new['nominal_time_sec'],old['nominal_time_sec'])
                self.assertEqual(new['ideal_velocity_world_ms'],old['ideal_velocity_world_ms'])
                a=new['ideal_axis_world'];b=new['actual_axis_world']
                self.assertAlmostEqual(math.degrees(math.atan2(a[1],side*a[0])),yaw)
                self.assertAlmostEqual(math.degrees(math.asin(a[2])),15.)
                self.assertAlmostEqual(math.degrees(math.asin(b[2])),17.)
                self.assertAlmostEqual(math.degrees(math.atan2(b[1],side*b[0])),yaw)
                self.assertAlmostEqual(dot(a,new['ideal_omega_world_rad_s']),0.)
                self.assertAlmostEqual(dot(b,new['actual_omega_world_rad_s']),0.)

    def test_left_right_mirror_and_forward_velocity_derivative(self):
        cases=[]
        for side in (1,-1):
            p=copy.deepcopy(prediction()['input']);p.update(handedness_sign=side,planned_bat_angle_deg=15.,execution_angle_error_deg=2.)
            p['forecast']['position_world_m'][0]=0.
            saved=plan_axis_swing(p,.9,.16,10.);cases.append(saved)
            plan=SwingPlan.from_saved(saved);motion=PlanExecutionMotion(state(),plan,ExecutionDeviation())
            self.assertLess(norm(sub(motion.pose(0,0)[0],saved['actual_point_world_m'])),1e-12)
            for t in (-.05,0.,.04):
                for axial in (-.16,0.,.16):
                    def point(t):
                        v,a=motion.pose(t,.002);return tuple(x+axial*y for x,y in zip(v,a))
                    epsilon=1e-7;velocity=mul(sub(point(t+epsilon),point(t-epsilon)),1/(2*epsilon))
                    self.assertLess(norm(sub(velocity,motion.velocity_at_phase(t,.002,axial))),4e-7)
        for field in ('ideal_axis_world','actual_axis_world','pivot_world_m','actual_point_world_m','actual_velocity_world_ms'):
            right,left=cases[0][field],cases[1][field]
            self.assertLess(norm(sub((-right[0],right[1],right[2]),left)),1e-12)
        for field in ('ideal_omega_world_rad_s','actual_omega_world_rad_s','execution_rotation_world_rad'):
            right,left=cases[0][field],cases[1][field]
            self.assertLess(norm(sub((right[0],-right[1],-right[2]),left)),1e-12)

    def test_sweep_search_counts_zero_controls_and_cached_direction_audit(self):
        import bat_pivot_orientation_probe as physics
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,db=source_db(root);before=sweep.digest(db)
            with patch.object(physics,'search',wraps=physics.search) as calls:
                result=sweep.run(source,db,root/'yaw',angles=(0.,15.),yaws=(-10.,0.,10.),sample_size=0)
            self.assertEqual(result['new_searches'],14);self.assertEqual(calls.call_count,14)
            self.assertEqual(sweep.digest(db),before)
            sweep.run(source,db,root/'old',angles=(0.,15.),sample_size=0)
            old=[json.loads(x) for x in (root/'old'/'plan_tilt_states.jsonl').read_text().splitlines()]
            new=[json.loads(x) for x in (root/'yaw'/'plan_tilt_states.jsonl').read_text().splitlines()]
            for r in new:
                if not r['scenario'].endswith('_yaw_0deg'):continue
                oldname=r['scenario'].replace('_yaw_0deg','')
                o=next(o for o in old if (o['scenario'],o['game_id'],o['count_seq'])==(oldname,r['game_id'],r['count_seq']))
                self.assertEqual(r['contact'],o['contact']);self.assertEqual(r['responses'],o['responses'])
            metadata=json.loads((root/'yaw'/'plan_tilt_metadata.json').read_text())
            self.assertEqual(metadata['version'],2);self.assertEqual(len(metadata['scenarios']),6)
            self.assertEqual(directions.run([root/'yaw'],root/'directions')['new_searches'],0)
            rows=csv_rows(root/'yaw'/'plan_tilt_summary.csv')
            for r in rows:
                self.assertEqual(sum(int(r[k]) for k in ('fair_high_vla_negative_batter_hla','fair_high_vla_zero_batter_hla','fair_high_vla_positive_batter_hla')),int(r['fair_high_vla']))
            for r in rows:
                if r['stratum']!='overall' or r['scope']!='all_swings' or r['orientation']!='planned_tilt' or not int(r['fair_contacts']):continue
                a=next(x for x in csv_rows(root/'directions'/'plan_tilt_direction_summary.csv') if (x['dataset'],x['scenario'],x['stratum'],x['model'],x['cohort'])==('seed:0',r['scenario'],'overall',r['model'],'fair_direction'))
                self.assertAlmostEqual(float(r['fair_avg_abs_hla_deg']),float(a['avg_abs_hla_deg']))
                self.assertAlmostEqual(float(r['fair_central_abs_lt10_fraction']),float(a['central_abs_lt10_fraction']))

    def test_yaw_metadata_and_old_metadata_are_both_supported(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,db=source_db(root)
            sweep.run(source,db,root/'old',angles=(0.,15.),sample_size=0)
            sweep.run(source,db,root/'yaw',angles=(0.,15.),sample_size=0,sample_seed='1',yaws=(0.,10.))
            self.assertEqual(directions.run([root/'old',root/'yaw'],root/'out')['datasets'],2)

    def test_invalid_yaw_parameters_before_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,db=source_db(root)
            for yaws in ((),(0.,0.),(float('nan'),),(-46.,),(46.,)):
                with self.subTest(yaws=yaws),self.assertRaises(ValueError):sweep.run(source,db,root/'bad',yaws=yaws)
                self.assertFalse((root/'bad').exists())


if __name__=='__main__':unittest.main()
