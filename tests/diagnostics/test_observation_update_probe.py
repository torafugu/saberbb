import copy
import csv
import hashlib
import json
import random
import sqlite3
import tempfile
import unittest
from pathlib import Path

from test_swing_prediction_probe import prediction, ROOT
from test_contact_state_probe import state
import observation_update_probe as probe


def input_with_history():
    p=prediction()['input']
    # Analytic trajectory reaches (.25, 0, 1.0875) at .39 seconds.
    p['observation_history']=[dict(time_sec=t,position_world_m=[.1+(t-.24),6-40*(t-.24),
        1.5-2*(t-.24)-5*(t-.24)**2]) for t in (.08,.16,.24)]
    return p


class ObservationUpdateTest(unittest.TestCase):
    def test_exact_history_recovers_acceleration_velocity_and_forecast(self):
        p=input_with_history();history=probe.validate_history(p)
        f,v,a=probe.estimate(history)
        for actual,expected in zip(v,[1,-40,-2]):self.assertAlmostEqual(actual,expected)
        for actual,expected in zip(a,[0,0,-10]):self.assertAlmostEqual(actual,expected)
        self.assertAlmostEqual(f['time_sec'],.39)
        for actual,expected in zip(f['position_world_m'],[.25,0,1.0875]):self.assertAlmostEqual(actual,expected)
        changed=copy.deepcopy(p);changed['observation_history'][0]['time_sec']=.3
        with self.assertRaises(ValueError):probe.validate_history(changed)
        changed=copy.deepcopy(p);changed['observation_history'][-1]['position_world_m'][0]+=.01
        with self.assertRaises(ValueError):probe.validate_history(changed)
        p.pop('observation_history')
        with self.assertRaisesRegex(ValueError,'missing_observation_history'):probe.validate_history(p)

    def test_common_noise_ability_effect_and_no_global_rng(self):
        random.seed(123);before=random.getstate()
        n=probe.standard_noise(1,2)
        self.assertEqual(random.getstate(),before)
        self.assertEqual(n,probe.standard_noise(1,2));self.assertNotEqual(n,probe.standard_noise(1,3))
        h=input_with_history()['observation_history'];original=copy.deepcopy(h)
        low=probe.observe(h,.005,-2,n);high=probe.observe(h,.005,2,n)
        self.assertEqual(h,original)
        for o,l,r in zip(h,low,high):
            for x,y,z in zip(o['position_world_m'],l['position_world_m'],r['position_world_m']):
                self.assertLessEqual(abs(z-x),abs(y-x))
        self.assertEqual(probe.observe(h,0,0,n),h)
        # Estimator API has no actual endpoint, time, ability, legacy miss or future observation.
        f,_,_=probe.estimate(high)
        truth=state();truth['input']['ball_position_world_m']=[100,0,-100]
        self.assertEqual(f,probe.estimate(high)[0])

    def test_error_summary_preserves_denominators_and_signed_bias(self):
        rows=[dict(forecast_condition='test',sensor_sigma_m=.005,eye_for_sensor=eye,
            forecast_error_x_m=x,forecast_error_z_m=0.,forecast_time_error_ms=2*x)
            for eye,x in [(-2.,-1.),(0.,1.),(2.,3.)]]
        summary=probe.error_summary(rows)
        all_rows=[r for r in summary if r['eye_band']=='all']
        self.assertEqual(len(all_rows),1);r=all_rows[0]
        self.assertEqual(r['balls'],3);self.assertAlmostEqual(r['mean_signed_x_m'],1.)
        self.assertAlmostEqual(r['mean_abs_x_m'],5/3)
        self.assertAlmostEqual(r['p50_abs_x_m'],1.)
        self.assertAlmostEqual(r['p90_abs_x_m'],2.6)
        self.assertEqual(sum(x['balls'] for x in summary if x['eye_band']!='all'),3)

    def test_readonly_coverage_pairs_legacy_and_exact_control(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);path=root/'test.db';db=sqlite3.connect(path)
            db.execute('create table player_game_batting(game_id,count_seq,adjusted_contact_type,result,launch_speed,launch_angle,final_hla_deg)')
            db.executescript((ROOT/'migrations/upgrades/add_contact_state_shadow.sql').read_text())
            for seq in (1,2,3):
                db.execute("insert into player_game_batting values(10,?,'SolidContact','Out',46,10,0)",(seq,))
                s=state();s['swing_prediction']=prediction()
                if seq!=2:s['swing_prediction']['input']=input_with_history()
                if seq==3:s['swing_prediction']['input']['observation_history'][1]['time_sec']=.3
                db.execute('insert into player_game_contact_state values(10,?,1,?,NULL,?)',(seq,probe.SOURCE,json.dumps(s)))
            # Orphan key remains accounted for.
            s=state();s['swing_prediction']=prediction();s['swing_prediction']['input']=input_with_history()
            db.execute('insert into player_game_contact_state values(10,4,1,?,NULL,?)',(probe.SOURCE,json.dumps(s)))
            db.commit();db.close();before=hashlib.sha256(path.read_bytes()).hexdigest()
            out=root/'output';coverage=probe.run(path,10,10,out,sigmas=(0,.005),levers=(.9,))
            self.assertEqual(coverage['swings'],4);self.assertEqual(coverage['valid_histories'],2)
            self.assertEqual(coverage['trial_rows'],8)
            def rows(name):
                with (out/f'observation_update_{name}.csv').open() as f:return list(csv.DictReader(f))
            self.assertEqual(len(rows('contacts')),8);self.assertEqual(len(rows('forecasts')),8)
            self.assertEqual(len(rows('eye_sensitivity')),12)
            self.assertEqual(sum(int(r['states']) for r in rows('transitions')),6)
            self.assertTrue(any(r['status']=='missing_observation_history' for r in rows('status')))
            bycondition={r['forecast_condition']:r for r in rows('contacts') if r['count_seq']=='1'}
            for field in ('status','ev_ms','hla_deg','vla_deg'):
                self.assertEqual(bycondition['observed_sigma_0'][field],bycondition['exact_observation_control'][field])
            self.assertEqual(before,hashlib.sha256(path.read_bytes()).hexdigest())
            with self.assertRaises(FileExistsError):probe.run(path,10,10,out)
            self.assertEqual(probe.run(path,20,20,root/'empty')['trial_rows'],0)
            for sigmas in [(),(-.1,),(.1,.1),(float('nan'),)]:
                with self.assertRaises(ValueError):probe.run(path,10,10,root/'bad',sigmas=sigmas)


if __name__=='__main__':unittest.main()
