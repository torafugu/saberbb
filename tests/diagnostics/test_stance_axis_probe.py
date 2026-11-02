import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from test_forecast_contact_geometry import fixture, csv_rows
from test_observation_update_probe import input_with_history
import stance_axis_probe as probe


class StanceAxisTest(unittest.TestCase):
    def test_equal_angles_equal_plans_and_stance_independent_of_target(self):
        p=input_with_history();f=p['forecast']
        angle=probe.angle_for_position(f['position_world_m'])
        self.assertEqual(probe.make_plan(p,f,.9),probe.make_plan(p,f,.9,angle))
        plan=probe.make_plan(p,f,.9,30)
        changed=copy.deepcopy(f);changed['position_world_m'][2]+=.2;changed['time_sec']+=.01
        moved=probe.make_plan(p,changed,.9,30)
        for key in ('actual_axis_world','actual_velocity_world_ms','actual_omega_world_rad_s','planned_axis_angle_deg'):
            self.assertEqual(plan[key],moved[key])
        self.assertAlmostEqual(moved['actual_point_world_m'][2]-plan['actual_point_world_m'][2],.2)
        self.assertAlmostEqual(moved['nominal_time_sec']-plan['nominal_time_sec'],.01)
        # Candidate API accepts no actual ball endpoint/time or legacy contact outcome.
        self.assertEqual(p,input_with_history())
        self.assertNotEqual(probe.make_plan(p,changed,.9)['actual_axis_world'],probe.make_plan(p,f,.9)['actual_axis_world'])

    def test_mirror_and_finite_frame(self):
        p=input_with_history();p['execution_angle_error_deg']=3.
        right=probe.make_plan(p,p['forecast'],.9,30)
        left_p=copy.deepcopy(p);left_p['handedness_sign']=-1;left_p['forecast']['position_world_m'][0]*=-1
        left=probe.make_plan(left_p,left_p['forecast'],.9,30)
        self.assertAlmostEqual(left['actual_point_world_m'][0],-right['actual_point_world_m'][0])
        self.assertAlmostEqual(left['actual_point_world_m'][2],right['actual_point_world_m'][2])
        self.assertAlmostEqual(probe.norm(right['actual_axis_world']),1.)

    def test_baseline_replay_pair_denominators_and_db_unchanged(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root);path=root/'test.db'
            before=hashlib.sha256(path.read_bytes()).hexdigest()
            out=root/'out';c=probe.run(path,10,10,out,angles=(30.,),levers=(.9,),sigmas=(.005,))
            self.assertEqual(c['valid_histories'],2);self.assertEqual(c['trial_rows'],12)
            self.assertEqual(before,hashlib.sha256(path.read_bytes()).hexdigest())
            summary=csv_rows(out/'stance_axis_summary.csv')
            old=csv_rows(source/'observation_update_summary.csv')
            for r in summary:
                if r['scope']!='all_swings' or r['z_error_band']!='all' or r['axis_model']!='height_dependent':continue
                rs=[x for x in old if x['variant']==r['forecast_condition']+'_r0.9' and x['timing_band']=='all']
                self.assertEqual(int(r['contact']),sum(int(x['contact_states']) for x in rs))
                for key in ('avg_ev_ms','avg_vla_deg','foul_direction_pct'):
                    n=sum(int(x['impulse_contacts']) for x in rs)
                    expected=sum(float(x[key])*int(x['impulse_contacts']) for x in rs if x[key]!='')/n
                    self.assertAlmostEqual(float(r[key]),expected)
            transitions=csv_rows(out/'stance_axis_transitions.csv')
            pairs=csv_rows(out/'stance_axis_pairs.csv')
            for r in pairs:
                ts=[t for t in transitions if all(t[k]==r[k] for k in probe.KEYS)]
                self.assertEqual(sum(int(t['states']) for t in ts),int(r['states']))
                self.assertEqual(sum(int(t['states']) for t in ts if t['before_collision_status']==t['after_collision_status']=='impulse_contact'),int(r['paired_impulse_contacts']))
            for scope,n in [('all_swings',2),('recorded_batting_only',1)]:
                rs=[r for r in summary if r['scope']==scope and r['z_error_band']=='all']
                self.assertEqual(sum(int(r['states']) for r in rs),n*6)
            checks=csv_rows(out/'stance_axis_checks.csv')
            self.assertLess(max(float(r['max_error']) for r in checks),1e-8)
            with self.assertRaises(FileExistsError):probe.run(path,10,10,out)
            self.assertEqual(probe.run(path,20,20,root/'empty')['trial_rows'],0)
            for angles in [(),(30.,30.),(float('nan'),),(81.,)]:
                with self.assertRaises(ValueError):probe.run(path,10,10,root/'bad',angles=angles)

    def test_noncontact_not_invented_in_pairs(self):
        p=input_with_history();from test_contact_state_probe import state
        from test_swing_prediction_probe import prediction
        s=state();plan=probe.make_plan(p,p['forecast'],.9,30)
        plan['pivot_world_m']=probe.sub(plan['pivot_world_m'],(100.,0.,0.))
        r=probe.evaluate(dict(game_id=1,count_seq=1,adjusted_contact_type='SolidContact'),s,plan)
        self.assertEqual(r['status'],'no_contact');self.assertIsNone(r['ev_ms'])
        a=probe.Aggregate();a.add('SolidContact','prior_only',.9,'zero_1e-8',{'height_dependent':r,'stance_30deg':r})
        self.assertTrue(all(g['paired_impulse_contacts']==0 for g in a.pairs.values()))


if __name__=='__main__':unittest.main()
