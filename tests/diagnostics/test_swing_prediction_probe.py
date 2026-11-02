import copy
import csv
import hashlib
import json
import math
import sqlite3
import sys
import tempfile
import unittest
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools/diagnostics'))
import swing_prediction_probe as probe
from test_contact_state_probe import state
from rotating_bat_probe import result_record


def prediction():
    observation=dict(time_sec=.24,position_world_m=[.1,6.,1.5])
    velocity=[1.,-40.,-2.];acceleration=[0.,0.,-10.]
    forecast=probe.forecast(observation,velocity,acceleration)
    forecast['position_world_m']=list(forecast['position_world_m'])
    angle=probe.angle_for_position(forecast['position_world_m'])
    return dict(version=1,source=probe.PREDICTION_SOURCE,invalid_reason=None,input=dict(observation=observation,
        estimated_velocity_world_ms=velocity,estimated_acceleration_world_ms2=acceleration,forecast=forecast,
        prior_flight_time_sec=.4,prior_speed_ms=40.,commit_ratio=.6,planned_bat_angle_deg=angle,
        planned_attack_angle_deg=10.,execution_angle_error_deg=0.,bat_speed_ms=35.,handedness_sign=1.,
        batting_eye=0.,bat_control=0.,zone_modifier_at_prediction=1.,residual_execution_delay_sec=0.))


