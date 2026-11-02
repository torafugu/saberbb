import copy
import json
import math
import tempfile
import unittest
from pathlib import Path

from test_forecast_contact_geometry import fixture, csv_rows, replace_csv
import effective_mass_probe as probe
from contact_recoil import inverse_mass, solve, recoil_checks
from contact_3d import ContactGeometry, solve_contact
from bat_mass_presets import PRESET, measured_wood


class EffectiveMassTest(unittest.TestCase):
    def test_measured_preset_units_axis_and_reference_sign(self):
        p=measured_wood();m=p['body_mass_kg'];c=p['cm_offset_from_reference_m'];k=p['gyration_radius_m']
        self.assertAlmostEqual(m,.83375947510625)
        self.assertAlmostEqual(p['transverse_cm_inertia_kg_m2'],.043347248664200255)
        self.assertAlmostEqual(c,-.17272)
        self.assertAlmostEqual(k,.2280134035681508)
        self.assertAlmostEqual(m*k*k,p['transverse_cm_inertia_kg_m2'])
        self.assertAlmostEqual(p['published_radius_at_impact_m'],.029464)
        a=(1.,0.,0.);n=(0.,1.,0.)
        inv,arm,i=inverse_mass(a,n,0.,.9,'free',m,k,c)
        # At the mapped published impact, not the CM: 1/Meff=1/M+b^2/I0.
        self.assertAlmostEqual(arm[0],.17272)
        self.assertAlmostEqual(1/inv,.5297729719775672)
        near,_,_=inverse_mass(a,n,-.1,.9,'free',m,k,c)
        far,_,_=inverse_mass(a,n,.1,.9,'free',m,k,c)
        self.assertLess(near,inv);self.assertGreater(far,inv)
        _,_,ip=inverse_mass(a,n,0.,.9,'pivot',m,k,c)
        self.assertAlmostEqual(ip,.4843528158068796)
        for model in ('free','pivot'):
            inv,arm,i=inverse_mass(a,n,.1,.9,model,m,k,c)
            g=ContactGeometry(.4,a,n,.1,.1,.16,'side')
            vin=(0.,-40.,-2.);vbat=(0.,35.,5.)
            checks=recoil_checks(solve(g,vin,vbat,inv),vin,vbat,(0.,-3.,20.),n,arm,i,model,m)
            self.assertLess(max(checks.values()),1e-10)

    def test_measured_pipeline_keeps_events_baseline_and_provenance(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root)
            hashes={p.name:probe.digest(p) for p in source.iterdir()}
            before=probe.run(source,root/'synthetic')
            after=probe.run(source,root/'measured',preset=PRESET)
            self.assertEqual(before['mass_models'],7);self.assertEqual(after['mass_models'],3)
            self.assertEqual(after['collision_trial_rows'],18)
            self.assertEqual(before['geometry_contacts'],after['geometry_contacts'])
            old=csv_rows(root/'synthetic/effective_mass_summary.csv')
            new=csv_rows(root/'measured/effective_mass_summary.csv')
            self.assertEqual([r for r in old if r['mass_model']=='constant_0.6kg'],[r for r in new if r['mass_model']=='constant_0.6kg'])
            meta=json.loads((root/'measured/effective_mass_metadata.json').read_text())
            self.assertEqual(meta['bat_preset'],measured_wood())
            self.assertEqual(meta['model_names'],['constant_0.6kg','free_'+PRESET,'pivot_'+PRESET])
            self.assertEqual(meta['input_sha256'],{name:hashes[name] for name in (
                'observation_update_contacts.csv','observation_update_metadata.json','observation_update_coverage.csv')})
            self.assertEqual(hashes,{p.name:probe.digest(p) for p in source.iterdir()})
            self.assertTrue(all(float(r['max_error'])<1e-7 for r in csv_rows(root/'measured/effective_mass_checks.csv')))

    def test_preset_rejects_mixed_parameters_before_writing(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            for kwargs in (dict(body_mass=.6),dict(cm_offset=0.),dict(gyrations=(.25,))):
                with self.assertRaisesRegex(ValueError,'preset_cannot_be_combined'):
                    probe.run(root/'missing',root/'out',preset=PRESET,**kwargs)
            with self.assertRaisesRegex(ValueError,'unknown_bat_preset'):
                probe.run(root/'missing',root/'out',preset='unknown')
            self.assertFalse((root/'out').exists())

    def test_free_center_and_offcenter_parallel_axis_and_cap(self):
        a=(1.,0.,0.);n=(0.,1.,0.)
        center,_,i=inverse_mass(a,n,0.,.9,'free',.6,.25)
        self.assertAlmostEqual(center,1/.6);self.assertAlmostEqual(i,.6*.25**2)
        off,r,_=inverse_mass(a,n,.16,.9,'free',.6,.25)
        self.assertAlmostEqual(off,1/.6+.16**2/i)
        pivot,rp,ip=inverse_mass(a,n,.16,.9,'pivot',.6,.25)
        self.assertAlmostEqual(ip,i+.6*.9**2)
        self.assertAlmostEqual(pivot,1.06**2/ip)
        cap,_,_=inverse_mass(a,a,.16,.9,'pivot',.6,.25)
        self.assertEqual(cap,0)
        # Physical center-of-mass offset and swing lever are different coordinates.
        shifted,_,_=inverse_mass(a,n,-.1,.9,'free',.6,.25,cm_offset=-.1)
        self.assertAlmostEqual(shifted,1/.6)

    def test_scalar_solver_matches_existing_contract_and_infinite_limit(self):
        g=ContactGeometry(.4,(1.,0.,0.),(0.,1.,0.),0.,0.,.16,'side')
        vin=(0.,-40.,-2.);vbat=(0.,35.,5.)
        expected=solve_contact(g,vin,vbat)
        actual=solve(g,vin,vbat,1/.6)
        for k in ('ev_ms','hla_deg','vla_deg','impulse_delta_ms'):self.assertAlmostEqual(actual[k],expected[k])
        limit=solve(g,vin,vbat,0.)
        self.assertEqual(limit['infinite_effective_mass'],1);self.assertIsNone(limit['effective_mass_kg'])
        self.assertAlmostEqual(limit['impulse_delta_ms'],1.5*75)
        self.assertEqual(solve(g,(0,40,0),(0,0,0),1/.6)['collision_status'],'non_approaching')
        with self.assertRaises(ValueError):solve(g,vin,vbat,-1.)

    def test_momentum_angular_impulse_energy_and_restitution(self):
        a=(1.,0.,0.);n=(0.,.8,.6);vin=(2.,-40.,-3.);vbat=(0.,30.,5.);omega=(0.,-3.,20.)
        g=ContactGeometry(.4,a,n,.1,.1,.16,'side')
        for model in ('free','pivot'):
            inv,arm,inertia=inverse_mass(a,n,.1,.9,model)
            m=solve(g,vin,vbat,inv)
            checks=recoil_checks(m,vin,vbat,omega,n,arm,inertia,model)
            self.assertLess(max(checks.values()),1e-10)
        # Pure axial cap: zero rotational compliance at a prescribed pivot.
        g=ContactGeometry(.4,a,a,.2,.16,.16,'cap');vin=(-40.,0.,0.);vbat=(0.,0.,0.)
        inv,arm,i=inverse_mass(a,a,.16,.9,'pivot')
        self.assertEqual(inv,0.)
        checks=recoil_checks(solve(g,vin,vbat,inv),vin,vbat,(0.,0.,0.),a,arm,i,'pivot')
        self.assertLess(max(checks.values()),1e-10)

    def test_restitution_range_and_fair_direction_selection(self):
        a=(1.,0.,0.);n=(0.,1.,0.);g=ContactGeometry(.4,a,n,0.,0.,.16,'side')
        vin=(0.,-40.,0.);vbat=(0.,35.,0.)
        baseline=solve(g,vin,vbat,1/.6)
        small=solve(g,vin,vbat,1/.01);smaller=solve(g,vin,vbat,1/.001)
        self.assertEqual(probe.direction(baseline),'fair_direction')
        self.assertEqual(probe.direction(small),'foul_direction')
        self.assertGreater(smaller['ev_ms'],small['ev_ms'])
        r=dict(variant='test',adjusted_contact_type='SolidContact',status='contact',contact_region='side',
               closest_axial_at_entry_m=0.,local_abs_b_over_r=0.,normal_speed_fraction=1.,forecast_error_z_m=0.)
        agg=probe.Aggregate();agg.add(r,{'constant_0.6kg':baseline,'small':small})
        self.assertEqual(agg.summary['all_swings','test','small','overall','all','all']['fair_contacts'],0)
        self.assertEqual(agg.pairs['all_swings','test','small','overall','all','all']['paired_fair_contacts'],0)
        for restitution in (0.,.5,1.):
            for model in ('free','pivot'):
                inv,arm,i=inverse_mass(a,n,.16,.9,model)
                event=ContactGeometry(.4,a,n,.16,.16,.16,'side')
                m=solve(event,vin,vbat,inv,restitution)
                checks=recoil_checks(m,vin,vbat,(0.,0.,20.),n,arm,i,model,restitution=restitution)
                self.assertLess(max(checks.values()),1e-10)

    def test_frozen_event_pipeline_baseline_denominators_readonly(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root)
            hashes={p.name:probe.digest(p) for p in source.iterdir()}
            c=probe.run(source,root/'out',gyrations=(.25,))
            self.assertEqual(c['trial_rows'],6);self.assertEqual(c['mass_models'],3)
            self.assertEqual(c['collision_trial_rows'],18)
            self.assertEqual(hashes,{p.name:probe.digest(p) for p in source.iterdir()})
            summary=csv_rows(root/'out/effective_mass_summary.csv')
            for scope,n in [('all_swings',2),('recorded_batting_only',1)]:
                rs=[r for r in summary if r['scope']==scope and r['stage']=='overall']
                self.assertEqual(len(rs),9)
                self.assertTrue(all(int(r['states'])==n for r in rs))
                for r in rs:
                    baseline=next(x for x in rs if x['variant']==r['variant'] and x['mass_model']=='constant_0.6kg')
                    self.assertEqual(r['geometry_contacts'],baseline['geometry_contacts'])
                    self.assertEqual(r['impulse_contacts'],baseline['impulse_contacts'])
            distribution=csv_rows(root/'out/effective_mass_distribution.csv')
            self.assertEqual(sum(int(r['states']) for r in distribution if r['scope']=='all_swings'),18)
            checks=csv_rows(root/'out/effective_mass_checks.csv')
            self.assertLess(max(float(r['max_error']) for r in checks),1e-7)
            with self.assertRaises(FileExistsError):probe.run(source,root/'out')

    def test_duplicates_missing_and_corruption_rejected(self):
        for kind in ('duplicate','missing','bad_velocity'):
            with self.subTest(kind=kind),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);source=fixture(root);path=source/'observation_update_contacts.csv';rows=csv_rows(path)
                if kind=='duplicate':rows.append(rows[0])
                elif kind=='missing':rows.pop()
                else:
                    r=next(r for r in rows if r['collision_status']=='impulse_contact');r['outgoing_velocity_z_ms']='100'
                replace_csv(path,rows)
                with self.assertRaises(ValueError):probe.run(source,root/'out')


if __name__=='__main__':unittest.main()
