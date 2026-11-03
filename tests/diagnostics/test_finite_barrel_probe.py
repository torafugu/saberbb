import hashlib
import json
import math
import random
import tempfile
import unittest
from collections import defaultdict
from pathlib import Path

from test_forecast_contact_geometry import fixture, csv_rows, replace_csv
from test_vibration_profile import contact_row
import bat_vibration_profile as profile
import finite_barrel_probe as probe
from finite_barrel_collision import solve, inertia_apply
from contact_3d import ContactGeometry
from contact_spin import solve_side
from contact_collision_probe import dot
from rotating_bat_probe import norm, sub, rotate


class FiniteBarrelTest(unittest.TestCase):
    def geometry(self,s=.08,angle=0.):
        return ContactGeometry(.4,(1.,0.,0.),(0.,math.cos(angle),math.sin(angle)),s,s,.16,'side')

    def solve(self,g=None,vin=(8.,-40.,-8.),vbat=(0.,35.,0.),spin=(0.,0.,0.),omega=(0.,0.,20.),
              radius=.034,ia=.00015,e=.5,mu=.2,et=0.):
        return solve(g or self.geometry(),vin,vbat,spin,omega,profile.MASS_KG,
            profile.MASS_KG*profile.GYRATION_M**2,ia,profile.CM_Z_M-profile.REFERENCE_Z_M,radius,e,mu,et)

    def test_zero_radius_matches_centerline_including_energy_and_spinning_sphere(self):
        for mu in (0.,.001,.2,10.):
            for et in (0.,.4):
                g=self.geometry();spin=(100.,30.,-20.)
                a=self.solve(g,spin=spin,radius=0.,mu=mu,et=et)
                b=solve_side(g,(8.,-40.,-8.),(0.,35.,0.),spin,(0.,0.,20.),
                    profile.MASS_KG,profile.MASS_KG*profile.GYRATION_M**2,
                    profile.CM_Z_M-profile.REFERENCE_Z_M,.5,mu,et)
                for field in ('ev_ms','spin_rpm','normal_impulse_kg_ms','tangent_impulse_kg_ms','total_loss_j','system_before_j','system_after_j'):
                    self.assertAlmostEqual(a[field],b[field],places=9)
                self.assertLess(norm(sub(a['outgoing_world_ms'],b['outgoing_world_ms'])),1e-9)
                self.assertLess(norm(sub(a['outgoing_spin_rad_s'],b['outgoing_spin_rad_s'])),1e-9)

    def test_frictionless_finite_radius_matches_normal_centerline(self):
        for radius in (0.,.01,.034):
            for ia in (.00015,.0003):
                a=self.solve(mu=0.,radius=radius,ia=ia,spin=(100.,30.,-20.))
                b=self.solve(mu=0.,radius=0.,spin=(100.,30.,-20.))
                self.assertLess(norm(sub(a['outgoing_world_ms'],b['outgoing_world_ms'])),1e-9)
                self.assertEqual(a['outgoing_spin_rad_s'],(100.,30.,-20.))
                self.assertLess(max(a['checks'].values()),1e-7)

    def test_surface_velocity_and_normal_tangent_coupling_are_active(self):
        finite=self.solve();line=self.solve(radius=0.)
        self.assertNotEqual(finite['surface_speed_ms'],finite['centerline_speed_ms'])
        self.assertNotEqual(finite['normal_axial_coupling_kg_inv'],0.)
        self.assertNotAlmostEqual(finite['normal_impulse_kg_ms'],line['normal_impulse_kg_ms'])
        self.assertLess(finite['checks']['normal_restitution_ms'],1e-7)
        self.assertLess(finite['checks']['matrix_response_ms'],1e-7)

    def test_torsion_changes_with_axial_inertia_and_preserves_angular_momentum(self):
        a=self.solve(ia=.00015);b=self.solve(ia=.0003)
        self.assertNotEqual(a['bat_axial_spin_after_rad_s'],0.)
        self.assertNotAlmostEqual(a['spin_rpm'],b['spin_rpm'])
        for m in (a,b):
            self.assertLess(m['checks']['total_angular_impulse'],1e-7)
            self.assertAlmostEqual(m['system_before_j']-m['system_after_j'],m['total_loss_j'])

    def test_target_and_cone_limited_regimes(self):
        a=self.solve(mu=10.,et=.4);b=self.solve(mu=.001,et=.4)
        self.assertEqual(a['tangent_regime'],'restitution_target')
        self.assertLess(a['tangent_target_residual_ms'],1e-7)
        self.assertEqual(b['tangent_regime'],'coulomb_limited')
        self.assertAlmostEqual(b['tangent_impulse_kg_ms'],.001*b['normal_impulse_kg_ms'])
        self.assertGreater(b['tangent_target_residual_ms'],0.)

    def test_energy_increasing_coupled_restitution_target_is_explicitly_unavailable(self):
        m=self.solve(vin=(.32,-30.,3.),vbat=(0.,30.,3.),e=1.,mu=10.,et=0.)
        self.assertEqual(m['collision_status'],'not_evaluated:energy_increasing_target')
        self.assertGreater(m['candidate_energy_gain_j'],0.)
        self.assertIsNone(m['ev_ms'])

    def test_randomized_rotated_contacts_conservation_and_nonnegative_loss(self):
        rng=random.Random(20261103)
        for _ in range(300):
            theta=rng.uniform(-math.pi,math.pi);g=self.geometry(rng.uniform(-.15,.12),theta)
            n=g.normal_world;vbat=(0.,30.,3.)
            vin=tuple(v-rng.uniform(5,50)*x for v,x in zip(vbat,n))
            m=self.solve(g,vin,vbat,tuple(rng.uniform(-200,200) for _ in range(3)),
                (0.,-5.,20.),ia=rng.choice((.00015,.0003)),e=rng.uniform(.2,.5),
                mu=rng.choice((0.,.001,.2,10.)),et=rng.choice((0.,.4)))
            self.assertEqual(m['collision_status'],'impulse_contact')
            self.assertLess(max(m['checks'].values()),1e-7)
            self.assertGreaterEqual(m['total_loss_j'],0.)

    def test_full_world_rotation_covariance(self):
        rotation=tuple(.63*x/math.sqrt(14) for x in (1.,2.,3.))
        rot=lambda v:rotate(v,rotation,1.)
        g=self.geometry();vin=(8.,-40.,-8.);vbat=(0.,35.,0.)
        spin=(30.,-40.,20.);omega=(0.,0.,20.)
        a=self.solve(g,vin,vbat,spin,omega,et=.4)
        rotated=ContactGeometry(.4,rot(g.axis_world),rot(g.normal_world),.08,.08,.16,'side')
        b=self.solve(rotated,rot(vin),rot(vbat),rot(spin),rot(omega),et=.4)
        for key in ('outgoing_world_ms','outgoing_spin_rad_s','bat_omega_after_rad_s'):
            self.assertLess(norm(sub(rot(a[key]),b[key])),1e-8)
        self.assertAlmostEqual(a['ev_ms'],b['ev_ms'])
        self.assertAlmostEqual(a['total_loss_j'],b['total_loss_j'])
        self.assertLess(max(b['checks'].values()),1e-7)

    def test_inertia_tensor_inverse_and_axial_spin_input(self):
        axis=(0.,1.,0.);v=(3.,4.,5.)
        restored=inertia_apply(inertia_apply(v,axis,.04,.0002),axis,25.,5000.)
        self.assertLess(norm(sub(restored,v)),1e-9)
        m=self.solve(omega=(50.,0.,20.))
        self.assertLess(max(m['checks'].values()),1e-7)
        self.assertNotEqual(m['bat_axial_spin_after_rad_s'],50.)

    def test_invalid_parameters_nonapproach_and_caps(self):
        for kwargs in ({'radius':-.1},{'ia':0.},{'ia':math.nan},{'mu':-1.},{'et':1.1},{'spin':(0.,0.,math.nan)}):
            with self.assertRaises(ValueError):self.solve(**kwargs)
        self.assertEqual(self.solve(vin=(0.,40.,0.))['collision_status'],'non_approaching')
        g=ContactGeometry(.4,(1.,0.,0.),(1.,0.,0.),.18,.16,.16,'cap')
        with self.assertRaises(ValueError):self.solve(g)
        for r in (contact_row(cap=True),contact_row(.15)):
            results,reason=probe.event(r,defaultdict(float),probe.configs((.00015,.0003)),(0.,0.,0.),.9)
            self.assertTrue(reason.startswith('not_evaluated:'))
            self.assertTrue(all(m['ev_ms'] is None for m in results.values()))

    def test_pipeline_integrity_denominators_pairs_and_metadata(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root)
            hashes={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()}
            out=root/'out';coverage=probe.run(source,out,sample_limit=2)
            self.assertEqual(coverage['trial_rows'],6);self.assertEqual(coverage['models'],8)
            ss=csv_rows(out/'finite_barrel_summary.csv');ps=csv_rows(out/'finite_barrel_pairs.csv')
            self.assertEqual(sum(int(r['states']) for r in csv_rows(out/'finite_barrel_status.csv')),6*8)
            for r in ss:
                self.assertEqual(int(r['states']),int(r['impulse_contacts'])+int(r['unavailable_states']))
                self.assertLessEqual(int(r['fair_contacts']),int(r['impulse_contacts']))
            overall=[r for r in ss if r['stage']=='overall']
            selection=csv_rows(out/'finite_barrel_selection_summary.csv')
            audit=csv_rows(out/'finite_barrel_selection_coverage.csv')
            self.assertEqual(sum(int(r['trial_rows']) for r in audit if r['scope']=='all_swings'),6*8)
            for r in overall:
                selected=[x for x in selection if all(x[k]==r[k] for k in ('scope','variant','model'))]
                success=[x for x in selected if x['stage']=='model_success' and x['outcome_basis']=='input_spin_model']
                self.assertEqual(sum(int(x['contacts']) for x in success),int(r['impulse_contacts']))
                fair=[x for x in selected if x['stage']=='model_fair' and x['outcome_basis']=='input_spin_model']
                self.assertEqual(sum(int(x['contacts']) for x in fair),int(r['fair_contacts']))
                source_all=[x for x in selected if x['stage']=='all_geometry_contacts' and x['outcome_basis']=='source_constant_mass_normal']
                source_reasons=[x for x in selected if x['stage'].startswith('reason:') and x['outcome_basis']=='source_constant_mass_normal']
                self.assertEqual(sum(int(x['contacts']) for x in source_all),sum(int(x['contacts']) for x in source_reasons))
            self.assertEqual(sum(int(r['states']) for r in overall if r['scope']=='all_swings'),6*8)
            for r in overall:
                for stage in ('source_z','source_z_reference_direction','baseline_impact'):
                    bins=[b for b in ss if b['stage']==stage and all(b[k]==r[k] for k in ('scope','variant','model'))]
                    self.assertEqual(sum(int(b['states']) for b in bins),int(r['states']))
            for r in ps:
                self.assertLessEqual(int(r['paired_contacts']),int(r['states']))
                for k in probe.PAIR_METRICS:
                    if int(r['n_'+k]):self.assertAlmostEqual(float(r['avg_after_'+k])-float(r['avg_before_'+k]),float(r['avg_delta_'+k]))
                if r['after_model']=='finite_mu0' and int(r['n_ev_ms']):self.assertAlmostEqual(float(r['avg_delta_ev_ms']),0.)
            self.assertTrue(all(float(r['max_error'])<1e-7 for r in csv_rows(out/'finite_barrel_checks.csv')))
            self.assertLessEqual(len(csv_rows(out/'finite_barrel_examples.csv')),2)
            meta=json.loads((out/'finite_barrel_metadata.json').read_text())
            tilt=csv_rows(out/'finite_barrel_tilt_outcomes.csv')
            for r in overall:
                xs=[x for x in tilt if x['scope']==r['scope'] and x['variant']==r['variant'] and x['outcome_basis']==r['model'] and x['collision_status']=='impulse_contact']
                for stage in ('overall','tilt','tilt_impact','tilt_impact_normal_z'):
                    ys=[x for x in xs if x['stratum']==stage]
                    self.assertEqual(sum(int(x['contacts']) for x in ys),int(r['impulse_contacts']))
                    self.assertEqual(sum(int(x['contacts']) for x in ys if x['direction']=='fair_direction'),int(r['fair_contacts']))
            self.assertTrue(all(float(x['max_error'])<1e-8 for x in csv_rows(out/'finite_barrel_tilt_checks.csv')))
            horizontal=csv_rows(out/'finite_barrel_horizontal_summary.csv')
            for r in overall:
                xs=[x for x in horizontal if x['scope']==r['scope'] and x['variant']==r['variant'] and x['outcome_basis']==r['model'] and x['stratum']=='overall']
                self.assertEqual(sum(int(x['contacts']) for x in xs),int(r['impulse_contacts']))
                self.assertEqual(sum(int(x['contacts']) for x in xs if x['final_direction']=='fair_direction'),int(r['fair_contacts']))
                for x in xs:
                    for axis in 'xyz':
                        self.assertAlmostEqual(float(x['avg_outgoing_'+axis]),sum(float(x['avg_'+name+'_'+axis]) for name in ('incoming','normal_delta','tangent_delta')))
            self.assertTrue(all(float(x['max_error'])<1e-7 for x in csv_rows(out/'finite_barrel_horizontal_checks.csv')))
            transitions=csv_rows(out/'finite_barrel_observation_transitions.csv')
            source_meta=json.loads((source/'observation_update_metadata.json').read_text())
            expected=2*len(source_meta['sensor_sigma_m'])*int(coverage['trial_rows'])/(len(source_meta['sensor_sigma_m'])+2)
            overall_transitions=[x for x in transitions if x['scope']=='all_swings' and x['stratum']=='overall']
            self.assertEqual(sum(int(x['paired_trials']) for x in overall_transitions),expected)
            direction_summary=csv_rows(out/'finite_barrel_direction_summary.csv')
            for r in overall:
                xs=[x for x in direction_summary if x['scope']==r['scope'] and x['variant']==r['variant'] and x['outcome_basis']==r['model'] and x['stratum']=='overall']
                self.assertEqual(sum(int(x['contacts']) for x in xs),int(r['impulse_contacts']))
                self.assertEqual(sum(int(x['contacts']) for x in xs if x['direction']=='fair_direction'),int(r['fair_contacts']))
            self.assertEqual(meta['incoming_ball_spin_world_rad_s'],[0.,0.,0.])
            self.assertEqual(hashes,{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()})
            with self.assertRaises(FileExistsError):probe.run(source,out)

    def test_corrupt_duplicate_missing_sources_and_bad_cli_parameters_fail(self):
        for mode in ('duplicate','missing','corrupt'):
            with tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);source=fixture(root);path=source/'observation_update_contacts.csv';rs=csv_rows(path)
                if mode=='duplicate':rs.append(rs[0])
                elif mode=='missing':rs.pop()
                else:next(r for r in rs if r['status']=='contact')['outgoing_velocity_y_ms']='100'
                replace_csv(path,rs)
                with self.assertRaises(ValueError):probe.run(source,root/'out')
        for kwargs in ({'inertias':(.00015,.00015)},{'height':0.},{'sample_limit':-1}):
            with self.assertRaises(ValueError):probe.run('missing','unused',**kwargs)


if __name__=='__main__':unittest.main()