class SwingPredictionTest(unittest.TestCase):
    def test_forecast_analytic_and_saved_reconstruction(self):
        p,checks=probe.validate_prediction(prediction())
        self.assertAlmostEqual(p['forecast']['time_sec'],.39)
        self.assertAlmostEqual(p['forecast']['position_world_m'][0],.25)
        self.assertAlmostEqual(p['forecast']['position_world_m'][2],1.0875)
        self.assertLess(max(checks.values()),1e-12)
        changed=prediction();changed['input']['forecast']['position_world_m'][0]+=.01
        with self.assertRaises(ValueError):probe.validate_prediction(changed)
        changed=prediction();changed['input']['residual_execution_delay_sec']=.01
        with self.assertRaises(ValueError):probe.validate_prediction(changed)
        for velocity,accel in [([0,0,0],[0,0,0]),([0,-40,0],[0,1,0])]:
            with self.assertRaises(ValueError):probe.forecast(p['observation'],velocity,accel)

    def test_plan_has_no_actual_endpoint_or_old_contact_input(self):
        p,_=probe.validate_prediction(prediction());before=probe.plan_swing(p,.9,.16)
        truth1=state();truth2=state()
        truth2['input']['ball_position_world_m']=[100,0,-100]
        truth2['input']['reference_time_sec_since_release']=.8
        truth2['input']['legacy_timing_impact_x_m']=100
        truth2['input']['timing_offset_sec']=.5
        # Truth can change forecast-error metrics and contact tests, but is absent from plan API.
        after=probe.plan_swing(p,.9,.16)
        self.assertEqual(before,after)
        def physical_motion(truth):
            return probe.FrameMotion(truth,.9,.16,before['pivot_world_m'],before['pivot_velocity_world_ms'],
                before['actual_axis_world'],before['actual_omega_world_rad_s'])
        self.assertNotEqual(physical_motion(truth1).evaluate(0,0)['clearance_m'],
                            physical_motion(truth2).evaluate(0,0)['clearance_m'])
        self.assertEqual(before['forecast_position_world_m'],tuple(p['forecast']['position_world_m']))
        self.assertAlmostEqual(before['nominal_time_sec'],.39)
        self.assertNotEqual(before['forecast_position_world_m'],tuple(truth1['input']['ball_position_world_m']))
        # The forecast part of the plan can be moved only by changing forecast inputs.
        changed=copy.deepcopy(p);changed['forecast']['position_world_m'][0]+=.1
        self.assertNotEqual(before,probe.plan_swing(changed,.9,.16))

    def test_common_pivot_execution_and_timing_once(self):
        p,_=probe.validate_prediction(prediction());p['execution_angle_error_deg']=5
        plan=probe.plan_swing(p,.9,.16)
        delta=probe.norm(probe.sub(plan['actual_point_world_m'],plan['ideal_point_world_m']))
        self.assertAlmostEqual(delta,2*.9*math.sin(math.radians(5)/2))
        s=state();m=probe.FrameMotion(s,.9,.16,plan['pivot_world_m'],plan['pivot_velocity_world_ms'],plan['actual_axis_world'],plan['actual_omega_world_rad_s'])
        forecast_delay=plan['nominal_time_sec']-s['input']['reference_time_sec_since_release']
        trial=copy.deepcopy(s);trial['input']['timing_offset_sec']=forecast_delay
        r=result_record(dict(game_id=1,count_seq=1,adjusted_contact_type='SolidContact'),trial,m,'forecast',-.06,.06,1e-6,4000)
        self.assertAlmostEqual(r['timing_offset_ms'],forecast_delay*1000)
        self.assertNotAlmostEqual(r['timing_offset_ms'],(forecast_delay+s['input']['timing_offset_sec'])*1000)
        mirrored=copy.deepcopy(p);mirrored['handedness_sign']=-1;mirrored['forecast']['position_world_m'][0]*=-1
        left=probe.plan_swing(mirrored,.9,.16)
        self.assertAlmostEqual(left['actual_point_world_m'][0],-plan['actual_point_world_m'][0])
        self.assertAlmostEqual(left['actual_point_world_m'][2],plan['actual_point_world_m'][2])

    def test_prediction_angle_rule_with_nonzero_control(self):
        record=prediction();p=record['input'];p['bat_control']=1;p['zone_modifier_at_prediction']=.3
        pos=list(p['forecast']['position_world_m']);pos[2]=.825+(pos[2]-.825)*(1+.05*.7)
        p['execution_angle_error_deg']=(probe.angle_for_position(pos)-p['planned_bat_angle_deg'])*max(.1,1-1/(1+math.exp(-1)))
        _,checks=probe.validate_prediction(record)
        self.assertLess(max(checks.values()),1e-12)

    def test_db_readonly_old_rows_missing_forecast_pairs_and_metadata(self):
        with tempfile.TemporaryDirectory() as folder:
            folder=Path(folder);path=folder/'test.db';db=sqlite3.connect(path)
            db.execute('create table player_game_batting(game_id,count_seq,adjusted_contact_type,result,launch_speed,launch_angle,final_hla_deg)')
            db.executescript((ROOT/'migrations/upgrades/add_contact_state_shadow.sql').read_text())
            for seq,kind in [(1,'SolidContact'),(2,'SwungAndMiss'),(3,'WeakContact')]:
                db.execute('insert into player_game_batting values(10,?,?,?,46,10,0)',(seq,kind,'Out'))
            for seq in (1,2,3,4,5):
                payload=state()
                if seq!=2:payload['swing_prediction']=prediction()
                if seq==3:payload['swing_prediction']['invalid_reason']='invalid_clock';payload['swing_prediction']['input']=None
                if seq==5:payload['swing_prediction']['source']='future'
                db.execute('insert into player_game_contact_state values(10,?,1,?,NULL,?)',(seq,probe.SOURCE,json.dumps(payload)))
            db.commit();db.close();before=hashlib.sha256(path.read_bytes()).hexdigest()
            out=folder/'output';coverage=probe.run(path,10,10,out,levers=(.9,))
            self.assertEqual(coverage['swings'],5);self.assertEqual(coverage['valid_contact_states'],5)
            self.assertEqual(coverage['valid_predictions'],2);self.assertEqual(coverage['trial_rows'],6)
            def read(name):
                with (out/name).open() as f:return list(csv.DictReader(f))
            self.assertEqual(len(read('swing_plan_contacts.csv')),6)
            self.assertEqual(len(read('swing_plan_forecasts.csv')),2)
            self.assertEqual(sum(int(r['states']) for r in read('swing_plan_transitions.csv')),4)
            self.assertEqual(sum(int(r['states']) for r in read('contact_3d_status.csv')),6)
            self.assertTrue(any(r['status']=='missing_prediction' for r in read('swing_plan_status.csv')))
            rows=read('swing_plan_contacts.csv')
            for r in rows:
                if r['planning_condition']=='forecast_position_time':self.assertAlmostEqual(float(r['nominal_swing_time_sec']),.39)
                if r['planning_condition']=='forecast_oracle_time':self.assertAlmostEqual(float(r['nominal_swing_time_sec']),.4)
            self.assertEqual(before,hashlib.sha256(path.read_bytes()).hexdigest())
            self.assertEqual(json.loads((out/'swing_plan_metadata.json').read_text())['residual_execution_delay_sec'],0)
            with self.assertRaises(FileExistsError):probe.run(path,10,10,out)
            self.assertEqual(probe.run(path,20,20,folder/'empty')['valid_predictions'],0)


if __name__=='__main__':unittest.main()
