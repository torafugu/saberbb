import copy
import hashlib
import json
import math
import sqlite3
import tempfile
import unittest
from collections import defaultdict
from pathlib import Path
from unittest.mock import patch

from test_forecast_contact_geometry import fixture,csv_rows
from test_contact_state_probe import state
from test_swing_prediction_probe import prediction
import bat_pivot_orientation_probe as probe
from swing_prediction_probe import plan_swing


def source_db(root):
    src=fixture(root);path=root/'test.db';db=sqlite3.connect(path)
    db.execute('CREATE TABLE player_game_pitching(game_id,count_seq,spin_rate,spin_angle,spin_efficiency)')
    for seq in (1,2):db.execute('INSERT INTO player_game_pitching VALUES(10,?,2200,0,.9)',(seq,))
    db.commit();db.close()
    return src,path


class PivotOrientationTest(unittest.TestCase):
    def test_anchor_and_signed_axis_including_left_and_execution_error(self):
        for h in (-1.,1.):
            for error in (-2.,0.,2.):
                p=prediction()['input'];p.update(handedness_sign=h,execution_angle_error_deg=error)
                p['forecast']['position_world_m']=[.1,0,.5];p['planned_bat_angle_deg']=45.
                saved=plan_swing(p,.9,.16);changed=probe.counterfactual(saved)
                a,b=(probe.motion_for(state(),x) for x in (saved,changed))
                for l,r in zip(a.point,b.point):self.assertAlmostEqual(l,r)
                for l,r in zip(a.velocity,b.velocity):self.assertAlmostEqual(l,r)
                self.assertAlmostEqual(saved['actual_axis_world'][0],changed['actual_axis_world'][0])
                self.assertAlmostEqual(saved['actual_axis_world'][2],-changed['actual_axis_world'][2])
                self.assertAlmostEqual(probe.norm(a.omega),probe.norm(b.omega))
                self.assertLess(saved['pivot_world_m'][2],0);self.assertGreater(changed['pivot_world_m'][2],.5)
                self.assertEqual(saved['nominal_time_sec'],changed['nominal_time_sec'])
                self.assertEqual(saved['forecast_position_world_m'],changed['forecast_position_world_m'])
        # A zero tilt is an identity experiment, including nonzero vertical velocity.
        p.update(planned_bat_angle_deg=0.,execution_angle_error_deg=0.)
        saved=plan_swing(p,.9,.16);changed=probe.counterfactual(saved)
        a,b=(probe.motion_for(state(),x) for x in (saved,changed))
        for t in (-.02,0,.02):
            for av,bv in zip(a.pose(t,.001),b.pose(t,.001)):
                for x,y in zip(av,bv):self.assertAlmostEqual(x,y)

    def test_one_search_each_denominators_controls_and_readonly(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,path=source_db(root)
            before=hashlib.sha256(path.read_bytes()).hexdigest()
            original=probe.search
            with patch.object(probe,'search',wraps=original) as searched:
                counts=probe.run(source,path,root/'out')
            self.assertEqual(counts['paired_trials'],2);self.assertEqual(searched.call_count,2)
            self.assertEqual(before,hashlib.sha256(path.read_bytes()).hexdigest())
            rows=csv_rows(root/'out'/'bat_pivot_orientation_summary.csv')
            overall=[r for r in rows if r['scope']=='all_swings' and r['stratum']=='overall']
            self.assertEqual(len(overall),6)
            self.assertTrue(all(int(r['trials'])==2 for r in overall))
            for orient in ('saved_up','matched_down'):
                models={r['model']:r for r in overall if r['orientation']==orient}
                for field in ('successful_contacts','fair_contacts','avg_ev_ms','avg_vla_deg'):
                    self.assertAlmostEqual(float(models['normal_profile'][field]),float(models['finite_mu0'][field]))
            transition=csv_rows(root/'out'/'bat_pivot_orientation_transitions.csv')
            for model in probe.MODEL_NAMES:
                selected=[r for r in transition if r['scope']=='all_swings' and r['stratum']=='overall' and r['model']==model]
                self.assertEqual(sum(int(r['trials']) for r in selected),2)
            meta=json.loads((root/'out'/'bat_pivot_orientation_metadata.json').read_text())
            self.assertEqual(meta['new_searches'],2);self.assertEqual(meta['spin']['unique_pitches'],2)
            checks=csv_rows(root/'out'/'bat_pivot_orientation_checks.csv')
            self.assertLess(max(float(r['max_error']) for r in checks),1e-7)
            hist=csv_rows(root/'out'/'bat_pivot_orientation_ev_vla.csv')
            for r in overall:
                for cohort,field in [('all_successful','successful_contacts'),('fair_direction','fair_contacts')]:
                    cells=[x for x in hist if x['scope']=='all_swings' and x['orientation']==r['orientation']
                        and x['model']==r['model'] and x['cohort']==cohort]
                    self.assertEqual(sum(int(x['contacts']) for x in cells),int(r[field]))
                    if cells:self.assertAlmostEqual(sum(float(x['fraction']) for x in cells),1.)
            with self.assertRaises(FileExistsError):probe.run(source,path,root/'out')

    def test_saved_baseline_matches_existing_finite_probe(self):
        import finite_barrel_probe as finite
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,path=source_db(root)
            probe.run(source,path,root/'pair')
            finite.run(source,root/'finite',pitch_db=path,gyro_sign=0,
                diagnostics=['tilt'],model_names=['finite_I0.00015_et0.4'],variants=['observed_sigma_0.005_r0.9'])
            paired=csv_rows(root/'pair'/'bat_pivot_orientation_summary.csv')
            old=csv_rows(root/'finite'/'finite_barrel_tilt_outcomes.csv')
            for model in probe.MODEL_NAMES:
                r=next(r for r in paired if r['scope']=='all_swings' and r['stratum']=='overall'
                    and r['orientation']=='saved_up' and r['model']==model)
                cells=[x for x in old if x['scope']=='all_swings' and x['stratum']=='overall'
                    and x['outcome_basis']==model and x['collision_status']=='impulse_contact']
                n=sum(int(x['contacts']) for x in cells)
                self.assertEqual(n,int(r['successful_contacts']))
                self.assertEqual(sum(int(x['contacts']) for x in cells if x['direction']=='fair_direction'),int(r['fair_contacts']))
                if n:self.assertAlmostEqual(sum(float(x['avg_ev_ms'])*int(x['n_ev_ms']) for x in cells)/n,float(r['avg_ev_ms']))

    def test_database_clock_mismatch_and_missing_pitch_fail(self):
        for mode in ('clock','pitch'):
            with self.subTest(mode=mode),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);source,path=source_db(root);db=sqlite3.connect(path)
                if mode=='clock':
                    s=json.loads(db.execute('SELECT state_json FROM player_game_contact_state WHERE count_seq=1').fetchone()[0])
                    # Change the state coherently so the saved-state validator still passes.
                    s['input']['reference_time_sec_since_release']+=.01
                    db.execute('UPDATE player_game_contact_state SET state_json=? WHERE count_seq=1',(json.dumps(s),))
                else:db.execute('DELETE FROM player_game_pitching WHERE count_seq=1')
                db.commit();db.close()
                with self.assertRaisesRegex(ValueError,'db_clock_sec' if mode=='clock' else 'pitch_spin_key_missing'):
                    probe.run(source,path,root/'bad')

    def test_unavailable_outcomes_and_unresolved_remain_in_transitions(self):
        agg=probe.Aggregate();p=plan_swing(prediction()['input'],.9,.16)
        row=dict(adjusted_contact_type='SolidContact',variant='test',status='unresolved')
        plans=dict(saved_up=p,matched_down=probe.counterfactual(p))
        results={o:{m:dict(collision_status='not_evaluated:unresolved',ev_ms=None) for m in probe.MODEL_NAMES} for o in plans}
        agg.add(row,plans,{o:row for o in plans},results)
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);agg.write(root)
            rows=csv_rows(root/'bat_pivot_orientation_summary.csv')
            self.assertTrue(all(r['trials']=='1' and r['successful_contacts']=='0' and r['avg_ev_ms']=='' for r in rows))
            self.assertTrue(all(r['paired_successful_contacts']=='0' for r in csv_rows(root/'bat_pivot_orientation_pairs.csv')))


if __name__=='__main__':unittest.main()
