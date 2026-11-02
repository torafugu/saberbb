import hashlib
import json
import math
import tempfile
import unittest
from collections import defaultdict
from pathlib import Path

from test_forecast_contact_geometry import fixture, csv_rows, replace_csv
import bat_vibration_profile as profile
import vibration_profile_probe as probe
from contact_3d import ContactGeometry, solve_contact
from contact_recoil import inverse_mass, solve


def contact_row(s=0.,cap=False):
    axis=(1.,0.,0.);normal=(1.,0.,0.) if cap else (0.,1.,0.)
    axial=.18 if cap else s;closest=.16 if cap else s
    vin=(-40.,-40.,-2.);vbat=(0.,35.,5.)
    g=ContactGeometry(.4,axis,normal,axial,closest,.16,'cap' if cap else 'side')
    m=solve_contact(g,vin,vbat)
    r=dict(status='contact',collision_status=m['collision_status'],entry_since_release_sec=.4,
        axial_at_entry_m=axial,closest_axial_at_entry_m=closest,half_length_m=.16,contact_region=g.region,lever_m=.9,
        relative_normal_ms=m['relative_normal_ms'],variant='test',adjusted_contact_type='SolidContact',
        local_abs_b_over_r=0. if not cap else None,normal_speed_fraction=m['normal_speed_fraction'],forecast_error_z_m=0.)
    for prefix,value in [('axis_at_entry_',axis),('normal_',normal),('ball_velocity_',vin),('point_velocity_',vbat),('outgoing_velocity_',m['outgoing_world_ms'])]:
        for a,x in zip('xyz',value):r[prefix+a+('_ms' if 'velocity' in prefix else '')]=x
    for a,x in zip('xyz',(0.,-3.,20.)):r['omega_'+a+'_rad_s']=x
    return r


