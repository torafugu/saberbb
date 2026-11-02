import hashlib
import json
import math
import tempfile
import unittest
from collections import defaultdict
from pathlib import Path

from test_forecast_contact_geometry import fixture, csv_rows
from contact_3d import ContactGeometry
from contact_recoil import solve
from forecast_contact_geometry import audit
from restitution_bounds import analyze
from restitution_sensitivity import FIXED_E, KEYS, SensitivityAggregate, direction, upper_band
import restitution_bounds_probe as probe


def sample(vin=(0.,-40.,0.),vbat=(0.,35.,0.),n=(0.,1.,0.),inv=1/.53):
    axis=(0.,0.,1.) if n[0] else (1.,0.,0.)
    g=ContactGeometry(.4,axis,n,.03,.03,.16,'side')
    return analyze(vin,vbat,n,inv),{e:solve(g,vin,vbat,inv,e) for e in FIXED_E}


def row(kind='SolidContact'):
    return dict(variant='test',adjusted_contact_type=kind,status='contact',contact_region='side',
        closest_axial_at_entry_m=.03,local_abs_b_over_r=.2,normal_speed_fraction=1.,forecast_error_z_m=0.)


class RestitutionSensitivityTest(unittest.TestCase):
    def test_upper_bins_keep_open_and_closed_limits_separate(self):
        for hi,band in ((.099,'0..<0.1'),(.1,'0.1..<0.2'),(.2,'0.2..<0.3'),(.3,'0.3..<0.4'),(.4,'0.4..<0.5'),(.5,'0.5')):
            self.assertEqual(upper_band(dict(weak_fair_possible=1,weak_e_hi=hi)),band)
        self.assertEqual(upper_band(dict(weak_fair_possible=0)),'not_possible')
        self.assertEqual(upper_band(dict(weak_fair_possible=None)),'not_evaluated')
        with self.assertRaises(ValueError):upper_band(dict(weak_fair_possible=1,weak_e_hi=.6))
        with tempfile.TemporaryDirectory() as tmp:
            agg=SensitivityAggregate();r=row()
            # Same supremum, distinct endpoint attainability; reduction's infimum is zero.
            for bat_speed,is_open in ((24.,True),(23.5,False)):
                m,fixed=sample(vin=(0.,22.,0.),vbat=(0.,bat_speed,0.),inv=0.)
                self.assertEqual(m['weak_e_hi'],.5);self.assertEqual(m['weak_e_hi_open'],is_open)
                agg.add(r,m,fixed)
            agg.write(Path(tmp));rs=[x for x in csv_rows(Path(tmp)/'restitution_bounds_weak_hi.csv') if x['scope']=='all_swings' and x['stage']=='overall']
            self.assertEqual(len(rs),2);self.assertEqual({r['weak_e_hi_open'] for r in rs},{'True','False'})
            self.assertTrue(all(float(r['avg_required_reduction_infimum'])==0 for r in rs))

    def test_paired_strong_to_weak_and_medium_and_identity(self):
        m,fixed=sample();agg=SensitivityAggregate();agg.add(row(),m,fixed)
        with tempfile.TemporaryDirectory() as tmp:
            out=Path(tmp);agg.write(out)
            rs={float(r['restitution_e']):r for r in csv_rows(out/'restitution_bounds_fixed.csv') if r['scope']=='all_swings' and r['stage']=='overall'}
            self.assertEqual(int(rs[.1]['strong_to_weak_fair_contacts']),1)
            self.assertEqual(int(rs[.2]['strong_to_medium_fair_contacts']),1)
            self.assertEqual(int(rs[.4]['strong_to_strong_fair_contacts']),1)
            self.assertEqual(rs[.5]['before_paired_fair_ev_ms'],rs[.5]['after_paired_fair_ev_ms'])
            for r in rs.values():
                self.assertEqual(sum(int(r[k]) for k in ('strong_to_weak_fair_contacts','strong_to_medium_fair_contacts','strong_to_strong_fair_contacts','strong_to_nonfair_contacts')),int(r['baseline_strong_fair_contacts']))
            upper=next(r for r in csv_rows(out/'restitution_bounds_weak_hi.csv') if r['scope']=='all_swings' and r['stage']=='overall')
            self.assertAlmostEqual(float(upper['avg_required_reduction_infimum']),.5-m['weak_e_hi'])

    def test_direction_flips_undefined_and_exact_ev_threshold(self):
        m,fixed=sample(n=(.4,math.sqrt(.84),0.),vbat=(14.,35*math.sqrt(.84),0.))
        self.assertEqual(direction(fixed[.5]),'fair_direction')
        self.assertEqual(direction(fixed[.3]),'foul_direction')
        # Exactly EV=25 is medium, not weak, even if analytic roots round by one ulp.
        _,tie=sample(vin=(0.,14.,0.),vbat=(0.,24.,0.),inv=0.)
        self.assertEqual(tie[.1]['ev_ms'],25.)
        agg=SensitivityAggregate();agg.add(row(),m,fixed)
        mm,zero=sample(vin=(0.,-3.,0.),vbat=(0.,-1.,0.),inv=0.)
        self.assertEqual(direction(zero[.5]),'undefined_direction');agg.add(row(),mm,zero)
        mt,_=sample(vin=(0.,14.,0.),vbat=(0.,24.,0.),inv=0.);agg.add(row(),mt,tie)
        with tempfile.TemporaryDirectory() as tmp:
            out=Path(tmp);agg.write(out)
            transitions=csv_rows(out/'restitution_bounds_fixed_transitions.csv')
            t=[r for r in transitions if r['scope']=='all_swings' and r['stage']=='overall' and float(r['restitution_e'])==.3]
            self.assertTrue(any(r['before_direction']=='fair_direction' and r['after_direction']=='foul_direction' for r in t))
            self.assertTrue(any(r['before_direction']=='undefined_direction' for r in t))

    def test_continuous_feasibility_does_not_promise_a_grid_hit(self):
        m,fixed=sample(vin=(0.,-120.,24.999999),vbat=(0.,-20.,0.),inv=0.)
        self.assertEqual(m['weak_fair_possible'],1)
        self.assertFalse(any(direction(x)=='fair_direction' and x['ev_ms']<25 for x in fixed.values()))

    def test_unevaluated_remains_separate_and_scopes_do_not_mix(self):
        m={k:None for k in probe.BOUNDS};m['diagnosis']='not_evaluated:unresolved'
        r=dict(row('UnrecordedBatting'),status='unresolved',contact_region='',local_abs_b_over_r=None,normal_speed_fraction=None)
        agg=SensitivityAggregate();agg.add(r,m,{})
        with tempfile.TemporaryDirectory() as tmp:
            out=Path(tmp);agg.write(out);fixed=csv_rows(out/'restitution_bounds_fixed.csv')
            self.assertTrue(all(x['scope']=='all_swings' for x in fixed))
            self.assertTrue(all(x['avg_ev_ms']=='' and int(x['not_evaluated_contacts'])==1 for x in fixed))
            self.assertTrue(all(x['weak_e_hi_band']=='not_evaluated' for x in csv_rows(out/'restitution_bounds_weak_hi.csv')))
        with self.assertRaises(ValueError):agg.add(row(),sample()[0],{.5:sample()[1][.5]})

    def test_full_pipeline_rollups_endpoints_preservation_and_readonly(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root);hashes={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()}
            out=root/'out';probe.run(source,out)
            summaries=csv_rows(out/'restitution_bounds_summary.csv');fixed=csv_rows(out/'restitution_bounds_fixed.csv')
            upper=csv_rows(out/'restitution_bounds_weak_hi.csv');transitions=csv_rows(out/'restitution_bounds_fixed_transitions.csv')
            key=lambda r:tuple(r[k] for k in KEYS)
            s={key(r):r for r in summaries};u=defaultdict(list);t=defaultdict(list)
            for r in upper:u[key(r)].append(r)
            for r in transitions:t[key(r)+(float(r['restitution_e']),)].append(r)
            for k,rs in u.items():
                self.assertEqual(sum(int(r['states']) for r in rs),int(s[k]['states']))
                possible=[r for r in rs if r['weak_e_hi_band'] not in ('not_possible','not_evaluated')]
                self.assertEqual(sum(int(r['states']) for r in possible),int(s[k]['weak_fair_possible_contacts']))
                if possible:self.assertAlmostEqual(sum(float(r['pct_of_weak_possible']) for r in possible),100.)
            for r in fixed:
                k=key(r);e=float(r['restitution_e']);ss=s[k];tt=t[k+(e,)]
                self.assertEqual(r['states'],ss['states'])
                self.assertEqual(sum(int(x['states']) for x in tt),int(r['states']))
                self.assertEqual(sum(int(x['states']) for x in tt if x['after_direction']=='fair_direction'),int(r['fair_contacts']))
                self.assertEqual(int(r['fair_contacts'])+int(r['foul_contacts'])+int(r['undefined_direction_contacts']),int(r['impulse_contacts']))
                if e==.5:
                    self.assertEqual(r['fair_contacts'],ss['baseline_fair_contacts'])
                    if r['avg_ev_ms']:self.assertAlmostEqual(float(r['avg_ev_ms']),float(ss['avg_baseline_ev_ms']))
            # The wrapper retains the original analytic result and endpoint-only checks.
            checks=defaultdict(float)
            with (source/'observation_update_contacts.csv').open() as f:
                import csv
                raw=next(csv.DictReader(f))
            r=audit(raw,checks);m=probe.event(r,probe.measured_wood(),checks)
            m2,fs=probe.evaluated_event(r,probe.measured_wood(),defaultdict(float),True)
            self.assertEqual(m,m2)
            if fs:self.assertEqual(set(fs),set(FIXED_E))
            meta=json.loads((out/'restitution_bounds_metadata.json').read_text())
            self.assertEqual(meta['fixed_restitution_values'],list(FIXED_E))
            self.assertTrue(all(float(r['max_error'])<1e-7 for r in csv_rows(out/'restitution_bounds_checks.csv')))
            self.assertEqual(hashes,{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()})
