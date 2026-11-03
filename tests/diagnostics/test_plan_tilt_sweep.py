import copy
import json
import math
import random
import tempfile
import unittest
from collections import defaultdict
from pathlib import Path
from unittest.mock import patch

from test_bat_pivot_orientation_probe import source_db
from test_forecast_contact_geometry import csv_rows,replace_csv
from test_contact_state_probe import state
from test_swing_prediction_probe import prediction
from swing_prediction_probe import plan_swing
from swing_execution import SwingPlan,PlanExecutionMotion,ExecutionDeviation
import plan_tilt_sweep as sweep


class PlanTiltTest(unittest.TestCase):
    def test_common_forecast_velocity_angular_error_and_forward_plans(self):
        for h in (-1,1):
            for error in (0.,2.):
                p=copy.deepcopy(prediction()['input']);p.update(handedness_sign=h,execution_angle_error_deg=error)
                saved=plan_swing(p,.9,.16);original=copy.deepcopy(saved)
                plans=[]
                for angle in (0.,15.,30.):
                    changed=sweep.replan(saved,angle,defaultdict(float))
                    self.assertEqual(changed['forecast_position_world_m'],saved['forecast_position_world_m'])
                    self.assertEqual(changed['nominal_time_sec'],saved['nominal_time_sec'])
                    self.assertEqual(changed['execution_angle_error_deg'],error)
                    self.assertLess(sweep.norm(sweep.sub(changed['ideal_velocity_world_ms'],saved['ideal_velocity_world_ms'])),1e-12)
                    self.assertAlmostEqual(math.degrees(math.asin(changed['ideal_axis_world'][2])),angle)
                    plan=SwingPlan.from_saved(changed);plans.append(plan)
                    motion=PlanExecutionMotion(state(),plan,ExecutionDeviation())
                    point,axis=motion.pose(0,0)
                    self.assertLess(sweep.norm(sweep.sub(point,changed['actual_point_world_m'])),1e-12)
                    if error==0:self.assertLess(sweep.norm(sweep.sub(point,changed['forecast_position_world_m'])),1e-12)
                    self.assertLess(sweep.norm(sweep.sub(motion.pose(-.09,0)[0],plan.start_point_world_m)),1e-12)
                    values=sweep.kinematics(changed,plan,motion)
                    self.assertLessEqual(values['pulse_min_pivot_z_m'],values['start_pivot_z_m'])
                self.assertGreater(sweep.norm(sweep.sub(plans[0].start_point_world_m,plans[-1].start_point_world_m)),.01)
                self.assertEqual(saved,original)

    def test_sample_is_order_outcome_and_rng_independent(self):
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'population.csv'
            rows=[dict(game_id=10,count_seq=i,variant='v',status='contact',ev_ms=50) for i in range(1,101)]
            replace_csv(path,rows);rng=random.getstate()
            selected,n=sweep.sample_keys(path,'v',20,'test')
            self.assertEqual(n,100);self.assertEqual(len(selected),20)
            self.assertEqual(random.getstate(),rng)
            changed=list(reversed(rows))
            for r in changed:r.update(status='no_contact',ev_ms=-123)
            replace_csv(path,changed)
            self.assertEqual(selected,sweep.sample_keys(path,'v',20,'test')[0])
            self.assertNotEqual(selected,sweep.sample_keys(path,'v',20,'another')[0])
            self.assertEqual(len(sweep.sample_keys(path,'v',0,'test')[0]),100)
            changed.append(changed[0]);replace_csv(path,changed)
            with self.assertRaisesRegex(ValueError,'duplicate'):sweep.sample_keys(path,'v',20,'test')

    def test_search_count_controls_partition_and_readonly(self):
        import bat_pivot_orientation_probe as physics
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,db=source_db(root);before=sweep.digest(db)
            original=physics.search
            with patch.object(physics,'search',wraps=original) as calls:
                result=sweep.run(source,db,root/'out',sample_size=1)
            self.assertEqual(result['sampled_trials'],1);self.assertEqual(result['new_searches'],4)
            self.assertEqual(calls.call_count,4);self.assertEqual(before,sweep.digest(db))
            summary=csv_rows(root/'out'/'plan_tilt_summary.csv')
            hist=csv_rows(root/'out'/'plan_tilt_ev_vla.csv')
            overall=[r for r in summary if r['stratum']=='overall']
            self.assertTrue(overall)
            controls={}
            for r in overall:
                self.assertEqual(int(r['trials']),1)
                if r['orientation']=='saved_tilt':
                    key=(r['scope'],r['model']);values=tuple(r[k] for k in ('successful_contacts','fair_contacts','avg_ev_ms','avg_vla_deg'))
                    self.assertEqual(controls.setdefault(key,values),values)
                for cohort,count in [('all_successful','successful_contacts'),('fair_direction','fair_contacts')]:
                    cells=[c for c in hist if all(c[k]==r[k] for k in ('scenario','scope','model','orientation')) and c['cohort']==cohort]
                    self.assertEqual(sum(int(c['contacts']) for c in cells),int(r[count]))
                    if cohort=='fair_direction':
                        low=sum(int(c['contacts']) for c in cells if c['ev_band_ms'] in ('<0','0..<10','10..<20','20..<25'))
                        high=sum(int(c['contacts']) for c in cells if c['vla_band_deg'] in ('50..<60','60..<90','90+'))
                        self.assertEqual(low,int(r['fair_low_ev']))
                        self.assertEqual(high,int(r['fair_high_vla']))
            transitions=csv_rows(root/'out'/'plan_tilt_transitions.csv')
            for scenario in ('tilt_0deg','tilt_15deg','tilt_30deg'):
                for model in physics.MODEL_NAMES:
                    cells=[r for r in transitions if (r['scenario'],r['scope'],r['model'],r['stratum'])==(scenario,'all_swings',model,'overall')]
                    self.assertEqual(sum(int(r['trials']) for r in cells),1)
            kin=csv_rows(root/'out'/'plan_tilt_kinematics.csv')
            self.assertEqual(len([r for r in kin if r['scope']=='all_swings']),4)
            states=[json.loads(x) for x in (root/'out'/'plan_tilt_states.jsonl').read_text().splitlines()]
            self.assertEqual(len(states),4)
            self.assertEqual(len(csv_rows(root/'out'/'plan_tilt_sample.csv')),1)
            metadata=json.loads((root/'out'/'plan_tilt_metadata.json').read_text())
            self.assertEqual(metadata['maximum_searches'],4)
            with self.assertRaises(FileExistsError):sweep.run(source,db,root/'out')

    def test_saved_tilt_matches_previous_execution_control(self):
        import swing_execution_probe as execution
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,db=source_db(root)
            sweep.run(source,db,root/'out',sample_size=0)
            execution.run(source,db,root/'old',efforts=(1.,))
            new=csv_rows(root/'out'/'plan_tilt_summary.csv');old=csv_rows(root/'old'/'swing_execution_summary.csv')
            for r in new:
                if r['stratum']!='overall' or r['orientation']!='saved_tilt':continue
                o=next(o for o in old if o['stratum']=='overall' and o['orientation']=='execution' and all(o[k]==r[k] for k in ('scope','model')))
                for field in ('trials','geometry_contacts','successful_contacts','fair_contacts','evaluable_trials'):
                    self.assertEqual(r[field],o[field])
                for field in ('avg_ev_ms','avg_vla_deg'):
                    if r[field]:self.assertAlmostEqual(float(r[field]),float(o[field]))

    def test_clock_invalid_is_shared_and_not_searched(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,db=source_db(root)
            path=source/'observation_update_inputs.jsonl';records=[json.loads(x) for x in path.read_text().splitlines()]
            for r in records:
                if r['observations']:r['observations'][-1]['time_sec']=r['plan']['nominal_time_sec']-.001
            path.write_text(''.join(json.dumps(r)+'\n' for r in records))
            with patch('bat_pivot_orientation_probe.search',side_effect=AssertionError('searched')):
                result=sweep.run(source,db,root/'out')
            self.assertEqual(result['new_searches'],0)
            for r in csv_rows(root/'out'/'plan_tilt_kinematics.csv'):
                self.assertEqual(int(r['evaluable_trials']),0)
                self.assertEqual(r['avg_start_pivot_z_m'],'')
            for r in csv_rows(root/'out'/'plan_tilt_summary.csv'):
                self.assertEqual(int(r['evaluable_trials']),0)
                self.assertEqual(int(r['geometry_contacts']),0)

    def test_bad_parameters_fail_before_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,db=source_db(root)
            for args in (dict(angles=()),dict(angles=(15.,15.)),dict(angles=(float('nan'),)),dict(angles=(-1.,)),dict(angles=(61.,)),dict(sample_size=-1),dict(duration=0)):
                with self.subTest(args=args),self.assertRaises(ValueError):sweep.run(source,db,root/'bad',**args)
                self.assertFalse((root/'bad').exists())


if __name__=='__main__':unittest.main()
