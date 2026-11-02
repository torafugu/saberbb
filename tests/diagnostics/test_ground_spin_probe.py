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
import ground_spin_probe as probe
import bat_vibration_profile as profile
from contact_3d import ContactGeometry
from contact_recoil import inverse_mass, solve
from contact_spin import solve_side, first_ground, ground_impact, BALL_INERTIA, BALL_RADIUS_M
from contact_collision_probe import dot
from rotating_bat_probe import norm


class GroundSpinTest(unittest.TestCase):
    def geometry(self):
        return ContactGeometry(.4,(1.,0.,0.),(0.,1.,0.),.03,.03,.16,'side')

    def spin(self,g=None,vin=(8.,-40.,-8.),vbat=(0.,35.,0.),omega=(0.,0.,0.),batomega=(0.,0.,20.),mu=.2,et=0.,e=.5):
        return solve_side(g or self.geometry(),vin,vbat,omega,batomega,profile.MASS_KG,
            profile.MASS_KG*profile.GYRATION_M**2,profile.CM_Z_M-profile.REFERENCE_Z_M,e,mu,et)

    def test_frictionless_matches_existing_normal_solver_and_keeps_incident_spin(self):
        g=self.geometry();omega=(100.,30.,-20.)
        m=self.spin(mu=0.,omega=omega)
        inv,_,_=inverse_mass(g.axis_world,g.normal_world,.03,.9,'free',profile.MASS_KG,profile.GYRATION_M,profile.CM_Z_M-profile.REFERENCE_Z_M)
        base=solve(g,(8.,-40.,-8.),(0.,35.,0.),inv)
        for a,b in zip(m['outgoing_world_ms'],base['outgoing_world_ms']):self.assertAlmostEqual(a,b)
        self.assertEqual(m['outgoing_spin_rad_s'],omega)
        self.assertEqual(m['tangent_loss_j'],0.)
        self.assertLess(max(m['checks'].values()),1e-7)

    def test_spin_generated_with_equal_opposite_angular_impulse_and_energy_loss(self):
        m=self.spin(vin=(0.,-40.,-8.))
        self.assertGreater(m['spin_rpm'],0)
        self.assertGreater(m['ball_rotation_after_j'],0)
        self.assertGreater(m['total_loss_j'],0)
        self.assertAlmostEqual(m['system_before_j']-m['system_after_j'],m['total_loss_j'])
        self.assertLess(max(m['checks'].values()),1e-7)
        # The sign follows r_ball=-R*n and Jt opposite incoming -Z slip.
        self.assertLess(m['outgoing_spin_rad_s'][0],0)

    def test_tangent_rebound_enhances_spin_and_coulomb_cap_limits_it(self):
        a=self.spin(mu=10.,et=0.)
        b=self.spin(mu=10.,et=.4)
        self.assertGreater(b['spin_rpm'],a['spin_rpm'])
        self.assertEqual(a['tangent_regime'],'restitution_target')
        c=self.spin(mu=.001,et=.4)
        self.assertEqual(c['tangent_regime'],'coulomb_limited')
        self.assertAlmostEqual(c['tangent_impulse_kg_ms'],.001*c['normal_impulse_kg_ms'])
        self.assertGreaterEqual(c['tangent_loss_j'],0)

    def test_randomized_conservation_in_rotated_side_frames(self):
        rng=random.Random(9123)
        for _ in range(250):
            angle=rng.uniform(-math.pi,math.pi)
            n=(0.,math.cos(angle),math.sin(angle));axis=(1.,0.,0.)
            g=ContactGeometry(.4,axis,n,0.,0.,.16,'side')
            approach=rng.uniform(1.,100.)
            vbat=(0.,20.,5.);vin=tuple(v-approach*x for v,x in zip(vbat,n))
            vin=(rng.uniform(-30.,30.),vin[1],vin[2])
            m=self.spin(g,vin,vbat,tuple(rng.uniform(-300,300) for _ in range(3)),
                (0.,-10.,20.),mu=rng.choice((0.,.001,.2,10.)),et=rng.choice((0.,.4,1.)),e=rng.random())
            self.assertLess(max(m['checks'].values()),1e-7)
            self.assertGreaterEqual(m['total_loss_j'],-1e-10)

    def test_high_ev_downward_can_be_slow_or_fast_after_ground(self):
        def flight(angle):
            a=math.radians(angle)
            return first_ground((0.,45*math.cos(a),45*math.sin(a)),(0.,0.,0.))
        steep=flight(-80);shallow=flight(-10)
        self.assertLess(steep['post_ground_horizontal_ms'],25)
        self.assertGreater(shallow['post_ground_horizontal_ms'],25)
        self.assertLess(steep['first_ground_distance_m'],shallow['first_ground_distance_m'])
        self.assertGreater(steep['rebound_height_m'],10)  # slow horizontal does not imply a low bounce
        self.assertLess(steep['ground_energy_balance_j'],1e-7)
        self.assertLess(steep['flight_energy_balance_j'],1e-7)

    def test_spin_can_feed_horizontal_speed_without_gaining_total_energy(self):
        no_spin=ground_impact((0.,10.,-10.),(0.,0.,0.))
        topspin=ground_impact((0.,10.,-10.),(-600.,0.,0.))
        self.assertGreater(topspin['velocity'][1],no_spin['velocity'][1])
        self.assertLess(topspin['energy_balance_j'],1e-7)
        self.assertGreaterEqual(topspin['loss_j'],0)

    def test_flight_is_exact_in_gravity_only_model(self):
        for vz in (-1000.,-40.,0.,40.):
            m=first_ground((3.,4.,vz),(20.,-40.,3.),.9,0.,0.)
            t=m['first_ground_time_sec']
            self.assertAlmostEqual(.9+vz*t-.5*9.80665*t*t,0.,places=8)
            self.assertEqual(m['post_ground_horizontal_ms'],5.)
            self.assertAlmostEqual(m['rebound_height_m'],0.)
            self.assertLess(m['ground_energy_balance_j'],1e-7)

    def test_invalid_nonapproaching_cap_and_missing_inputs(self):
        for kwargs in ({'mu':-1},{'et':1.1},{'e':math.nan},{'omega':(math.nan,0,0)}):
            with self.assertRaises(ValueError):self.spin(**kwargs)
        self.assertEqual(self.spin(vin=(0.,40.,0.))['collision_status'],'non_approaching')
        cap=ContactGeometry(.4,(1.,0.,0.),(1.,0.,0.),.18,.16,.16,'cap')
        with self.assertRaises(ValueError):self.spin(g=cap)
        for h in (0.,-1.,math.nan):
            with self.assertRaises(ValueError):first_ground((0.,1.,-1.),(0.,0.,0.),h)
        r=contact_row(cap=True)
        results,_=probe.event(r,defaultdict(float),(0.,0.,0.))
        for n,_,_ in probe.SPIN_MODELS:self.assertEqual(results[n]['collision_status'],'not_evaluated:unsupported_cap')

    def test_pipeline_readonly_count_closure_metadata_and_common_pairs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root)
            hashes={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()}
            out=root/'out';coverage=probe.run(source,out)
            self.assertEqual(coverage['trial_rows'],6)
            statuses=csv_rows(out/'ground_spin_status.csv')
            self.assertEqual(sum(int(r['states']) for r in statuses),6*len(probe.MODELS))
            rows=csv_rows(out/'ground_spin_summary.csv')
            self.assertEqual(sum(int(r['states']) for r in rows if r['scope']=='all_swings' and r['stage']=='overall'),6*len(probe.MODELS)*3)
            for r in rows:
                self.assertEqual(int(r['states']),int(r['evaluated_contacts'])+int(r['unavailable_states']))
                self.assertLessEqual(int(r['fair_high_ev_downward_slow_contacts']),int(r['fair_contacts']))
                if r['stage']=='overall':
                    bins=[b for b in rows if b['stage']=='ev_vla' and all(b[k]==r[k] for k in ('scope','variant','model','height_m'))]
                    self.assertEqual(sum(int(b['states']) for b in bins),int(r['states']))
            for r in csv_rows(out/'ground_spin_pairs.csv'):
                self.assertEqual(sum(int(r[k]) for k in ('slow_to_slow','slow_to_fast','fast_to_slow','fast_to_fast')),int(r['paired_contacts']))
            self.assertTrue(all(float(r['max_error'])<1e-7 for r in csv_rows(out/'ground_spin_checks.csv')))
            meta=json.loads((out/'ground_spin_metadata.json').read_text())
            self.assertEqual(meta['incoming_ball_spin_world_rad_s'],[0.,0.,0.])
            self.assertEqual(meta['heights_m'],[.6,.9,1.2])
            self.assertEqual(hashes,{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()})
            with self.assertRaises(FileExistsError):probe.run(source,out)

    def test_examples_and_undefined_launch_direction_are_written(self):
        agg=probe.Aggregate(2);r=contact_row();r.update(game_id=1,count_seq=2)
        angle=math.radians(-80)
        m=dict(collision_status='impulse_contact',outgoing_world_ms=(0.,45*math.cos(angle),45*math.sin(angle)),
            outgoing_spin_rad_s=(0.,0.,0.),ev_ms=45.,hla_deg=0.,vla_deg=-80.)
        zero=dict(collision_status='impulse_contact',outgoing_world_ms=(0.,0.,0.),
            outgoing_spin_rad_s=(0.,0.,0.),ev_ms=0.,hla_deg=None,vla_deg=None)
        checks=defaultdict(float)
        agg.add(r,{'test':m,'zero':zero},(.9,),.45,.25,25.,checks)
        self.assertEqual(len(agg.samples),1)
        self.assertLess(max(checks.values()),1e-7)
        with tempfile.TemporaryDirectory() as tmp:
            agg.write(Path(tmp))
            samples=csv_rows(Path(tmp)/'ground_spin_examples.csv')
            self.assertEqual(len(samples),1)
            self.assertEqual(float(samples[0]['ev_ms']),45.)
            rows=csv_rows(Path(tmp)/'ground_spin_summary.csv')
            high=next(x for x in rows if x['scope']=='all_swings' and x['stage']=='overall' and x['model']=='test')
            self.assertEqual(high['fair_high_ev_downward_slow_contacts'],'1')

    def test_bad_source_and_parameters_fail_closed(self):
        for mode in ('duplicate','missing','corrupt'):
            with tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);source=fixture(root);path=source/'observation_update_contacts.csv';rows=csv_rows(path)
                if mode=='duplicate':rows.append(rows[0])
                elif mode=='missing':rows.pop()
                else:next(r for r in rows if r['status']=='contact')['outgoing_velocity_y_ms']='1234'
                replace_csv(path,rows)
                with self.assertRaises(ValueError):probe.run(source,root/'out')
        with self.assertRaises(ValueError):probe.run('missing','unused',heights=(.9,.9))


if __name__=='__main__':unittest.main()
