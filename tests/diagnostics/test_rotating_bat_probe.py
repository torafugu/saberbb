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
import rotating_bat_probe as probe
from test_contact_state_probe import state


class RotatingBatTest(unittest.TestCase):
    def test_reference_anchor_derivative_and_delayed_orbit(self):
        for lever in (None,.6,.9,1.2):
            s=state();m=probe.Motion(s,lever)
            self.assertLess(max(probe.check_anchor(m).values()),1e-12)
            eps=1e-7
            before,_=m.pose(-eps,0);after,_=m.pose(eps,0)
            derivative=tuple((b-a)/(2*eps) for a,b in zip(before,after))
            self.assertLess(probe.norm(probe.sub(derivative,m.velocity)),1e-7)
            self.assertEqual(m.pose(.02,.003),m.pose(.017,0))
            if lever:
                for t in (-.06,0,.06):
                    p,axis=m.pose(t,0)
                    pivot=probe.add(m.pivot,probe.mul(m.pivot_velocity,t))
                    self.assertAlmostEqual(probe.norm(probe.sub(p,pivot)),lever)
                    self.assertAlmostEqual(probe.norm(axis),1)

    def test_linear_exact_entry_and_spatial_tolerance_convergence(self):
        s=state(0)
        s['input']['ball_position_world_m']=[0,0,0]
        s['input']['ball_velocity_world_ms']=[0,-40,0]
        s['bat_ideal_position_world_m']=[0,0,0]
        s['bat_axis_world']=[1,0,0]
        s['bat_velocity_world_ms']=[0,40,0]
        m=probe.Motion(s,None)
        loose=probe.search(m,.02,-.06,.06,tolerance=1e-6)
        tight=probe.search(m,.02,-.06,.06,tolerance=1e-8)
        exact=(40*.02-.07)/80
        self.assertEqual(loose['status'],'contact')
        self.assertLess(abs(tight['time_offset_sec']-exact),1e-8/80)
        self.assertLess(abs(tight['time_offset_sec']-exact),abs(loose['time_offset_sec']-exact))
        self.assertAlmostEqual(loose['normal'][1],1)
        self.assertEqual(probe.search(m,.5,-.01,.01)['status'],'no_contact')
        self.assertEqual(probe.search(m,0,0,.01)['status'],'overlap_at_start')
        self.assertEqual(probe.search(m,.02,-.06,.06,max_iterations=1)['status'],'unresolved')

    def test_curvature_can_change_contact_without_tightening_window(self):
        s=state(0)
        s['input']['ball_position_world_m']=[0,0,0]
        s['input']['ball_velocity_world_ms']=[0,-40,0]
        s['bat_ideal_position_world_m']=[0,0,0]
        s['bat_axis_world']=[1,0,0]
        s['bat_velocity_world_ms']=[0,40,0]
        for delay in (-.04,.04):
            self.assertEqual(probe.search(probe.Motion(s,None),delay,-.06,.06)['status'],'contact')
            self.assertEqual(probe.search(probe.Motion(s,.6),delay,-.06,.06)['status'],'no_contact')
        # Radius sensitivity is intentional; no configuration is treated as fitted truth.
        self.assertEqual(probe.search(probe.Motion(s,.6),.03,-.06,.06)['status'],'no_contact')
        self.assertEqual(probe.search(probe.Motion(s,1.2),.03,-.06,.06)['status'],'contact')

    def test_caps_mirror_and_speed_bound(self):
        s=state();m=probe.Motion(s,.9)
        for t in (-.02,.015):
            value=m.evaluate(t,m.delay)
            eps=1e-7
            derivative=abs((m.evaluate(t+eps,m.delay)['clearance_m']-
                            m.evaluate(t-eps,m.delay)['clearance_m'])/(2*eps))
            self.assertLessEqual(derivative,m.speed_bound+1e-5)
        mirrored=json.loads(json.dumps(s))
        for field in ('ball_position_world_m','ball_velocity_world_ms','aim_error_world_m'):
            mirrored['input'][field][0]*=-1
        for field in ('bat_ideal_position_world_m','bat_velocity_world_ms','bat_axis_world'):
            mirrored[field][0]*=-1
        mm=probe.Motion(mirrored,.9)
        for t in (-.03,0,.03):
            self.assertAlmostEqual(m.evaluate(t,.003)['clearance_m'],mm.evaluate(t,.003)['clearance_m'])
        linear=probe.Motion(s,None)
        linear.ball=probe.add(linear.point,probe.mul(linear.axis,.20))
        value=linear.evaluate(0,0)
        self.assertEqual(value['contact_region'],'cap')
        self.assertAlmostEqual(value['clearance_m'],.04-.07)
        # Zero motion has a definite non-contact result rather than dividing by zero.
        linear.ball_velocity=linear.velocity=(0,0,0);linear.speed_bound=0
        linear.ball=probe.add(linear.point,probe.mul(linear.axis,1))
        self.assertEqual(probe.search(linear,0,-.01,.01)['status'],'no_contact')

    def test_rotating_contact_matches_independent_dense_samples(self):
        # At t=0 the ideal reference point coincides with the ball.
        # Dense independent samples bracket a contact inside a narrow time interval.
        m=probe.Motion(state(0),.6)
        r=probe.search(m,0,-.06,.06)
        self.assertEqual(r['status'],'contact')
        self.assertLessEqual(abs(r['clearance_m']),1e-6)
        times=[-.06+i*.00005 for i in range(2401)]
        dense=[t for t in times if m.evaluate(t,0)['clearance_m']<=0]
        self.assertTrue(dense)
        self.assertLessEqual(r['time_offset_sec'],dense[0])
        self.assertLess(dense[0]-r['time_offset_sec'],.000051)

    def test_readonly_union_counts_metadata_and_invalid_controls(self):
        with tempfile.TemporaryDirectory() as folder:
            folder=Path(folder);path=folder/'db.sqlite'
            db=sqlite3.connect(path)
            db.execute('create table player_game_batting(game_id,count_seq,adjusted_contact_type,result,launch_speed,launch_angle,final_hla_deg)')
            db.executescript((ROOT/'migrations/upgrades/add_contact_state_shadow.sql').read_text())
            db.executemany('insert into player_game_batting values(10,?,?,?,46,10,0)',
                [(1,'SolidContact','Out'),(2,'SwungAndMiss','StrikeSwung'),(3,'WeakContact','Single'),(4,'MarginalContact','Foul')])
            for seq in (1,2,4,5):
                source='future' if seq==4 else probe.SOURCE
                db.execute('insert into player_game_contact_state values(10,?,1,?,NULL,?)',
                           (seq,source,json.dumps(state())))
            db.commit();db.close();before=hashlib.sha256(path.read_bytes()).hexdigest()
            output=folder/'reports'
            coverage=probe.run(path,10,10,output,levers=(.9,))
            self.assertEqual(coverage['swings'],5)
            self.assertEqual(coverage['valid_states'],3)
            self.assertEqual(coverage['trial_rows'],6)
            def read(name):
                with (output/name).open() as f: return list(csv.DictReader(f))
            self.assertEqual(len(read('rotating_contacts.csv')),6)
            allrows=[r for r in read('rotating_summary.csv') if r['timing_band']=='all']
            self.assertEqual(sum(int(r['valid_states']) for r in allrows),6)
            self.assertEqual(sum(int(r['swings']) for r in read('rotating_transitions.csv')),3)
            quality=read('contact_3d_quality.csv')
            for variant in ('linear_finite','rotating_r0.9'):
                expected=sum(int(r['impulse_contacts']) for r in allrows if r['variant']==variant)
                for dimension in ('axial_fraction','normal_speed_fraction','normal_elevation_deg','bat_point_speed_ms'):
                    self.assertEqual(sum(int(r['contacts']) for r in quality if r['variant']==variant
                        and r['adjusted_contact_type']=='all' and r['dimension']==dimension),expected)
            self.assertEqual(sum(int(r['states']) for r in read('contact_3d_status.csv')),6)
            self.assertTrue(any(r['adjusted_contact_type']=='UnrecordedBatting' for r in allrows))
            self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(),before)
            self.assertEqual(json.loads((output/'rotating_metadata.json').read_text())['levers_m'],[.9])
            with self.assertRaises(FileExistsError): probe.run(path,10,10,output)
            empty=probe.run(path,20,20,folder/'empty')
            self.assertEqual(empty['trial_rows'],0)
            self.assertEqual(sum(int(r['swings']) for r in read('rotating_state_status.csv')),5)
            for kw in (dict(levers=(.1,)),dict(levers=(.6,.6)),dict(window=float('nan')),
                       dict(tolerance=0),dict(max_iterations=0)):
                with self.assertRaises(ValueError): probe.run(path,10,10,folder/'bad',**kw)


if __name__=='__main__': unittest.main()
