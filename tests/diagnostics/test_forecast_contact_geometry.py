import csv
import math
from collections import defaultdict
import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from test_swing_prediction_probe import ROOT
from test_observation_update_probe import input_with_history
from test_contact_state_probe import state
import observation_update_probe as update
import forecast_contact_geometry as geometry
import sqlite3


def fixture(root):
    dbpath=root/'test.db';db=sqlite3.connect(dbpath)
    db.execute('create table player_game_batting(game_id,count_seq,adjusted_contact_type,result,launch_speed,launch_angle,final_hla_deg)')
    db.executescript((ROOT/'migrations/upgrades/add_contact_state_shadow.sql').read_text())
    from test_swing_prediction_probe import prediction
    for seq in (1,2):
        if seq==1:db.execute("insert into player_game_batting values(10,1,'SolidContact','Out',46,10,0)")
        payload=state()
        shift=[.25-.02,0,1.0875-.9]
        payload['input']['ball_position_world_m']=[.25,0,1.0875]
        payload['input']['reference_time_sec_since_release']=.39
        for key in ('bat_position_world_m','bat_ideal_position_world_m'):
            payload[key]=[a+b for a,b in zip(payload[key],shift)]
        payload['swing_prediction']=prediction();payload['swing_prediction']['input']=input_with_history()
        db.execute('insert into player_game_contact_state values(10,?,1,?,NULL,?)',(seq,update.SOURCE,json.dumps(payload)))
    db.commit();db.close()
    source=root/'source';update.run(dbpath,10,10,source,sigmas=(.005,),levers=(.9,))
    return source


def csv_rows(path):
    with path.open(newline='') as f:return list(csv.DictReader(f))


def replace_csv(path,rows):
    with path.open('w',newline='') as f:
        w=csv.DictWriter(f,fieldnames=list(rows[0]));w.writeheader();w.writerows(rows)


