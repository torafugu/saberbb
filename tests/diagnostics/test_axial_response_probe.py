import hashlib
import json
import math
import tempfile
import unittest
from collections import defaultdict
from pathlib import Path

from test_forecast_contact_geometry import fixture, csv_rows, replace_csv
from test_vibration_profile import contact_row
import axial_response_probe as probe
from contact_3d import ContactGeometry, solve_contact


class AxialResponseTest(unittest.TestCase):
    def test_shapley_distributes_interaction_and_closes(self):
        # Additive components 2, -3, 4 and an interaction 6 shared by factors 0/1.
        values={m:10+2*bool(m&1)-3*bool(m&2)+4*bool(m&4)+6*bool(m&1 and m&2) for m in range(8)}
        actual=probe.shapley(values)
        for name,value in zip(probe.FACTORS,(5.,0.,4.)):self.assertAlmostEqual(actual[name],value)
        self.assertAlmostEqual(sum(actual.values()),values[7]-values[0])
        with self.assertRaises(ValueError):probe.shapley({0:0})
        with self.assertRaises(ValueError):probe.shapley({m:math.nan for m in range(8)})

    def test_reference_position_has_no_attributed_effect(self):
        checks=defaultdict(float);result=probe.event(contact_row(0.),checks)
        self.assertEqual(result['status'],'eligible')
        self.assertEqual(len(result['factorial']),8)
        for parts in result['contributions'].values():
            for value in parts.values():self.assertAlmostEqual(value,0.)
        self.assertLess(max(checks.values()),1e-7)

    def test_rootward_speed_and_effective_e_reduce_ev_but_recoil_can_help(self):
        result=probe.event(contact_row(-.13),defaultdict(float))
        d=result['descriptors'];parts=result['contributions']['ev_ms']
        self.assertLess(d['point_speed_ms'],d['reference_point_speed_ms'])
        self.assertGreater(d['effective_mass_kg'],d['reference_effective_mass_kg'])
        self.assertLess(d['applied_e'],.5)
        self.assertLess(parts['local_velocity'],0.)
        self.assertGreater(parts['rigid_recoil'],0.)
        self.assertLess(parts['positional_e'],0.)
        self.assertAlmostEqual(sum(parts.values()),d['actual_ev_ms']-d['reference_ev_ms'])

    def test_energy_budget_is_closed_and_not_named_body_or_vibration_energy(self):
        for s in (-.16,-.13,-.06,0.,.06,.12):
            checks=defaultdict(float);result=probe.event(contact_row(s),checks);d=result['descriptors']
            self.assertAlmostEqual(d['ball_before_j']+d['bat_before_j'],d['ball_after_j']+d['bat_after_j']+d['effective_loss_j'])
            self.assertGreaterEqual(d['effective_loss_j'],0.)
            self.assertAlmostEqual(d['bat_before_j'],d['bat_translation_before_j']+d['bat_rotation_before_j'])
            self.assertAlmostEqual(d['bat_after_j'],d['bat_translation_after_j']+d['bat_rotation_after_j'])
            self.assertLess(max(checks.values()),1e-7)
            self.assertFalse(any('body' in k or 'vibration' in k for k in d))

    def test_factors_change_only_their_own_solver_parameter(self):
        result=probe.event(contact_row(-.13),defaultdict(float));f=result['factorial']
        self.assertNotEqual(f[0]['relative_normal_ms'],f[1]['relative_normal_ms'])
        self.assertEqual(f[0]['effective_mass_kg'],f[1]['effective_mass_kg'])
        self.assertEqual(f[0]['applied_e'],f[1]['applied_e'])
        self.assertEqual(f[0]['relative_normal_ms'],f[2]['relative_normal_ms'])
        self.assertNotEqual(f[0]['effective_mass_kg'],f[2]['effective_mass_kg'])
        self.assertEqual(f[0]['relative_normal_ms'],f[4]['relative_normal_ms'])
        self.assertEqual(f[0]['effective_mass_kg'],f[4]['effective_mass_kg'])
        self.assertNotEqual(f[0]['applied_e'],f[4]['applied_e'])

    def test_counterfactual_nonapproaching_does_not_become_zero_ev(self):
        r=contact_row(.12)
        vin=(-40.,-.1,-2.);vbat=(0.,.1,5.)
        g=ContactGeometry(.4,(1.,0.,0.),(0.,1.,0.),.12,.12,.16,'side')
        m=solve_contact(g,vin,vbat)
        for prefix,value in [('ball_velocity_',vin),('point_velocity_',vbat),('outgoing_velocity_',m['outgoing_world_ms'])]:
            for axis,v in zip('xyz',value):r[prefix+axis+'_ms']=v
        r['relative_normal_ms']=m['relative_normal_ms']
        result=probe.event(r,defaultdict(float))
        self.assertEqual(result['status'],'eligible:counterfactual_nonapproaching')
        self.assertIsNone(result['contributions'])
        self.assertIsNone(result['factorial'][0]['ev_ms'])
        self.assertIsNotNone(result['descriptors']['actual_ev_ms'])

    def test_unavailable_stays_unavailable_and_profile_is_not_extrapolated(self):
        for r,reason in [(contact_row(cap=True),'unsupported_cap'),(contact_row(.15),'out_of_curve')]:
            result=probe.event(r,defaultdict(float))
            self.assertEqual(result['status'],'not_evaluated:'+reason)
            self.assertIsNone(result['factorial']);self.assertIsNone(result['descriptors'])
        r=dict(status='no_contact',contact_region='',variant='test',adjusted_contact_type='UnrecordedBatting',
            local_abs_b_over_r=None,normal_speed_fraction=None,forecast_error_z_m=0.)
        result=probe.event(r,defaultdict(float))
        self.assertEqual(result['status'],'not_evaluated:no_contact')
        aggregate=probe.Aggregate(0);aggregate.add(r,result)
        self.assertTrue(all(k[0]=='all_swings' for k in aggregate.summary))
        self.assertEqual(len(aggregate.factorial),0)

    def test_pipeline_source_integrity_counts_denominators_and_attribution(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root)
            hashes={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()}
            out=root/'out';coverage=probe.run(source,out,sample_limit=2)
            self.assertEqual(coverage['trial_rows'],6)
            self.assertEqual(sum(int(r['states']) for r in csv_rows(out/'axial_response_status.csv')),6)
            summary=csv_rows(out/'axial_response_summary.csv')
            for r in summary:
                self.assertEqual(int(r['states']),int(r['eligible_contacts'])+int(r['unavailable_states']))
                self.assertEqual(int(r['n_actual_ev_ms']),int(r['eligible_contacts']))
            for r in summary:
                if r['stage']!='overall':continue
                for stage in ('source_z','source_z_direction','saved_axial','normal_approach'):
                    rows=[b for b in summary if b['scope']==r['scope'] and b['variant']==r['variant'] and b['stage']==stage]
                    self.assertEqual(sum(int(b['states']) for b in rows),int(r['states']))
            factorial=csv_rows(out/'axial_response_factorial.csv')
            for r in factorial:
                self.assertEqual(int(r['eligible_contacts']),int(r['impulse_contacts'])+int(r['nonapproaching_contacts']))
            for r in csv_rows(out/'axial_response_attribution.csv'):
                self.assertAlmostEqual(sum(float(r['avg_'+k]) for k in probe.FACTORS),float(r['avg_delta']))
                self.assertAlmostEqual(float(r['avg_actual'])-float(r['avg_reference']),float(r['avg_delta']))
            self.assertLessEqual(len(csv_rows(out/'axial_response_examples.csv')),2)
            self.assertTrue(all(float(r['max_error'])<1e-7 for r in csv_rows(out/'axial_response_checks.csv')))
            meta=json.loads((out/'axial_response_metadata.json').read_text())
            self.assertEqual(meta['reference_saved_axial_m'],0.)
            self.assertEqual(hashes,{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()})
            with self.assertRaises(FileExistsError):probe.run(source,out)

    def test_corrupt_duplicate_missing_sources_fail(self):
        for mode in ('duplicate','missing','corrupt'):
            with tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);source=fixture(root);path=source/'observation_update_contacts.csv';rows=csv_rows(path)
                if mode=='duplicate':rows.append(rows[0])
                elif mode=='missing':rows.pop()
                else:next(r for r in rows if r['status']=='contact')['outgoing_velocity_y_ms']='1234'
                replace_csv(path,rows)
                with self.assertRaises(ValueError):probe.run(source,root/'out')
        with self.assertRaises(ValueError):probe.run('missing','unused',sample_limit=-1)


if __name__=='__main__':unittest.main()