class VibrationProfileTest(unittest.TestCase):
    def test_published_parameters_and_interpolation_domain(self):
        m=profile.metadata()
        self.assertAlmostEqual(m['transverse_cm_inertia_kg_m2'],.0468165)
        self.assertAlmostEqual(m['cm_offset_from_saved_reference_m'],-.146)
        self.assertEqual(m['barrel_nodes_m'],[.68,.72,.74])
        self.assertEqual(profile.profile_e(.68),.48);self.assertEqual(profile.profile_e(.72),.5)
        self.assertAlmostEqual(profile.profile_e(.55),.23)
        self.assertAlmostEqual(profile.profile_e(.55,'half_speed'),.245)
        self.assertEqual(profile.profile_e(.54),.22);self.assertEqual(profile.profile_e(.84),.21)
        for z in (.53,.85):self.assertIsNone(profile.profile_e(z))
        for z,high,half in profile.KNOTS:
            self.assertEqual(profile.profile_e(z),high);self.assertEqual(profile.profile_e(z,'half_speed'),half)
            self.assertTrue(0<=high<=.5 and 0<=half<=.5)
        with self.assertRaises(ValueError):profile.profile_e(math.nan)
        with self.assertRaises(ValueError):profile.profile_e(.7,'unknown')

    def test_signed_apparent_cor_matches_stationary_target_and_is_not_e_eff(self):
        g=ContactGeometry(.4,(1.,0.,0.),(0.,1.,0.),0.,0.,.16,'side')
        for e in (0.,.22,.5,1.):
            for inv in (0.,1/.53,5.):
                q=profile.apparent_from_e(e,inv)
                self.assertAlmostEqual(profile.e_from_apparent(q,inv),e)
                m=solve(g,(0.,-1.,0.),(0.,0.,0.),inv,e)
                self.assertAlmostEqual(m['outgoing_world_ms'][1],q)
        self.assertLess(profile.apparent_from_e(0.,1/.53),0.)
        # Treating the paper's e_eff as apparent COR would inflate it again.
        self.assertGreater(profile.e_from_apparent(.5,1/.6),.5)
        with self.assertRaises(ValueError):profile.e_from_apparent(1.,1.)

    def test_peak_is_rigid_control_and_mass_change_is_isolated(self):
        r=contact_row();checks=defaultdict(float);results,reason=probe.event(r,checks)
        self.assertEqual(reason,'eligible');self.assertEqual(set(results),set(probe.MODELS))
        rigid=results[probe.RIGID]
        for name in (probe.FULL,probe.HALF):
            self.assertEqual(results[name]['applied_e'],.5)
            self.assertEqual(results[name]['outgoing_world_ms'],rigid['outgoing_world_ms'])
        inv,_,_=inverse_mass((1.,0.,0.),(0.,1.,0.),0.,.9,'free',.885,.23,-.146)
        self.assertAlmostEqual(rigid['effective_mass_kg'],1/inv)
        self.assertNotAlmostEqual(rigid['effective_mass_kg'],results[probe.OLD]['effective_mass_kg'])
        self.assertLess(max(checks.values()),1e-7)

    def test_offcenter_uses_direct_e_and_speed_templates_without_refitting(self):
        r=contact_row(-.13);results,reason=probe.event(r,defaultdict(float))
        self.assertEqual(reason,'eligible');self.assertAlmostEqual(r['_source_z_m'],.58)
        self.assertAlmostEqual(results[probe.FULL]['applied_e'],.28)
        self.assertAlmostEqual(results[probe.HALF]['applied_e'],.30)
        ratio=75/(64+51*(.58-.564))
        self.assertAlmostEqual(results[probe.FULL]['source_speed_ratio'],ratio)
        self.assertAlmostEqual(results[probe.HALF]['source_speed_ratio'],2*ratio)
        self.assertLess(results[probe.FULL]['normal_impulse_kg_ms'],results[probe.RIGID]['normal_impulse_kg_ms'])

    def test_caps_and_out_of_curve_are_not_clipped_or_counted_as_outcome_losses(self):
        for r,reason in ((contact_row(cap=True),'not_evaluated:unsupported_cap'),(contact_row(.15),'not_evaluated:out_of_curve')):
            results,actual=probe.event(r,defaultdict(float));self.assertEqual(actual,reason)
            self.assertEqual(results[probe.OLD]['collision_status'],'impulse_contact')
            for name in (probe.RIGID,probe.FULL,probe.HALF):
                self.assertEqual(results[name]['collision_status'],reason);self.assertIsNone(results[name]['ev_ms'])
            agg=probe.Aggregate();agg.add(r,results)
            for p in agg.pairs.values():
                self.assertEqual(p['paired_impulse_contacts'],0);self.assertEqual(p['before_strong_fair'],0)

    def test_unresolved_is_not_a_zero_ev_contact(self):
        r=dict(status='unresolved',contact_region='',variant='test',adjusted_contact_type='UnrecordedBatting',
            local_abs_b_over_r=None,normal_speed_fraction=None,forecast_error_z_m=0.)
        results,reason=probe.event(r,defaultdict(float))
        self.assertEqual(reason,'not_evaluated:unresolved')
        self.assertTrue(all(m['ev_ms'] is None for m in results.values()))
        agg=probe.Aggregate();agg.add(r,results)
        self.assertTrue(all(k[0]=='all_swings' for k in agg.groups))

    def test_pipeline_readonly_counts_common_pairs_and_metadata(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root);hashes={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()}
            out=root/'out';coverage=probe.run(source,out)
            self.assertEqual(coverage['trial_rows'],6);self.assertEqual(coverage['models'],5)
            self.assertEqual(coverage['source_geometry_contacts'],coverage['source_impulse_contacts']+coverage['non_approaching_contacts'])
            self.assertEqual(coverage['source_impulse_contacts'],sum(coverage[k] for k in ('eligible_side_contacts','unsupported_cap_contacts','out_of_curve_contacts')))
            ss=csv_rows(out/'vibration_profile_summary.csv');ps=csv_rows(out/'vibration_profile_pairs.csv');ts=csv_rows(out/'vibration_profile_transitions.csv')
            key=lambda r:tuple(r[k] for k in probe.KEYS)
            transitions=defaultdict(list)
            for r in ts:transitions[key(r)+(r['before_model'],r['after_model'])].append(r)
            for r in ss:
                self.assertEqual(int(r['states']),int(r['impulse_contacts'])+int(r['unavailable_or_nonapproaching_states']))
                self.assertEqual(int(r['impulse_contacts']),sum(int(r[k]) for k in ('fair_contacts','foul_contacts','undefined_direction_contacts')))
            for r in ps:
                group=transitions[key(r)+(r['before_model'],r['after_model'])]
                self.assertEqual(sum(int(x['states']) for x in group),int(r['states']))
                self.assertEqual(sum(int(r['strong_to_'+k+'_contacts']) for k in ('weak','medium','strong','nonfair')),int(r['before_strong_fair_contacts']))
                self.assertEqual(sum(int(x['states']) for x in group if x['before_direction']==x['after_direction']=='fair_direction'),int(r['paired_fair_contacts']))
            statuses=csv_rows(out/'vibration_profile_status.csv')
            self.assertEqual(sum(int(r['states']) for r in statuses),6*5)
            meta=json.loads((out/'vibration_profile_metadata.json').read_text())
            self.assertEqual(meta['source_profile']['source_pdf_sha256'],profile.metadata()['source_pdf_sha256'])
            self.assertEqual(len(csv_rows(out/'vibration_profile_knots.csv')),len(profile.KNOTS))
            self.assertTrue(all(float(r['max_error'])<1e-7 for r in csv_rows(out/'vibration_profile_checks.csv')))
            self.assertEqual(hashes,{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()})
            with self.assertRaises(FileExistsError):probe.run(source,out)

    def test_duplicate_missing_and_corrupt_sources_fail_closed(self):
        for mode in ('duplicate','missing','corrupt'):
            with self.subTest(mode=mode),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);source=fixture(root);path=source/'observation_update_contacts.csv';rs=csv_rows(path)
                if mode=='duplicate':rs.append(rs[0])
                elif mode=='missing':rs.pop()
                else:next(r for r in rs if r['status']=='contact')['outgoing_velocity_y_ms']='100'
                replace_csv(path,rs)
                with self.assertRaises(ValueError):probe.run(source,root/'out')


if __name__=='__main__':unittest.main()