class ForecastGeometryTest(unittest.TestCase):
    def test_local_cylinder_geometry_and_sign(self):
        # Axis X, incoming relative motion -Y: tangent is +Z.
        self.assertAlmostEqual(geometry.local_impact((1,0,0),(0,.8,.6),(0,-80,0),'side'),.6)
        self.assertAlmostEqual(geometry.local_impact((1,0,0),(0,.8,-.6),(0,-80,0),'side'),-.6)
        self.assertIsNone(geometry.local_impact((1,0,0),(1,0,0),(0,-80,0),'cap'))
        self.assertIsNone(geometry.local_impact((1,0,0),(0,1,0),(80,0,0),'side'))
        self.assertEqual(geometry.signed_band(1e-16),'zero_1e-8')

    def test_cap_and_signed_vertical_impulse_reconstruction(self):
        with tempfile.TemporaryDirectory() as tmp:
            source=fixture(Path(tmp));template=next(r for r in csv_rows(source/'observation_update_contacts.csv') if r['collision_status']=='impulse_contact')
            def record(axis,normal,vin,vbat,axial,closest,region):
                raw=dict(template)
                contact=geometry.ContactGeometry(.39,axis,normal,axial,closest,.16,region)
                metrics=geometry.solve_contact(contact,vin,vbat)
                raw.update(status='contact',collision_status=metrics['collision_status'],contact_region=region,
                    axial_at_entry_m=str(axial),closest_axial_at_entry_m=str(closest),entry_since_release_sec='.39',
                    entry_time_offset_ms='0',min_evaluated_clearance_m='0')
                for k in geometry.CONTACT_SCALARS:
                    if k in metrics:raw[k]=str(metrics[k])
                for prefix,value in [('axis_at_entry_',axis),('normal_',normal),('ball_velocity_',vin),
                                     ('point_velocity_',vbat),('outgoing_velocity_',metrics['outgoing_world_ms'])]:
                    if value is not None:
                        for a,v in zip('xyz',value):raw[prefix+a+('_ms' if 'velocity' in prefix else '')]=str(v)
                for k in ('ev_ms','hla_deg','vla_deg'):raw[k]='' if metrics[k] is None else str(metrics[k])
                return geometry.audit(raw,defaultdict(float))
            cap=record((1,0,0),(1,0,0),(-40,0,-2),(35,0,5),.2,.16,'cap')
            self.assertIsNone(cap['local_abs_b_over_r']);self.assertEqual(cap['signed_axial_fraction'],1)
            up=record((1,0,0),(0,.8,.6),(0,-40,0),(0,35,0),0,0,'side')
            down=record((1,0,0),(0,.8,-.6),(0,-40,0),(0,35,0),0,0,'side')
            self.assertAlmostEqual(up['local_abs_b_over_r'],.6)
            self.assertAlmostEqual(up['outgoing_z_ms'],-down['outgoing_z_ms'])
            self.assertGreater(up['vla_deg'],0);self.assertLess(down['vla_deg'],0)
            away=record((1,0,0),(0,1,0),(0,40,0),(0,-35,0),0,0,'side')
            self.assertEqual(away['collision_status'],'non_approaching');self.assertIsNone(away['ev_ms'])

    def test_integration_readonly_closure_scopes_and_pairs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root)
            hashes={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()}
            coverage=geometry.run(source,root/'out')
            self.assertEqual(coverage,dict(trial_rows=6,ball_lever_keys=2,paired_trials=4,conditions=3))
            chain=csv_rows(root/'out/forecast_geometry_chain.csv')
            for scope,n in [('all_swings',2),('recorded_batting_only',1)]:
                overall=[r for r in chain if r['scope']==scope and r['stage']=='overall']
                self.assertEqual(len(overall),3)
                for row in overall:self.assertEqual(int(row['states']),n)
                for row in overall:
                    for stage in ('forecast_x_z','forecast_time','forecast_z_sign','error_to_impact'):
                        self.assertEqual(sum(int(r['states']) for r in chain if r['scope']==scope and r['variant']==row['variant'] and r['stage']==stage),n)
            checks=csv_rows(root/'out/forecast_geometry_checks.csv')
            self.assertTrue(checks);self.assertLess(max(float(r['max_error']) for r in checks),1e-7)
            transition=csv_rows(root/'out/forecast_geometry_transitions.csv')
            self.assertEqual(sum(int(r['states']) for r in transition if r['adjusted_contact_type']=='all'),4)
            self.assertEqual(hashes,{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()})
            with self.assertRaises(FileExistsError):geometry.run(source,root/'out')

    def test_duplicate_missing_pair_and_corrupt_vector_rejected(self):
        for change in ('duplicate','missing','corrupt'):
            with self.subTest(change=change),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);source=fixture(root);path=source/'observation_update_contacts.csv';rows=csv_rows(path)
                if change=='duplicate':rows.append(rows[0])
                elif change=='missing':rows.pop()
                else:
                    row=next(r for r in rows if r['collision_status']=='impulse_contact')
                    row['outgoing_velocity_z_ms']=str(float(row['outgoing_velocity_z_ms'])+1)
                replace_csv(path,rows)
                with self.assertRaises(ValueError):geometry.run(source,root/'out')

    def test_no_contact_has_no_fabricated_impact_or_quality(self):
        with tempfile.TemporaryDirectory() as tmp:
            source=fixture(Path(tmp));raw=csv_rows(source/'observation_update_contacts.csv')[0]
            raw.update(status='no_contact',collision_status='not_evaluated:no_contact',ev_ms='',hla_deg='',vla_deg='',contact_region='')
            row=geometry.audit(raw,{})
            self.assertIsNone(row['normal_z']);self.assertIsNone(row['local_abs_b_over_r'])
            a=geometry.Aggregates();a.add(row);a.write(Path(tmp))
            overall=next(r for r in csv_rows(Path(tmp)/'forecast_geometry_chain.csv') if r['stage']=='overall')
            self.assertEqual(overall['impulse_contacts'],'0');self.assertEqual(overall['avg_vla_deg'],'')


if __name__=='__main__':unittest.main()
