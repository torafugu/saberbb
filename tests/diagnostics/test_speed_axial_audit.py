import csv
import math
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_forecast_contact_geometry import fixture,csv_rows,replace_csv
from speed_axial_audit import SpeedAxialAudit,contact_values,strata
import finite_barrel_probe as probe


def contact(speed=30.,axial=.1,kind='SolidContact'):
    r=dict(status='contact',collision_status='impulse_contact',contact_region='side',
        variant='v',adjusted_contact_type=kind,closest_axial_at_entry_m=axial,_source_z_m=.71+axial,
        local_signed_b_over_r=-.6,bat_point_speed_ms=speed,ev_ms=40.,vla_deg=10.,hla_deg=0.)
    for prefix,values in [('axis_at_entry_',(math.sqrt(.75),0.,.5)),('normal_',(0.,1.,0.)),
        ('point_velocity_',(0.,speed,0.)),('ball_velocity_',(0.,-38.,0.))]:
        for a,v in zip('xyz',values):r[prefix+a+('_ms' if 'velocity' in prefix else '')]=v
    return r


class SpeedAxialTest(unittest.TestCase):
    def test_signed_coordinates_and_bin_boundaries(self):
        from collections import defaultdict
        values=contact_values(contact(),defaultdict(float))
        self.assertAlmostEqual(values['signed_tilt_deg'],30.)
        self.assertAlmostEqual(values['pitch_speed_ms'],38.)
        self.assertIn(('speed_axial','30..<35','0.08..<0.12','all','all'),list(strata(values)))
        self.assertIn(('speed_axial_impact','30..<35','0.08..<0.12','-0.75..<-0.5','all'),list(strata(values)))
        r=contact();r['bat_point_speed_ms']=31.
        with self.assertRaisesRegex(ValueError,'saved_speed_ms'):contact_values(r,defaultdict(float))

    def test_partition_model_independence_and_missing_outcomes(self):
        a=SpeedAxialAudit()
        for r in [contact(),contact(20.,-.1,'UnrecordedBatting')]:
            a.add(r,{'success':dict(collision_status='impulse_contact',ev_ms=20.,vla_deg=-5.,hla_deg=0.),
                'missing':dict(collision_status='not_evaluated:out_of_curve',ev_ms=None)})
        miss=dict(status='no_contact',variant='v',adjusted_contact_type='SolidContact')
        a.add(miss,{'success':dict(collision_status='not_evaluated:no_contact',ev_ms=None),
            'missing':dict(collision_status='not_evaluated:no_contact',ev_ms=None)})
        with tempfile.TemporaryDirectory() as tmp:
            out=Path(tmp);a.write(out)
            geometry=csv_rows(out/'finite_barrel_speed_axial_geometry.csv')
            outcomes=csv_rows(out/'finite_barrel_speed_axial_outcomes.csv')
            for s in ('overall','speed_axial','speed_axial_impact','speed_axial_tilt'):
                self.assertEqual(sum(int(r['contacts']) for r in geometry if r['scope']=='all_swings' and r['stratum']==s),2)
                self.assertEqual(sum(int(r['contacts']) for r in geometry if r['scope']=='recorded_batting_only' and r['stratum']==s),1)
            absent=[r for r in outcomes if r['outcome_basis']=='missing']
            self.assertTrue(all(r['n_ev_ms']=='0' and r['min_ev_ms']=='' for r in absent))
            success=[r for r in outcomes if r['outcome_basis']=='success']
            self.assertTrue(all(r['ev_lt25_contacts']==r['contacts'] and r['min_ev_ms']=='20.0' for r in success))
            coverage=csv_rows(out/'finite_barrel_speed_axial_coverage.csv')
            self.assertEqual(sum(int(r['trial_rows']) for r in coverage if r['scope']=='all_swings' and r['model']=='success'),3)

    def test_opt_in_mode_matches_combined_and_no_ground_or_search(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root)
            kwargs=dict(ball_spin=(100.,0.,0.),model_names=['finite_I0.00015_et0.4'],variants=['observed_sigma_0.005_r0.9'])
            with patch.object(probe,'first_ground',side_effect=AssertionError('unneeded ground')), \
                patch('rotating_bat_probe.search',side_effect=AssertionError('unneeded search')):
                probe.run(source,root/'only',diagnostics=['speed_axial'],**kwargs)
                probe.run(source,root/'both',diagnostics=['tilt','speed_axial'],**kwargs)
            for name in ('geometry','outcomes','checks','coverage'):
                filename=f'finite_barrel_speed_axial_{name}.csv'
                self.assertEqual((root/'only'/filename).read_bytes(),(root/'both'/filename).read_bytes())
            speed=csv_rows(root/'only'/'finite_barrel_speed_axial_outcomes.csv')
            tilt=csv_rows(root/'both'/'finite_barrel_tilt_outcomes.csv')
            def counts(rows):
                from collections import Counter
                c=Counter()
                for r in rows:
                    if r['scope']=='all_swings' and r['stratum']=='overall':
                        c[r['outcome_basis'],r['collision_status'],r['direction']]+=int(r['contacts'])
                return c
            self.assertEqual(counts(speed),counts(tilt))
            probe.run(source,root/'default',ball_spin=(100.,0.,0.))
            self.assertFalse((root/'default'/'finite_barrel_speed_axial_geometry.csv').exists())
            # Restricting the solved variant must still validate all source rows.
            rows=csv_rows(source/'observation_update_contacts.csv')
            next(r for r in rows if r['variant']!='observed_sigma_0.005_r0.9')['contact_contract_version']='99'
            replace_csv(source/'observation_update_contacts.csv',rows)
            with self.assertRaisesRegex(ValueError,'unsupported_contact_contract'):
                probe.run(source,root/'bad',diagnostics=['speed_axial'],**kwargs)


if __name__=='__main__':unittest.main()
