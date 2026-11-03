import csv
import sys
import tempfile
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2]/'tools'/'diagnostics'))
from spin_distributions import SpinDistributions, bounds, DELTA_EDGES


def response(ev=40., vla=0., spin=2000., hla=0.):
    return dict(collision_status='impulse_contact', ev_ms=ev, vla_deg=vla,
                spin_rpm=spin, hla_deg=hla)


class DistributionTest(unittest.TestCase):
    def test_cancellation_and_shared_denominators(self):
        a = SpinDistributions()
        r = dict(variant='test', adjusted_contact_type='SolidContact')
        for delta in (-2., 2.):
            a.add(r, {'normal_profile': response()},
                  {'normal_profile': response(vla=delta, spin=2000.+delta*100)})
        a.add(r, {'normal_profile': response()},
              {'normal_profile': dict(collision_status='not_evaluated', ev_ms=None)})
        with tempfile.TemporaryDirectory() as tmp:
            a.write(Path(tmp))
            def read(name):
                with (Path(tmp)/name).open() as f:
                    return list(csv.DictReader(f))
            stats = read('finite_barrel_spin_delta_stats.csv')
            row = next(x for x in stats if x['scope']=='all_swings'
                       and x['cohort']=='all_common' and x['metric']=='vla_deg')
            self.assertEqual(float(row['mean_delta']), 0.)
            self.assertEqual(float(row['mean_abs_delta']), 2.)
            self.assertEqual(float(row['rms_delta']), 2.)
            self.assertEqual(int(row['paired_contacts']), 2)
            self.assertEqual((row['positive'],row['negative'],row['zero']),('1','1','0'))
            for filename, fields in [
                ('finite_barrel_spin_delta_hist.csv',['metric']),
                ('finite_barrel_spin_ev_vla.csv',['side'])]:
                groups = {}
                for x in read(filename):
                    key = tuple(x[k] for k in ['scope','variant','cohort','model']+fields)
                    groups[key] = groups.get(key,0)+int(x['count'])
                self.assertTrue(groups)
                self.assertTrue(all(n==2 for n in groups.values()))

    def test_fixed_reference_keeps_direction_crossings(self):
        a=SpinDistributions()
        r=dict(variant='test', adjusted_contact_type='UnrecordedBatting')
        a.add(r, {'normal_profile':response(), 'finite':response(hla=44.)},
              {'normal_profile':response(), 'finite':response(hla=46.)})
        self.assertEqual(a.stats['all_swings','test','reference_fair_common','finite']['contacts'],1)
        self.assertNotIn(('all_swings','test','both_fair_common','finite'),a.stats)
        self.assertTrue(all(k[0]=='all_swings' for k in a.stats))

    def test_half_open_bins_and_tails(self):
        import bisect
        edges=DELTA_EDGES['vla_deg']
        self.assertEqual(bounds(edges,bisect.bisect_right(edges,0.)),(0.,1.))
        self.assertEqual(bounds(edges,0),(None,-10.))
        self.assertEqual(bounds(edges,len(edges)),(10.,None))

    def test_nonfinite_success_is_an_error(self):
        a=SpinDistributions()
        with self.assertRaisesRegex(ValueError,'nonfinite'):
            a.add(dict(variant='x',adjusted_contact_type='x'),
                  {'normal_profile':response()}, {'normal_profile':response(spin=float('nan'))})
