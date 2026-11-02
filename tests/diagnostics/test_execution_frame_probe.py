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
import execution_frame_probe as probe
from contact_state_probe import SOURCE,validate_state
from rotating_bat_probe import Motion,check_anchor,result_record,summarize
from test_contact_state_probe import state


def snapshot(delta_deg,side=1,delay=.003):
    s=state(delay);angle=probe.ideal_angle_from_saved_height(s)+delta_deg
    s['input']['bat_angle_deg']=angle;s['input']['handedness_sign']=side
    s['bat_axis_world']=[side*math.cos(math.radians(angle)),0,math.sin(math.radians(angle))]
    return s


class ExecutionFrameTest(unittest.TestCase):
    def test_shared_pivot_analytic_displacement_and_actual_velocity(self):
        for side in (-1,1):
            for delta in (-5,0,5):
                s=snapshot(delta,side);validate_state(s)
                trials,record,checks=probe.frames(s,.9)
                self.assertLess(max(checks.values()),1e-12)
                ideal=trials[1][1];actual=trials[2][1]
                self.assertEqual(ideal.pivot,actual.pivot)
                self.assertEqual(ideal.pivot_velocity,actual.pivot_velocity)
                self.assertAlmostEqual(probe.norm(probe.sub(actual.point,ideal.point)),2*.9*abs(math.sin(math.radians(delta)/2)))
                self.assertLess(probe.norm(probe.sub(actual.velocity,s['bat_velocity_world_ms'])),1e-12)
                shift=probe.sub(actual.point,trials[0][1].point)
                for t in (-.04,.02):
                    original,_=trials[0][1].pose(t,.003);executed,_=actual.pose(t,.003)
                    self.assertLess(probe.norm(probe.sub(probe.sub(executed,original),shift)),1e-12)
                for model,motion in trials:
                    self.assertLess(max(check_anchor(motion).values()),1e-12)
                    dt=1e-7;p0,_=motion.pose(-dt,0);p1,_=motion.pose(dt,0)
                    derivative=probe.mul(probe.sub(p1,p0),1/(2*dt))
                    self.assertLess(probe.norm(probe.sub(derivative,motion.velocity)),1e-7)
                    p0,a0=motion.pose(.02,.003);p1,a1=motion.pose(.017,0)
                    self.assertLess(probe.norm(probe.sub(p0,p1)),1e-12)
                    self.assertLess(probe.norm(probe.sub(a0,a1)),1e-12)

    def test_zero_execution_error_reproduces_reanchored_control(self):
        for side in (-1,1):
            s=snapshot(0,side)
            trials,_,_=probe.frames(s,.9)
            for t in (-.06,0,.06):
                p0,a0=trials[0][1].pose(t,.003)
                for _,motion in trials[1:]:
                    p1,a1=motion.pose(t,.003)
                    self.assertLess(probe.norm(probe.sub(p0,p1)),1e-12)
                    self.assertLess(probe.norm(probe.sub(a0,a1)),1e-12)

    def test_mirror_and_saved_legacy_values_do_not_move_frame(self):
        right=snapshot(5);left=snapshot(5,-1)
        for field in ('ball_position_world_m','ball_velocity_world_ms','aim_error_world_m'):
            left['input'][field][0]*=-1
        for field in ('bat_ideal_position_world_m','bat_position_world_m','relative_position_world_m','relative_velocity_world_ms'):
            left[field][0]*=-1
        r,_,_=probe.frames(right,.9);l,_,_=probe.frames(left,.9)
        for (_,mr),(_,ml) in zip(r,l):
            for t in (-.01,.02):
                pr,ar=mr.pose(t,.003);pl,al=ml.pose(t,.003)
                self.assertAlmostEqual(pr[0],-pl[0]);self.assertAlmostEqual(pr[1],pl[1]);self.assertAlmostEqual(pr[2],pl[2])
                self.assertAlmostEqual(ar[0],-al[0]);self.assertAlmostEqual(ar[2],al[2])
        changed=json.loads(json.dumps(right));changed['input']['legacy_timing_impact_x_m']=100
        changed['input']['legacy_contact_offset_xz_m']=[100,-100]
        c,_,_=probe.frames(changed,.9)
        self.assertEqual(r[2][1].pose(.01,.003),c[2][1].pose(.01,.003))

    def test_summary_streaming_matches_existing_summary(self):
        s=snapshot(5);acc=probe.SummaryAccumulator();groups={}
        for frame,motion in probe.frames(s,.9)[0]:
            row=result_record(dict(game_id=1,count_seq=1,adjusted_contact_type='SolidContact'),s,motion,frame,-.06,.06,1e-6,4000)
            acc.add(row)
            for band in (row['timing_band'],'all'):groups['SolidContact',row['legacy_group'],band,frame]=[row]
        expected=summarize(groups);actual=acc.rows()
        self.assertEqual(len(expected),len(actual))
        for a,b in zip(expected,actual):
            for key,value in a.items():self.assertEqual(value,b[key],key)

    def test_db_readonly_paired_denominators_and_inputs(self):
        with tempfile.TemporaryDirectory() as folder:
            folder=Path(folder);path=folder/'test.db';db=sqlite3.connect(path)
            db.execute('create table player_game_batting(game_id,count_seq,adjusted_contact_type,result,launch_speed,launch_angle,final_hla_deg)')
            db.executescript((ROOT/'migrations/upgrades/add_contact_state_shadow.sql').read_text())
            for seq,kind in [(1,'SolidContact'),(2,'SwungAndMiss'),(3,'WeakContact')]:
                db.execute('insert into player_game_batting values(10,?,?,?,46,10,0)',(seq,kind,'Out'))
            for seq in (1,2,4,5):
                payload=snapshot(5);source=SOURCE if seq!=5 else 'future'
                db.execute('insert into player_game_contact_state values(10,?,1,?,NULL,?)',(seq,source,json.dumps(payload)))
            db.commit();db.close();before=hashlib.sha256(path.read_bytes()).hexdigest()
            out=folder/'out';coverage=probe.run(path,10,10,out,levers=(.9,))
            self.assertEqual(coverage['swings'],5);self.assertEqual(coverage['valid_states'],3)
            self.assertEqual(coverage['trial_rows'],9);self.assertEqual(coverage['input_rows'],3)
            def read(name):
                with (out/name).open() as stream:return list(csv.DictReader(stream))
            self.assertEqual(len(read('execution_frame_contacts.csv')),9)
            with (out/'execution_frame_inputs.jsonl').open() as f:inputs=[json.loads(x) for x in f]
            self.assertEqual(len(inputs),3);self.assertEqual(inputs[0]['input_model_version'],2)
            summary=[r for r in read('execution_frame_summary.csv') if r['timing_band']=='all']
            self.assertEqual(sum(int(r['valid_states']) for r in summary),9)
            self.assertEqual(sum(int(r['states']) for r in read('execution_frame_transitions.csv')),6)
            self.assertEqual(sum(int(r['states']) for r in read('execution_frame_angle_bands.csv')),9)
            self.assertEqual(sum(int(r['states']) for r in read('contact_3d_status.csv')),9)
            self.assertEqual(len(read('execution_frame_pairs.csv')),6)
            self.assertEqual(before,hashlib.sha256(path.read_bytes()).hexdigest())
            with self.assertRaises(FileExistsError):probe.run(path,10,10,out)
            empty=probe.run(path,20,20,folder/'empty')
            self.assertEqual(empty['trial_rows'],0)
            for kwargs in (dict(levers=(.1,)),dict(window=float('nan')),dict(levers=(.9,.9))):
                with self.assertRaises(ValueError):probe.run(path,10,10,folder/'bad',**kwargs)


if __name__=='__main__':unittest.main()
