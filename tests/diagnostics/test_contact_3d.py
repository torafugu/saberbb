import csv
import hashlib
import json
import math
import sys
import tempfile
import unittest
from dataclasses import replace
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools/diagnostics'))
from contact_3d import ContactGeometry, QualityAccumulator, solve_contact, quality_bands
from contact_collision_probe import BALL_MASS, collide, angles
import contact_3d_quality_probe as audit
import rotating_bat_probe as rotating
from test_contact_state_probe import state


def geometry(): return ContactGeometry(.4,(1,0,0),(0,1,0),0,0,.16,'side')


class ContactContractTest(unittest.TestCase):
    def test_head_on_momentum_restitution_and_common_vector(self):
        g=geometry();vin=(0,-40,0);bat=(0,40,0)
        q=solve_contact(g,vin,bat)
        expected=1.5*80/(1+BALL_MASS/.6)
        self.assertAlmostEqual(q['impulse_delta_ms'],expected)
        self.assertAlmostEqual(q['ev_ms'],expected-40)
        self.assertEqual(q['hla_deg'],0)
        self.assertEqual(q['vla_deg'],0)
        self.assertAlmostEqual(q['normal_impulse_kg_ms'],BALL_MASS*expected)
        self.assertEqual(q['normal_speed_fraction'],1)
        self.assertLess(q['restitution_residual_ms'],1e-12)
        self.assertLess(q['tangential_impulse_residual_ms'],1e-12)
        post_bat=40-BALL_MASS/.6*expected
        self.assertAlmostEqual(q['outgoing_world_ms'][1]-post_bat,40)
        self.assertAlmostEqual(BALL_MASS*vin[1]+.6*bat[1],BALL_MASS*q['outgoing_world_ms'][1]+.6*post_bat)
        self.assertEqual((q['ev_ms'],q['hla_deg'],q['vla_deg']),angles(q['outgoing_world_ms']))

    def test_old_impulse_compatibility_and_mirror(self):
        for z in (-.8,0,.8):
            n=(0,math.sqrt(1-z*z),z)
            g=replace(geometry(),normal_world=n)
            vin=(2,-35,-2);bat=(0,34,5)
            q=solve_contact(g,vin,bat)
            out,normal,delta=collide(vin,bat,n,.5,.6)
            self.assertEqual(q['outgoing_world_ms'],out)
            self.assertEqual(q['relative_normal_ms'],normal)
            self.assertEqual(q['impulse_delta_ms'],delta)
        g=replace(geometry(),axis_world=(0,0,1),normal_world=(.6,.8,0))
        q=solve_contact(g,(2,-35,-2),(0,34,5))
        mirrored=solve_contact(replace(g,normal_world=(-.6,.8,0)),(-2,-35,-2),(0,34,5))
        self.assertAlmostEqual(q['ev_ms'],mirrored['ev_ms'])
        self.assertAlmostEqual(q['hla_deg'],-mirrored['hla_deg'])
        self.assertAlmostEqual(q['vla_deg'],mirrored['vla_deg'])

    def test_non_approaching_and_invalid_geometry(self):
        q=solve_contact(geometry(),(0,50,0),(0,40,0))
        self.assertEqual(q['collision_status'],'non_approaching')
        self.assertIsNone(q['ev_ms'])
        self.assertIsNone(q['outgoing_world_ms'])
        for g in (replace(geometry(),normal_world=(0,2,0)),replace(geometry(),time_since_release_sec=-1),
                  replace(geometry(),normal_world=(.6,.8,0)),replace(geometry(),region='cap'),
                  replace(geometry(),axial_center_m=.2),replace(geometry(),closest_axial_m=.1)):
            with self.assertRaises(ValueError):solve_contact(g,(0,-40,0),(0,40,0))
        cap=replace(geometry(),axial_center_m=.2,closest_axial_m=.16,region='cap',normal_world=(.6,.8,0))
        solve_contact(cap,(0,-40,0),(0,40,0))
        with self.assertRaises(ValueError):solve_contact(replace(cap,normal_world=(-.6,.8,0)),(0,-40,0),(0,40,0))
        for args in ((float('nan'),.6),(.5,0),(.5,float('inf'))):
            with self.assertRaises(ValueError):solve_contact(geometry(),(0,-40,0),(0,40,0),*args)

    def test_zero_velocity_components_and_undefined_hla(self):
        g=replace(geometry(),normal_world=(0,0,1))
        q=solve_contact(g,(0,0,-40),(0,0,40))
        self.assertIsNone(q['hla_deg'])
        self.assertEqual(q['vla_deg'],90)
        row=dict(normal_x=0,normal_y=0,normal_z=1,axis_at_entry_x=1,axis_at_entry_y=0,axis_at_entry_z=0,
            point_velocity_x_ms=0,point_velocity_y_ms=0,point_velocity_z_ms=40,
            ball_velocity_x_ms=0,ball_velocity_y_ms=0,ball_velocity_z_ms=-40,
            axial_at_entry_m=0,closest_axial_at_entry_m=0,half_length_m=.16,entry_since_release_sec=.4,
            contact_region='side',relative_normal_ms=-80,impulse_delta_ms=q['impulse_delta_ms'],
            ev_ms=q['ev_ms'],hla_deg=None,vla_deg=90)
        r,source,checks=audit.audit_contact(row,dict(restitution=.5,effective_mass_kg=.6))
        self.assertEqual(source,'saved_incoming_vector')
        self.assertEqual(r['outgoing_world_ms'],q['outgoing_world_ms'])
        self.assertLess(max(checks.values()),1e-12)
        still=solve_contact(geometry(),(0,0,0),(0,0,0))
        self.assertIsNone(still['normal_speed_fraction'])
        self.assertEqual(still['collision_status'],'non_approaching')

    def test_no_legacy_label_or_timing_double_application(self):
        a=state();b=state()
        b['input']['legacy_timing_impact_x_m']=1e9
        b['input']['legacy_contact_offset_xz_m']=[1e9,-1e9]
        b['input']['legacy_timing_position_scale']=99
        rows=[]
        for s,kind in [(a,'SolidContact'),(b,'SwungAndMiss')]:
            rows.append(rotating.result_record(dict(game_id=1,count_seq=1,adjusted_contact_type=kind),s,
                rotating.Motion(s,.9),'rotating_r0.9',-.06,.06,1e-6,4000))
        for field in ('status','entry_time_offset_ms','ev_ms','hla_deg','vla_deg','outgoing_velocity_y_ms'):
            self.assertEqual(rows[0][field],rows[1][field])
        self.assertNotEqual(rows[0]['legacy_group'],rows[1]['legacy_group'])

    def test_stratified_denominators_and_csv_audit_versions(self):
        s=state();record=rotating.result_record(dict(game_id=1,count_seq=1,adjusted_contact_type='SolidContact'),s,
            rotating.Motion(s,.9),'rotating_r0.9',-.06,.06,1e-6,4000)
        metadata=dict(probe_version=2,contact_contract_version=1,state_source=rotating.SOURCE,restitution=.5,effective_mass_kg=.6,
            half_length_m=.16,effective_radius_m=.07)
        new,source,checks=audit.audit_contact(record,metadata)
        self.assertEqual(source,'saved_incoming_vector')
        self.assertLess(max(checks.values()),1e-10)
        old=dict(record)
        for field in list(old):
            if field.startswith('ball_velocity_') or field.startswith('outgoing_velocity_'):
                old[field]=None
        previous,source,checks=audit.audit_contact(old,dict(metadata,probe_version=1))
        self.assertEqual(source,'reconstructed_v1_incoming')
        self.assertAlmostEqual(new['ev_ms'],previous['ev_ms'])
        with tempfile.TemporaryDirectory() as folder:
            folder=Path(folder);input_path=folder/'contacts.csv';meta=folder/'metadata.json'
            with input_path.open('w',newline='') as stream:
                writer=csv.DictWriter(stream,rotating.FIELDS);writer.writeheader();writer.writerow(record)
                invalid=dict(record,count_seq=2,normal_y=2);writer.writerow(invalid)
                writer.writerow(dict(record,count_seq=3,status='no_contact'))
                writer.writerow(dict(record,count_seq=4,ev_ms=1))
            meta.write_text(json.dumps(metadata));before=hashlib.sha256(input_path.read_bytes()).hexdigest()
            counts=audit.run(input_path,meta,folder/'output')
            self.assertEqual(counts['rows'],4);self.assertEqual(counts['impulse_contact'],1)
            self.assertEqual(counts['invalid_contact_rows'],2);self.assertEqual(counts['not_contact_rows'],1)
            self.assertEqual(before,hashlib.sha256(input_path.read_bytes()).hexdigest())
            with (folder/'output/contact_3d_quality.csv').open() as stream:rows=list(csv.DictReader(stream))
            self.assertEqual(sum(int(r['contacts']) for r in rows if r['adjusted_contact_type']=='all'),4)
            self.assertEqual({r['dimension'] for r in rows},{'axial_fraction','normal_speed_fraction','normal_elevation_deg','bat_point_speed_ms'})
            with self.assertRaises(FileExistsError):audit.run(input_path,meta,folder/'output')
        self.assertEqual(quality_bands(dict(new,axial_fraction=1),'side')['axial_fraction'],'.75..1')
        self.assertEqual(quality_bands(new,'cap')['axial_fraction'],'cap')


if __name__=='__main__':unittest.main()
