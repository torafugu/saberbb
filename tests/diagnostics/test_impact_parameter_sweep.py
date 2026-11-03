import csv
import math
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_swing_prediction_probe import ROOT
import impact_parameter_sweep as probe


def read(root,name):
    with (root/f'impact_sweep_{name}.csv').open() as f:return list(csv.DictReader(f))


class ImpactSweepTest(unittest.TestCase):
    def test_local_normal_and_normal_control_analytic(self):
        axis=(1.,0.,0.);relative=(0.,-73.,0.)
        for b in (-.999,-.5,0,.5,.999):
            n=probe.normal_for_b(axis,relative,b)
            self.assertAlmostEqual(n[1],math.sqrt(1-b*b));self.assertAlmostEqual(n[2],b)
            self.assertAlmostEqual(probe.dot(axis,n),0)
        n=probe.normal_for_b(axis,relative,0)
        from collections import defaultdict
        checks=defaultdict(float)
        r=probe.responses(axis,n,0.,(0.,-38.,0.),(0.,35.,0.),(0.,0.,35/.9),(0.,0.,0.),.9,checks)
        # Independent 1D recoil formula at the reference point.
        p=probe.profile;inv=1/p.MASS_KG+(p.CM_Z_M-p.REFERENCE_Z_M)**2/(p.MASS_KG*p.GYRATION_M**2)
        expected=-38+1.5*73/(1+.145*inv)
        self.assertAlmostEqual(r['normal_profile']['ev_ms'],expected)
        for field in ('ev_ms','hla_deg','vla_deg'):
            self.assertAlmostEqual(r['normal_profile'][field],r['finite_mu0'][field])

    def test_grid_counts_minima_and_intervals_no_search_or_db(self):
        with tempfile.TemporaryDirectory() as tmp:
            out=Path(tmp)/'out'
            with patch('sqlite3.connect',side_effect=AssertionError('no DB')),patch('rotating_bat_probe.search',side_effect=AssertionError('no search')):
                result=probe.run(out,tilts=(0.,30.),axials=(0.,),samples=101)
            self.assertEqual(result['response_rows'],606)
            rows=read(out,'samples');summaries=read(out,'summary');mins=read(out,'minima');intervals=read(out,'weak_fair_intervals')
            for s in summaries:
                cells=[r for r in rows if all(r[k]==s[k] for k in probe.KEYS)]
                self.assertEqual(len(cells),101)
                for name,condition in [('successful_samples',lambda r:r['collision_status']=='impulse_contact'),
                    ('fair_samples',lambda r:r['direction']=='fair_direction'),
                    ('weak_samples',lambda r:r['ev_ms'] and float(r['ev_ms'])<25),
                    ('weak_fair_samples',lambda r:r['direction']=='fair_direction' and float(r['ev_ms'])<25)]:
                    self.assertEqual(sum(bool(condition(r)) for r in cells),int(s[name]))
                for cohort in ['all_successful','fair_direction']:
                    subset=[r for r in cells if r['collision_status']=='impulse_contact' and (cohort=='all_successful' or r['direction']==cohort)]
                    m=[r for r in mins if all(r[k]==s[k] for k in probe.KEYS) and r['cohort']==cohort]
                    if subset:self.assertAlmostEqual(float(m[0]['ev_ms']),min(float(r['ev_ms']) for r in subset))
                own=[r for r in intervals if all(r[k]==s[k] for k in probe.KEYS)]
                self.assertEqual(sum(int(r['grid_samples']) for r in own),int(s['weak_fair_samples']))
            with self.assertRaises(FileExistsError):probe.run(out,samples=3)

    def test_profile_unavailable_and_invalid_parameters(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);probe.run(root/'outside',tilts=(0.,),axials=(.15,),samples=3)
            self.assertTrue(all(r['collision_status']=='not_evaluated:out_of_curve' for r in read(root/'outside','samples')))
            for kw in [dict(samples=4),dict(samples=1),dict(b_limit=1.),dict(tilts=(90.,)),dict(axials=(.2,)),dict(spin_efficiency=2.)]:
                with self.subTest(kw=kw),self.assertRaises(ValueError):probe.run(root/'bad',**kw)
                self.assertFalse((root/'bad').exists())

    def test_weak_fair_intervals_are_nonempty_and_cover_exact_grid_points(self):
        with tempfile.TemporaryDirectory() as tmp:
            out=Path(tmp)/'slow';probe.run(out,tilts=(0.,),axials=(.1,),samples=101,bat_speed=20.)
            rows=read(out,'samples');intervals=read(out,'weak_fair_intervals')
            model=probe.MODELS[2]
            weak=[r for r in rows if r['model']==model and r['direction']=='fair_direction' and float(r['ev_ms'])<25]
            own=[r for r in intervals if r['model']==model]
            self.assertGreater(len(weak),0)
            self.assertEqual(sum(int(r['grid_samples']) for r in own),len(weak))
            for r in weak:
                self.assertEqual(sum(float(i['first_sample_b'])<=float(r['b_over_r'])<=float(i['last_sample_b']) for i in own),1)


if __name__=='__main__':unittest.main()
