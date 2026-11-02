import copy
import hashlib
import json
import math
import sqlite3
import tempfile
import unittest
from collections import Counter
from pathlib import Path

from test_contact_state_probe import state
from test_forecast_contact_geometry import fixture, csv_rows
import symmetric_height_probe as probe


class SymmetricHeightTest(unittest.TestCase):
    def test_analytic_up_down_mirror_and_miss(self):
        s=state();s['input']['ball_velocity_world_ms']=[0,-40,0]
        point=s['input']['ball_position_world_m'];velocity=(0,35,0)
        def frame(error):return dict(point=probe.add(point,(0,0,error)),axis=(1,0,0),velocity=velocity,nominal_time_sec=.4)
        lower=probe.trial(s,frame(-.03));upper=probe.trial(s,frame(.03));zero=probe.trial(s,frame(0))
        self.assertEqual(lower['collision_status'],'impulse_contact');self.assertEqual(upper['collision_status'],'impulse_contact')
        self.assertAlmostEqual(lower['ev_ms'],upper['ev_ms'])
        self.assertAlmostEqual(lower['vla_deg'],-upper['vla_deg'])
        self.assertAlmostEqual(lower['normal_z'],-upper['normal_z'])
        self.assertGreater(lower['vla_deg'],0);self.assertAlmostEqual(zero['vla_deg'],0)
        m=probe.motion_for(s,frame(.03))
        for t in (-.05,0,.05):
            value=m.evaluate(t,0);self.assertEqual(value['bat_velocity'],velocity)
            self.assertEqual(value['axis'],(1.,0.,0.))
        miss=probe.trial(s,frame(.1));self.assertEqual(miss['search_status'],'no_contact');self.assertIsNone(miss['vla_deg'])
        g=Counter();probe.pair_add(g,lower,upper,zero)
        self.assertAlmostEqual(g['vla_deg_center_shift'],0);self.assertEqual(g['triple_impulse_contacts'],1)

    def test_axis_rule_constant_velocity_anchor_and_clamps(self):
        from test_observation_update_probe import input_with_history
        p=input_with_history();p['execution_angle_error_deg']=2.
        base=probe.plan_swing(p,.9,.16)
        fixed=probe.control_frame(p,base,.9,0,probe.AXES[0]);adaptive=probe.control_frame(p,base,.9,0,probe.AXES[1])
        self.assertEqual(fixed,adaptive)
        for dz in (-.05,.05,10.):
            a=probe.control_frame(p,base,.9,dz,probe.AXES[0]);b=probe.control_frame(p,base,.9,dz,probe.AXES[1])
            self.assertEqual(a['point'],b['point']);self.assertEqual(a['velocity'],b['velocity'])
            self.assertEqual(a['nominal_time_sec'],b['nominal_time_sec'])
            self.assertNotEqual(a['axis'],b['axis'])
            self.assertAlmostEqual(probe.norm(b['axis']),1.)
        self.assertEqual(probe.angle_for_position([0,0,100]),10.)
        self.assertEqual(probe.angle_for_position([0,0,-100]),60.)

    def test_readonly_coverage_pairs_zero_equality_and_missing_history(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);fixture(root);path=root/'test.db'
            db=sqlite3.connect(path)
            payload=json.loads(db.execute('select state_json from player_game_contact_state where count_seq=1').fetchone()[0])
            payload['swing_prediction']['input'].pop('observation_history')
            db.execute('insert into player_game_contact_state values(10,3,1,?,NULL,?)',(probe.SOURCE,json.dumps(payload)))
            db.commit();db.close();before=hashlib.sha256(path.read_bytes()).hexdigest()
            out=root/'out';coverage=probe.run(path,10,10,out,heights=(.01,.03),levers=(.9,))
            self.assertEqual(coverage['swings'],3);self.assertEqual(coverage['valid_histories'],2)
            self.assertEqual(coverage['trial_rows'],20)
            status=csv_rows(out/'symmetric_height_status.csv')
            self.assertTrue(any(r['status']=='missing_observation_history' for r in status))
            summary=csv_rows(out/'symmetric_height_summary.csv')
            for scope,n in [('all_swings',2),('recorded_batting_only',1)]:
                rows=[r for r in summary if r['scope']==scope]
                self.assertEqual(sum(int(r['states']) for r in rows),n*10)
                for r in rows:self.assertEqual(sum(int(r[k]) for k in ('contact','no_contact','overlap_at_start','unresolved')),n)
                zeros=[r for r in rows if float(r['height_error_m'])==0]
                self.assertEqual(len(zeros),2)
                for k in ('avg_ev_ms','avg_vla_deg','contact','no_contact'):self.assertEqual(zeros[0][k],zeros[1][k])
            pairs=csv_rows(out/'symmetric_height_symmetry_pairs.csv')
            self.assertTrue(pairs)
            for r in pairs:
                if r['zero_vla_deg']:
                    self.assertAlmostEqual((float(r['triple_before_vla_deg'])+float(r['triple_after_vla_deg']))/2-float(r['zero_vla_deg']),float(r['center_shift_vla_deg']))
            checks=csv_rows(out/'symmetric_height_checks.csv')
            self.assertLess(max(float(r['max_error']) for r in checks),1e-8)
            self.assertEqual(before,hashlib.sha256(path.read_bytes()).hexdigest())
            with self.assertRaises(FileExistsError):probe.run(path,10,10,out)
            empty=probe.run(path,20,20,root/'empty');self.assertEqual(empty['trial_rows'],0)
            with self.assertRaises(ValueError):probe.run(path,10,10,root/'bad',heights=(0.,))

    def test_pair_does_not_hide_misses_or_invent_triples(self):
        s=state();frame=dict(point=s['input']['ball_position_world_m'],axis=(1,0,0),velocity=(0,35,0),nominal_time_sec=.4)
        contact=probe.trial(s,frame);miss=copy.deepcopy(contact);miss['collision_status']='not_evaluated:no_contact'
        g=Counter();probe.pair_add(g,contact,miss,contact)
        self.assertEqual(g['states'],1);self.assertEqual(g['paired_impulse_contacts'],0)
        g=Counter();probe.pair_add(g,contact,contact,miss)
        self.assertEqual(g['paired_impulse_contacts'],1);self.assertEqual(g['triple_impulse_contacts'],0)


if __name__=='__main__':unittest.main()
