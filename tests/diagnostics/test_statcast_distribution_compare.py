import csv
import tempfile
import unittest
from pathlib import Path

from spin_distributions import SpinDistributions
from statcast_distribution_compare import observed, simulated, run


def write(path, rows):
    with path.open('w',newline='') as f:
        w=csv.DictWriter(f,fieldnames=list(rows[0]));w.writeheader();w.writerows(rows)


def row(n, speed='90', angle='15', kind='X'):
    return dict(game_pk='1',at_bat_number='1',pitch_number=str(n),type=kind,
                launch_speed=speed,launch_angle=angle)


def emit(path):
    a=SpinDistributions()
    r=dict(variant='observed_sigma_0.005_r0.9',adjusted_contact_type='SolidContact')
    m=dict(collision_status='impulse_contact',ev_ms=40.,vla_deg=15.,spin_rpm=2000.,hla_deg=0.)
    a.add(r,{'normal_profile':m,'finite_I0.00015_et0.4':m},
          {'normal_profile':m,'finite_I0.00015_et0.4':m})
    a.write(path)
    return path/'finite_barrel_spin_ev_vla.csv'


class ComparisonTest(unittest.TestCase):
    def test_mph_conversion_missing_and_complete_bunt_labels(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);data=p/'data.csv';labels=p/'labels.csv'
            write(data,[row(1),row(2,speed=''),row(3),row(4,kind='S')])
            write(labels,[dict(game_pk='1',at_bat_number='1',pitch_number=str(n),is_bunt='1' if n==3 else '0') for n in (1,2,3)])
            grid,audit=observed(data,labels)
            self.assertEqual(sum(grid.values()),1)
            self.assertEqual(audit['excluded_bunt'],1)
            self.assertEqual(audit['missing_ev'],1)
            self.assertEqual(audit['not_in_play'],1)
            # 90 mph is 40.2336 m/s, in [40,45).
            self.assertEqual(next(iter(grid))[0],9)

    def test_full_run_denominators_and_unknown_bunts_are_explicit(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);sim=emit(p);data=p/'data.csv';write(data,[row(1),row(2,angle='')])
            meta=run(sim,data,p/'out')
            self.assertEqual(meta['simulation_n'],1)
            self.assertEqual(meta['statcast_n'],1)
            self.assertEqual(meta['bunt_policy'],'unknown_included')
            self.assertEqual(meta['status'],'exploratory_not_calibrated')
            with (p/'out'/'statcast_ev_vla_comparison.csv').open() as f:rows=list(csv.DictReader(f))
            self.assertEqual(sum(int(r['simulation_count']) for r in rows),1)
            self.assertEqual(sum(int(r['statcast_count']) for r in rows),1)
            with self.assertRaises(FileExistsError):run(sim,data,p/'out')

    def test_missing_bunt_label_does_not_silently_exclude(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);data=p/'data.csv';labels=p/'labels.csv';write(data,[row(1)])
            write(labels,[dict(game_pk='1',at_bat_number='1',pitch_number='2',is_bunt='0')])
            with self.assertRaisesRegex(ValueError,'missing_bunt_label'):observed(data,labels)

    def test_duplicate_and_invalid_measurement_fail(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp)/'data.csv'
            for rows,expected in [([row(1),row(1)],'duplicate'),([row(1,speed='inf')],'nonfinite'),
                                  ([row(1,angle='91')],'out_of_range')]:
                write(p,rows)
                with self.assertRaisesRegex(ValueError,expected):observed(p)

    def test_corrupt_histogram_denominator_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);sim=emit(p)
            with sim.open() as f:rows=list(csv.DictReader(f))
            for r in rows:r['paired_contacts']='2'
            write(sim,rows)
            with self.assertRaisesRegex(ValueError,'invalid_simulation'):simulated(sim,'all_swings','observed_sigma_0.005_r0.9','input_fair_common','finite_I0.00015_et0.4')
