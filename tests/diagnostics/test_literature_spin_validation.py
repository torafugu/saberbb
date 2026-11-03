import csv
import hashlib
import json
import math
import sqlite3
import tempfile
import unittest
from pathlib import Path

from test_forecast_contact_geometry import fixture, csv_rows
from bat_mass_presets import literature_wood_collision, OUNCE_KG, INCH_M
from contact_3d import ContactGeometry
from finite_barrel_collision import solve
from pitch_spin_source import reconstruct, PitchSpinSource
from contact_collision_probe import dot, cross
from rotating_bat_probe import norm, sub, mul
import finite_barrel_probe as finite
import literature_collision_probe as literature


class LiteratureSpinValidationTest(unittest.TestCase):
    def test_single_bat_and_sphere_preset_conversions_and_provenance(self):
        p=literature_wood_collision()
        self.assertAlmostEqual(p['bat_mass_kg'],29.41*OUNCE_KG)
        self.assertAlmostEqual(p['transverse_inertia_kg_m2'],2370*OUNCE_KG*INCH_M**2)
        self.assertAlmostEqual(p['axial_inertia_kg_m2'],9.1*OUNCE_KG*INCH_M**2)
        self.assertAlmostEqual(p['ball_mass_kg'],5.04*OUNCE_KG)
        self.assertAlmostEqual(p['ball_radius_m'],1.42*INCH_M)
        self.assertEqual(p['wood_fitted_tangent_e'],.464)
        self.assertIn('not a measured',p['provenance']['normal_e'])
        self.assertIn('not an independent',p['provenance']['tangent_e'])

    def test_uniform_mass_scaling_preserves_motion_and_scales_energy(self):
        g=ContactGeometry(.4,(1.,0.,0.),(0.,1.,0.),0.,0.,.16,'side')
        def trial(scale):
            return solve(g,(0.,-35.,-5.),(0.,30.,0.),(20.,30.,40.),(0.,0.,20.),
                .85*scale,.045*scale,.0002*scale,-.17,.03,.5,.5,.4,
                ball_mass=.143*scale,ball_radius=.036068)
        a,b=trial(1.),trial(2.)
        for k in ('outgoing_world_ms','outgoing_spin_rad_s'):
            self.assertLess(norm(sub(a[k],b[k])),1e-9)
        self.assertAlmostEqual(b['total_loss_j'],2*a['total_loss_j'])
        self.assertLess(max(b['checks'].values()),1e-7)

    def test_sphere_radius_changes_angular_velocity_not_impulse_for_fixed_inertia_factor(self):
        g=ContactGeometry(.4,(1.,0.,0.),(0.,1.,0.),0.,0.,.16,'side')
        def trial(radius):
            return solve(g,(0.,-35.,-5.),(0.,30.,0.),(0.,0.,0.),(0.,0.,20.),
                .85,.045,.0002,-.17,.03,.5,.5,.4,ball_mass=.143,ball_radius=radius)
        a,b=trial(.036),trial(.018)
        self.assertLess(norm(sub(a['outgoing_world_ms'],b['outgoing_world_ms'])),1e-9)
        self.assertLess(norm(sub(mul(a['outgoing_spin_rad_s'],2),b['outgoing_spin_rad_s'])),1e-8)
        self.assertAlmostEqual(a['ball_rotation_after_j'],b['ball_rotation_after_j'])
        with self.assertRaises(ValueError):trial(0.)

    def test_planar_reference_has_headon_limit_and_linear_speed_scaling(self):
        p=literature_wood_collision()
        head=literature.planar_reference(34.,0.,.5,.464,p)
        self.assertEqual(head['theta_deg'],0.);self.assertEqual(head['spin_rpm'],0.)
        expected=1.5*34/(1+p['ball_mass_kg']*(1/p['bat_mass_kg']+p['impact_from_cm_m']**2/p['transverse_inertia_kg_m2']))
        self.assertAlmostEqual(head['ev_ms'],expected)
        a=literature.planar_reference(28.,20.,.5,.464,p);b=literature.planar_reference(39.,20.,.5,.464,p)
        self.assertAlmostEqual(a['theta_deg'],b['theta_deg'])
        self.assertAlmostEqual(a['ev_ms']/28,b['ev_ms']/39)
        self.assertAlmostEqual(a['spin_rpm']/28,b['spin_rpm']/39)
        self.assertGreater(a['spin_rpm'],literature.planar_reference(28.,20.,.5,0.,p)['spin_rpm'])

    def test_literature_pipeline_labels_formalism_not_empirical_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            output=Path(tmp)/'out';result=literature.run(output)
            self.assertEqual(result['cases'],576);self.assertEqual(result['successful_cases'],576)
            self.assertEqual(result['empirical_validation'],'not_established')
            points=csv_rows(output/'literature_collision_points.csv')
            for r in points:
                self.assertIn('not_raw_observations',r['reference_kind'])
                self.assertGreaterEqual(float(r['full_total_ev_ms'])+1e-9,float(r['full_ev_ms']))
                self.assertGreaterEqual(float(r['full_total_spin_rpm'])+1e-9,abs(float(r['full_spin_rpm'])))
                scale=77*.44704/float(r['speed_ms'])
                self.assertAlmostEqual(float(r['reference_normalized_spin_rpm']),float(r['reference_spin_rpm'])*scale)
            summary=csv_rows(output/'literature_collision_summary.csv')
            self.assertTrue(all(r['empirical_validation_status'].startswith('not_established:') for r in summary))
            self.assertTrue(all(float(r['max_error'])<1e-7 for r in csv_rows(output/'literature_collision_checks.csv')))
            with self.assertRaises(FileExistsError):literature.run(output)

    def test_pitch_reconstruction_signs_efficiency_and_gyro_scenarios(self):
        omega=2000*math.pi/30;v=(0.,-40.,0.)
        back=reconstruct(2000,0.,.8,v);side=reconstruct(2000,90.,.8,v)
        self.assertAlmostEqual(back[0],-.8*omega);self.assertAlmostEqual(back[2],0.)
        self.assertAlmostEqual(side[2],.8*omega)
        for sign in (-1,1):
            full=reconstruct(2000,0.,.8,v,sign)
            self.assertAlmostEqual(norm(full),omega)
            self.assertAlmostEqual(dot(full,(0.,-1.,0.)),sign*.6*omega)
        tilted=(3.,-40.,-4.);unit=mul(tilted,1/norm(tilted))
        transverse=reconstruct(2000,40.,.8,tilted)
        self.assertAlmostEqual(dot(transverse,unit),0.)
        self.assertAlmostEqual(norm(transverse),omega*.8)
        self.assertAlmostEqual(norm(reconstruct(2000,40.,.8,tilted,1)),omega)

    def test_pitch_invalid_input_fails(self):
        for args in [(-1.,0.,1.,(0.,-40.,0.)),(2000.,0.,1.1,(0.,-40.,0.)),
                     (math.nan,0.,1.,(0.,-40.,0.)),(2000.,0.,1.,(0.,0.,0.))]:
            with self.assertRaises(ValueError):reconstruct(*args)

    def add_pitch_rows(self,dbpath,mode=None):
        with sqlite3.connect(dbpath) as db:
            db.execute('CREATE TABLE player_game_pitching(game_id,count_seq,spin_rate,spin_angle,spin_efficiency)')
            rows=[(10,1,2000.,0.,.8),(10,2,2500.,90.,.9)]
            if mode=='missing':rows.pop()
            elif mode=='duplicate':rows.append(rows[0])
            elif mode=='invalid':rows[0]=(10,1,-2000.,0.,.8)
            db.executemany('INSERT INTO player_game_pitching VALUES(?,?,?,?,?)',rows)

    def test_per_pitch_db_pipeline_is_readonly_and_records_reconstruction(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root);dbpath=root/'test.db';self.add_pitch_rows(dbpath)
            before=hashlib.sha256(dbpath.read_bytes()).hexdigest()
            output=root/'out';finite.run(source,output,pitch_db=dbpath,gyro_sign=1)
            self.assertEqual(before,hashlib.sha256(dbpath.read_bytes()).hexdigest())
            meta=json.loads((output/'finite_barrel_metadata.json').read_text())
            self.assertIsNone(meta['incoming_ball_spin_world_rad_s'])
            self.assertEqual(meta['pitch_spin_source']['unique_pitches'],2)
            self.assertEqual(meta['pitch_spin_source']['gyro_sign'],1)
            self.assertEqual(len(meta['pitch_spin_source']['used_scalar_rows_sha256']),64)
            examples=csv_rows(output/'finite_barrel_examples.csv')
            self.assertEqual({round(float(r['incoming_spin_rpm'])) for r in examples},{2000,2500})
            pairs=csv_rows(output/'finite_barrel_spin_pairs.csv')
            self.assertTrue(pairs)
            stats=csv_rows(output/'finite_barrel_spin_delta_stats.csv')
            self.assertTrue(stats)
            lookup={(x['scope'],x['variant'],x['after_model'].split(':')[0]):x
                    for x in pairs if x['stage']=='overall'}
            for x in stats:
                if x['cohort']!='all_common':continue
                pair=lookup[x['scope'],x['variant'],x['model']]
                self.assertEqual(int(x['paired_contacts']),int(pair['paired_contacts']))
                self.assertAlmostEqual(float(x['mean_delta']),float(pair['avg_delta_'+x['metric']]))
            joint=csv_rows(output/'finite_barrel_spin_ev_vla.csv')
            totals={}
            for x in joint:
                key=tuple(x[k] for k in ['scope','variant','cohort','model','side'])
                totals[key]=totals.get(key,0)+int(x['count'])
            for x in stats:
                if x['metric']!='ev_ms':continue
                key=tuple(x[k] for k in ['scope','variant','cohort','model'])
                for side in ('zero','input_spin'):
                    self.assertEqual(totals[key+(side,)],int(x['paired_contacts']))
            self.assertTrue(any(abs(float(r['avg_delta_spin_rpm']))>1 for r in pairs if int(r['n_spin_rpm'])))
            for r in pairs:
                self.assertLessEqual(int(r['paired_contacts']),int(r['states']))
                if int(r['n_ev_ms']):self.assertAlmostEqual(float(r['avg_after_ev_ms'])-float(r['avg_before_ev_ms']),float(r['avg_delta_ev_ms']))
            self.assertTrue(all(float(r['max_error'])<1e-7 for r in csv_rows(output/'finite_barrel_checks.csv')))

    def test_missing_duplicate_invalid_pitch_and_ambiguous_sources_fail(self):
        for mode in ('missing','duplicate','invalid'):
            with tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);source=fixture(root);dbpath=root/'test.db';self.add_pitch_rows(dbpath,mode)
                with self.assertRaises(ValueError):finite.run(source,root/'out',pitch_db=dbpath)
        with self.assertRaises(ValueError):finite.run('missing','unused',gyro_sign=1)
        with self.assertRaises(ValueError):finite.run('missing','unused',pitch_db='db',ball_spin=(1.,0.,0.))

    def test_pitch_source_read_snapshot_and_row_hash_are_stable(self):
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'db.sqlite';self.add_pitch_rows(path)
            row=dict(game_id=10,count_seq=1,ball_velocity_x_ms=0.,ball_velocity_y_ms=-40.,ball_velocity_z_ms=0.)
            source=PitchSpinSource(path)
            try:
                first=source.get(row);meta=source.metadata();second=source.get(row)
                self.assertEqual(first,second);self.assertEqual(meta,source.metadata())
                with self.assertRaises(sqlite3.OperationalError):source.db.execute('DELETE FROM player_game_pitching')
            finally:source.close()


if __name__=='__main__':unittest.main()
