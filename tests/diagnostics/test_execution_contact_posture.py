import hashlib
import json
import math
import sqlite3
import tempfile
import unittest
from collections import defaultdict
from pathlib import Path
from unittest.mock import patch

from test_bat_pivot_orientation_probe import source_db
from test_forecast_contact_geometry import csv_rows,replace_csv
from contact_3d import ContactGeometry,solve_contact
import swing_execution_probe as probe
import execution_contact_posture as posture


class ContactPostureTest(unittest.TestCase):
    def fixture(self,root,invalid=False):
        source,db=source_db(root)
        if invalid:
            path=source/'observation_update_inputs.jsonl'
            records=[json.loads(x) for x in path.read_text().splitlines()]
            for r in records:
                if r['observations']:r['observations'][-1]['time_sec']=r['plan']['nominal_time_sec']-.001
            path.write_text(''.join(json.dumps(r)+'\n' for r in records))
        sweep=root/'sweep'
        probe.run(source,db,sweep,efforts=(1.,),execution_ratios=(1.,.95,.9,.85))
        return source,db,sweep

    def test_no_search_readonly_replay_closure_and_partitions(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,db,sweep=self.fixture(root)
            dbhash=posture.digest(db)
            paths=list(sweep.glob('*.*'))+list(source.glob('*.*'))
            before={p:posture.digest(p) for p in paths if p.is_file()}
            with patch('bat_pivot_orientation_probe.search',side_effect=AssertionError('searched')), \
                 patch('rotating_bat_probe.search',side_effect=AssertionError('searched')):
                result=posture.run(sweep,source,db,root/'out')
            self.assertEqual(result['source_trials'],2);self.assertEqual(result['new_searches'],0)
            self.assertGreater(result['replayed_contacts'],0)
            self.assertEqual(dbhash,posture.digest(db))
            self.assertEqual(before,{p:posture.digest(p) for p in before})
            metadata=json.loads((root/'out'/'execution_contact_posture_metadata.json').read_text())
            self.assertEqual(metadata['new_searches'],0)
            rows=csv_rows(root/'out'/'execution_contact_posture_summary.csv')
            overall=[r for r in rows if r['stratum']=='overall']
            original=csv_rows(sweep/'swing_execution_summary.csv')
            for r in overall:
                old=next(o for o in original if all(o[k]==r[k] for k in ('scenario','scope','model'))
                         and o['orientation']=='execution' and o['stratum']=='overall')
                n=old['fair_contacts' if r['cohort']=='fair_direction' else 'successful_contacts']
                self.assertEqual(r['contacts'],n)
                cells=[c for c in rows if c['stratum']=='vla' and all(c[k]==r[k] for k in ('scenario','scope','model','cohort'))]
                self.assertEqual(sum(int(c['contacts']) for c in cells),int(n))
                # Means of the per-contact decomposition also close on each identical set.
                self.assertAlmostEqual(float(r['avg_incoming_z_ms'])+float(r['avg_normal_delta_z_ms'])+
                                       float(r['avg_tangent_delta_z_ms']),float(r['avg_outgoing_z_ms']),places=10)
            selection=csv_rows(root/'out'/'execution_contact_posture_selection.csv')
            for old in original:
                if old['orientation']!='execution' or old['stratum']!='overall':continue
                selected=[r for r in selection if all(old[k]==r[k] for k in ('scenario','scope','model'))]
                self.assertEqual(sum(int(r['contacts']) for r in selected),int(old['geometry_contacts']))
            checks=csv_rows(root/'out'/'execution_contact_posture_checks.csv')
            self.assertLess(max(float(r['max_error']) for r in checks),2e-6)
            with self.assertRaises(FileExistsError):posture.run(sweep,source,db,root/'out')

    def test_vertical_normal_sign_and_tangent_contribution(self):
        for z in (-.6,.6):
            n=(0,.8,z);axis=(1.,0.,0.);vin=(0.,-40.,-2.);vbat=(0.,35.,3.)
            result=solve_contact(ContactGeometry(.4,axis,n,0.,0.,.16,'side'),vin,vbat)
            row=dict(contact_region='side',closest_axial_at_entry_m=0.,half_length_m=.16,
                     min_evaluated_clearance_m=0.,effective_radius_m=.07)
            for prefix,values in [('normal_',n),('axis_at_entry_',axis),('ball_velocity_',vin),('point_velocity_',vbat)]:
                for a,v in zip('xyz',values):row[prefix+a+('_ms' if 'velocity' in prefix else '')]=v
            values=posture.posture(row,result,defaultdict(float))
            self.assertEqual(values['normal_z'],z)
            self.assertGreater(values['normal_delta_z_ms']*z,0)
            self.assertEqual(values['tangent_delta_z_ms'],0.)
            self.assertAlmostEqual(values['vla_deg'],values['normal_only_vla_deg'])
            # Add an explicit downward tangential impulse and verify vector-derived VLA.
            result['tangent_impulse_world_kg_ms']=(0.,0.,-.145)
            out=tuple(v-(1. if i==2 else 0.) for i,v in enumerate(result['outgoing_world_ms']))
            result['outgoing_world_ms']=out;result['ev_ms'],_,result['vla_deg']=posture.angles(out)
            values=posture.posture(row,result,defaultdict(float))
            self.assertAlmostEqual(values['tangent_delta_z_ms'],-1.)
            self.assertLess(values['tangential_vla_change_deg'],0.)

    def test_delayed_execution_and_automatic_plan_controls_replay(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,db=source_db(root);sweep=root/'sweep'
            probe.run(source,db,sweep,efforts=(1.,.85),execution_ratios=(.85,),offsets=(.005,))
            with patch('bat_pivot_orientation_probe.search',side_effect=AssertionError('searched')):
                result=posture.run(sweep,source,db,root/'out')
            self.assertEqual(result['source_trials'],2)
            self.assertEqual(result['new_searches'],0)

    def test_invalid_clocks_remain_unevaluated(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,db,sweep=self.fixture(root,invalid=True)
            result=posture.run(sweep,source,db,root/'out')
            self.assertEqual(result['replayed_contacts'],0)
            coverage={r['metric']:int(r['trials']) for r in csv_rows(root/'out'/'execution_contact_posture_coverage.csv')}
            self.assertEqual(sum(n for k,n in coverage.items() if k.endswith(':clock_invalid')),8)
            self.assertEqual(csv_rows(root/'out'/'execution_contact_posture_summary.csv'),[])

    def test_corrupt_saved_entries_keys_and_missing_rows_rejected(self):
        for mode in ('time','key','plan','missing','extra'):
            with self.subTest(mode=mode),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);source,db,sweep=self.fixture(root)
                path=sweep/'swing_execution_states.jsonl';rows=[json.loads(x) for x in path.read_text().splitlines()]
                contact=next(r for r in rows if r.get('entry_time_sec') is not None)
                if mode=='time':contact['entry_time_sec']+=.01
                if mode=='key':rows[0]['execution']['count_seq']+=1
                if mode=='plan':rows[0]['plan']['reference_axis_world'][2]+=.1
                if mode=='missing':rows.pop()
                if mode=='extra':rows.append(rows[-1])
                path.write_text(''.join(json.dumps(r)+'\n' for r in rows))
                with self.assertRaises(ValueError):posture.run(sweep,source,db,root/'out')
                self.assertFalse((root/'out'/'execution_contact_posture_metadata.json').exists())

    def test_summary_histogram_and_spin_drift_rejected(self):
        for mode in ('summary','histogram','spin'):
            with self.subTest(mode=mode),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);source,db,sweep=self.fixture(root)
                if mode=='summary':
                    path=sweep/'swing_execution_summary.csv';rows=csv_rows(path)
                    r=next(r for r in rows if r['orientation']=='execution' and r['stratum']=='overall' and int(r['successful_contacts']))
                    r['avg_vla_deg']=str(float(r['avg_vla_deg'])+1);replace_csv(path,rows)
                elif mode=='histogram':
                    path=sweep/'swing_execution_ev_vla.csv';rows=csv_rows(path)
                    r=next(r for r in rows if r['orientation']=='execution');r['vla_band_deg']='90+'
                    replace_csv(path,rows)
                else:
                    dbconn=sqlite3.connect(db);dbconn.execute('UPDATE player_game_pitching SET spin_rate=spin_rate+100');dbconn.commit();dbconn.close()
                with self.assertRaises(ValueError):posture.run(sweep,source,db,root/'out')

    def test_source_alignment_and_metadata_version_rejected(self):
        for mode in ('alignment','version'):
            with self.subTest(mode=mode),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);source,db,sweep=self.fixture(root)
                if mode=='alignment':
                    path=source/'observation_update_inputs.jsonl';lines=path.read_text().splitlines();path.write_text('\n'.join(lines[:-1])+'\n')
                else:
                    path=sweep/'swing_execution_metadata.json';meta=json.loads(path.read_text());meta['version']=1;path.write_text(json.dumps(meta))
                with self.assertRaises(ValueError):posture.run(sweep,source,db,root/'out')


if __name__=='__main__':unittest.main()
