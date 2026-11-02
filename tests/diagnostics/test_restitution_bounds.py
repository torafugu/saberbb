import json
import hashlib
import math
import random
from collections import defaultdict
import tempfile
import unittest
from pathlib import Path

from contact_collision_probe import BALL_MASS, dot
from restitution_bounds import Interval, analyze
from forecast_contact_geometry import audit
import restitution_bounds_probe as probe
from test_forecast_contact_geometry import fixture, csv_rows, replace_csv


class RestitutionBoundsTest(unittest.TestCase):
    def test_strict_intersections_and_empty_tangent(self):
        self.assertIsNone(Interval(0.,.5).intersect(Interval(.5,1.,True)))
        self.assertTrue(Interval(.5,.5).nonempty())
        self.assertFalse(Interval(.5,.5,True).nonempty())
        # The upper allowed e only reaches the foul line, never the fair interior.
        r=analyze((10.,-20.,0.),(0.,0.,0.),(0.,1.,0.),0.)
        self.assertEqual(r['diagnosis'],'no_fair_in_range')
        self.assertEqual(r['baseline_direction'],'foul_direction')
        # Tangential speed exactly equals the strict EV threshold: no weak interval.
        r=analyze((25.,-120.,0.),(0.,-20.,0.),(0.,1.,0.),0.)
        self.assertEqual(r['diagnosis'],'tangent_floor_ge_threshold')
        self.assertIsNone(r['weak_e_lo'])

    def test_e0_weak_and_interior_infimum_not_attained(self):
        r=analyze((0.,-40.,0.),(0.,35.,0.),(0.,1.,0.),1/.53)
        self.assertEqual(r['e0_fair'],1);self.assertEqual(r['weak_fair_possible'],1)
        self.assertEqual(r['fair_min_attained'],1);self.assertEqual(r['fair_infimum_e'],0.)
        self.assertEqual(r['weak_e_lo'],0.);self.assertFalse(r['weak_e_lo_open'])
        self.assertTrue(r['weak_e_hi_open']);self.assertEqual(r['baseline_weak_fair'],0)
        # EV decreases then increases with e. At e=1/3, horizontal speed is zero;
        # fair e>1/3 approaches EV=0, but the zero-vector endpoint is not fair.
        r=analyze((0.,-40.,0.),(0.,-10.,0.),(0.,1.,0.),0.)
        self.assertAlmostEqual(r['min_any_e'],1/3)
        self.assertAlmostEqual(r['fair_ev_infimum_ms'],0.)
        self.assertEqual(r['fair_min_attained'],0);self.assertTrue(r['fair_e_lo_open'])
        self.assertEqual(r['e0_direction'],'foul_direction')
        self.assertLess(r['min_any_ev_ms'],r['e0_ev_ms'])

    def test_impossibility_reasons_are_distinct(self):
        cases=[((100.,-40.,0.),(0.,35.,0.),1/.6,'no_fair_in_range'),
               ((30.,-40.,0.),(0.,35.,0.),1/.6,'tangent_floor_ge_threshold'),
               ((0.,-40.,0.),(0.,60.,0.),0.,'normal_response_floor_ge_threshold'),
               ((24.,-120.,0.),(0.,-20.,0.),0.,'fair_constraint_blocks_weak')]
        for vin,vbat,inv,diagnosis in cases:
            with self.subTest(diagnosis=diagnosis):
                r=analyze(vin,vbat,(0.,1.,0.),inv)
                self.assertEqual(r['diagnosis'],diagnosis)
                self.assertEqual(r['weak_fair_possible'],0)
        for inv in (-1.,math.inf):
            with self.assertRaises(ValueError):analyze((0,-40,0),(0,35,0),(0,1,0),inv)
        with self.assertRaises(ValueError):analyze((0,40,0),(0,35,0),(0,1,0),1.)

    def test_narrow_weak_interval_is_not_lost_to_e_grid(self):
        r=analyze((0.,-120.,24.999999),(0.,-20.,0.),(0.,1.,0.),0.)
        self.assertEqual(r['weak_fair_possible'],1)
        self.assertGreater(r['weak_e_lo'],.19)
        self.assertLess(r['weak_e_hi']-r['weak_e_lo'],.0001)
        self.assertTrue(r['weak_e_lo_open']);self.assertTrue(r['weak_e_hi_open'])

    def test_analytic_intervals_against_independent_dense_vectors(self):
        rng=random.Random(20261102)
        for _ in range(100):
            n=tuple(rng.uniform(-1,1) for _ in range(3));length=math.sqrt(dot(n,n));n=tuple(x/length for x in n)
            vin=tuple(rng.uniform(-40,40) for _ in range(3));speed=rng.uniform(1,80)
            vbat=tuple(v+speed*x for v,x in zip(vin,n));inv=rng.uniform(0,8)
            r=analyze(vin,vbat,n,inv)
            fair=Interval(*(r['fair_e_'+k] for k in ('lo','hi','lo_open','hi_open'))) if r['fair_exists'] else None
            weak=Interval(*(r['weak_e_'+k] for k in ('lo','hi','lo_open','hi_open'))) if r['weak_fair_possible'] else None
            grid_min=math.inf
            for i in range(1001):
                e=i/2000;un=sum((v-b)*x for v,b,x in zip(vin,vbat,n))
                delta=-(1+e)*un/(1+BALL_MASS*inv)
                out=tuple(v+delta*x for v,x in zip(vin,n));ev=math.sqrt(sum(x*x for x in out))
                is_fair=out[1]>abs(out[0]);is_weak=is_fair and ev<25
                self.assertEqual(bool(fair and fair.contains(e)),is_fair)
                self.assertEqual(bool(weak and weak.contains(e)),is_weak)
                grid_min=min(grid_min,ev)
            self.assertLessEqual(r['min_any_ev_ms'],grid_min+1e-10)
            self.assertLess(grid_min-r['min_any_ev_ms'],.05)

    def test_pipeline_readonly_scopes_and_interval_denominators(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root)
            hashes={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()}
            result=probe.run(source,root/'out')
            self.assertEqual(result['trial_rows'],6)
            contacts=csv_rows(root/'out/restitution_bounds_contacts.csv')
            self.assertEqual(len(contacts),6)
            summary=csv_rows(root/'out/restitution_bounds_summary.csv')
            diagnoses=csv_rows(root/'out/restitution_bounds_diagnoses.csv')
            for scope,n in [('all_swings',2),('recorded_batting_only',1)]:
                rs=[r for r in summary if r['scope']==scope and r['stage']=='overall']
                self.assertEqual(len(rs),3)
                for r in rs:
                    self.assertEqual(int(r['states']),n)
                    self.assertEqual(sum(int(x['states']) for x in diagnoses if all(x[k]==r[k] for k in probe.KEYS)),n)
                    self.assertEqual(int(r['fair_exists_contacts']),int(r['fair_min_attained_contacts'])+int(r['fair_infimum_only_contacts']))
            for r in contacts:
                if r['search_status']!='contact':
                    self.assertTrue(r['diagnosis'].startswith('not_evaluated:'))
                    self.assertEqual(r['e0_ev_ms'],'');self.assertEqual(r['weak_fair_possible'],'')
            meta=json.loads((root/'out/restitution_bounds_metadata.json').read_text())
            self.assertEqual(meta['restitution_range'],[0,.5]);self.assertEqual(meta['bat_preset']['preset'],'kensrud2017_wood')
            self.assertEqual(meta['input_sha256'],probe.ContactSource(source).hashes)
            self.assertEqual(hashes,{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()})
            self.assertTrue(all(float(r['max_error'])<1e-7 for r in csv_rows(root/'out/restitution_bounds_checks.csv')))
            with self.assertRaises(FileExistsError):probe.run(source,root/'out')

    def test_duplicate_missing_and_corrupt_inputs_rejected(self):
        for kind in ('duplicate','missing','corrupt'):
            with self.subTest(kind=kind),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);source=fixture(root);path=source/'observation_update_contacts.csv';rs=csv_rows(path)
                if kind=='duplicate':rs.append(rs[0])
                elif kind=='missing':rs.pop()
                else:next(r for r in rs if r['status']=='contact')['outgoing_velocity_y_ms']='100'
                replace_csv(path,rs)
                with self.assertRaises(ValueError):probe.run(source,root/'out')

    def test_noncontact_and_unresolved_are_unevaluated_not_impossible(self):
        with tempfile.TemporaryDirectory() as tmp:
            source=fixture(Path(tmp));raw=csv_rows(source/'observation_update_contacts.csv')[0]
            for status in ('no_contact','unresolved','overlap_at_start'):
                raw.update(status=status,collision_status='not_evaluated:'+status,contact_region='',ev_ms='',hla_deg='',vla_deg='')
                checks=defaultdict(float);r=audit(raw,checks);m=probe.event(r,probe.measured_wood(),checks)
                self.assertEqual(m['diagnosis'],'not_evaluated:'+status)
                self.assertTrue(all(v is None for k,v in m.items() if k!='diagnosis'))


if __name__=='__main__':unittest.main()
