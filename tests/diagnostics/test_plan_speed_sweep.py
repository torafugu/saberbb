import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_bat_pivot_orientation_probe import source_db
from test_forecast_contact_geometry import csv_rows
from planned_axis import with_planned_speed
from swing_execution import SwingPlan
from rotating_bat_probe import add,mul,norm,rotate,sub
import plan_tilt_sweep as sweep
import plan_tilt_direction_audit as direction
import plan_contact_ev_audit as ev


class PlannedSpeedTest(unittest.TestCase):
    def fixture(self,root,**kw):
        src,db=source_db(root)
        sweep.run(src,db,root/'speed',sample_size=0,angles=(0.,15.),yaws=(0.,),
                  aim_z_offsets=(0.,),speed_ratios=(.4,.6,.8,1.),**kw)
        return src,db,root/'speed'

    def test_reference_invariants_scaled_speed_and_distinct_start_poses(self):
        with tempfile.TemporaryDirectory() as t:
            _,_,folder=self.fixture(Path(t))
            rows=[json.loads(x) for x in (folder/'plan_tilt_states.jsonl').read_text().splitlines()]
            for r in rows:
                if r['scenario']=='saved_tilt':continue
                p=r['forecast_plan'];ratio=p['planned_speed_ratio']
                control=next(x for x in rows if x['game_id']==r['game_id'] and x['count_seq']==r['count_seq']
                    and x['scenario']==r['scenario'].split('_speed_r')[0]+'_speed_r1')['forecast_plan']
                for k in ['forecast_position_world_m','nominal_time_sec','ideal_axis_world','actual_axis_world',
                          'ideal_point_world_m','actual_point_world_m','pivot_world_m','execution_angle_error_deg']:
                    self.assertEqual(p[k],control[k])
                for k in ['ideal_velocity_world_ms','actual_velocity_world_ms','pivot_velocity_world_ms',
                          'ideal_omega_world_rad_s','actual_omega_world_rad_s']:
                    self.assertEqual(p[k],list(mul(control[k],ratio)))
                plan=SwingPlan.from_saved(p);full=SwingPlan.from_saved(control)
                self.assertEqual(plan.planned_effort,1.)
                # Independently integrate the analytic pulse to its forecast clock.
                from swing_execution import SwingExecution
                clock=SwingExecution(0,0,p['nominal_time_sec'],1.,.18,.5,0.)
                progress=clock.integral(0.)
                axis=rotate(plan.start_axis_world,plan.peak_omega_world_rad_s,progress)
                pivot=add(plan.start_pivot_world_m,mul(plan.peak_pivot_velocity_world_ms,progress))
                self.assertLess(norm(sub(add(pivot,mul(axis,plan.lever_m)),p['actual_point_world_m'])),1e-12)
                self.assertAlmostEqual(r['kinematics']['reference_point_speed_ms'],
                    norm(control['actual_velocity_world_ms'])*ratio,places=10)
                if ratio!=1:self.assertGreater(norm(sub(plan.start_point_world_m,full.start_point_world_m)),.01)

    def test_unit_ratio_reproduces_prior_aim_trajectories_and_cache_audits(self):
        with tempfile.TemporaryDirectory() as t:
            root=Path(t);src,db,folder=self.fixture(root)
            sweep.run(src,db,root/'old',sample_size=0,angles=(0.,15.),yaws=(0.,),aim_z_offsets=(0.,))
            before=[json.loads(x) for x in (root/'old'/'plan_tilt_states.jsonl').read_text().splitlines()]
            after=[json.loads(x) for x in (folder/'plan_tilt_states.jsonl').read_text().splitlines()]
            for r in before:
                name=r['scenario']+('_speed_r1' if r['scenario']!='saved_tilt' else '')
                s=next(x for x in after if x['scenario']==name and x['game_id']==r['game_id'] and x['count_seq']==r['count_seq'])
                for field in ['plan','execution','entry_time_sec','search_status','contact','responses']:
                    self.assertEqual(r[field],s[field])
            self.assertEqual((folder/'plan_tilt_sample.csv').read_bytes(),(root/'old'/'plan_tilt_sample.csv').read_bytes())
            meta=json.loads((folder/'plan_tilt_metadata.json').read_text())
            self.assertEqual(meta['version'],4);self.assertEqual(meta['new_searches'],18)
            with patch('sqlite3.connect',side_effect=AssertionError('DB')), \
                 patch('bat_pivot_orientation_probe.search',side_effect=AssertionError('search')), \
                 patch('finite_barrel_collision.solve',side_effect=AssertionError('solve')):
                direction.run([folder],root/'direction');ev.run([folder],root/'ev')
            for r in csv_rows(folder/'plan_tilt_summary.csv'):
                self.assertEqual(int(r['trials']),int(r['evaluable_trials'])+int(r['execution_clock_invalid_trials']))

    def test_bad_ratios_and_cached_declarations_rejected(self):
        for ratio in [0.,-1.,1.1,float('nan'),True]:
            with self.subTest(ratio=ratio),self.assertRaises(ValueError):with_planned_speed({},ratio)
        with tempfile.TemporaryDirectory() as t:
            root=Path(t);_,_,folder=self.fixture(root);p=folder/'plan_tilt_states.jsonl'
            rows=[json.loads(x) for x in p.read_text().splitlines()]
            r=next(x for x in rows if '_speed_r0.4' in x['scenario'])
            r['forecast_plan']['planned_speed_ratio']=.5
            p.write_text(''.join(json.dumps(x)+'\n' for x in rows))
            with self.assertRaises(ValueError):ev.run([folder],root/'bad')
            self.assertFalse((root/'bad').exists())


if __name__=='__main__':unittest.main()
