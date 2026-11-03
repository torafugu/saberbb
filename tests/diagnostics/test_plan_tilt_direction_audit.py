import copy
import json
import math
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_bat_pivot_orientation_probe import source_db
from test_forecast_contact_geometry import csv_rows,replace_csv
import plan_tilt_sweep as sweep
import plan_tilt_direction_audit as audit


class TiltDirectionTest(unittest.TestCase):
    def fixture(self,root):
        source,db=source_db(root);out=root/'sweep'
        sweep.run(source,db,out,sample_size=0)
        return source,db,out

    def synthetic(self,side,out):
        ev,hla,vla=audit.angles(out)
        p=dict(forecast_position_world_m=[0,0,.7],ideal_axis_world=[side,0,0],nominal_time_sec=.4,
               ideal_velocity_world_ms=[0,35,0],execution_angle_error_deg=0)
        r=dict(forecast_plan=p,kinematics=dict.fromkeys(sweep.KINEMATICS,0),search_status='contact',
               entry_time_sec=.4,contact=dict(status='contact'),responses={'m':dict(collision_status='impulse_contact',
                   outgoing_world_ms=out,ev_ms=ev,hla_deg=hla,vla_deg=vla)})
        return {'saved_tilt':copy.deepcopy(r),'tilt_0deg':copy.deepcopy(r)}

    def test_mirror_by_side_circular_wrap_and_undefined_hla(self):
        a=audit.Audit()
        a.add('s',self.synthetic(1,[20,40,10]),['m'])
        a.add('s',self.synthetic(-1,[-20,40,10]),['m'])
        with tempfile.TemporaryDirectory() as tmp:
            a.write(Path(tmp));rows=csv_rows(Path(tmp)/'plan_tilt_direction_summary.csv')
            rs=[r for r in rows if r['scenario']=='tilt_0deg' and r['cohort']=='fair_direction' and r['stratum']=='handedness']
            self.assertEqual(len(rs),2)
            self.assertAlmostEqual(float(rs[0]['batter_relative_circular_mean_hla_deg']),float(rs[1]['batter_relative_circular_mean_hla_deg']))
            overall=next(r for r in rows if (r['scenario'],r['stratum'],r['cohort'])==('tilt_0deg','overall','fair_direction'))
            self.assertAlmostEqual(float(overall['world_circular_mean_hla_deg']),0.)
            self.assertGreater(float(overall['batter_relative_circular_mean_hla_deg']),20.)
        a=audit.Audit()
        for deg in (179,-179):
            rad=math.radians(deg);a.add('s',self.synthetic(1,[40*math.sin(rad),40*math.cos(rad),0]),['m'])
        g=a.summary['s','tilt_0deg','overall','all','all','m','all_successful']
        self.assertAlmostEqual(abs(math.degrees(math.atan2(g['sin_hla'],g['cos_hla']))),180.)
        a=audit.Audit();a.add('s',self.synthetic(1,[0,0,30]),['m'])
        with tempfile.TemporaryDirectory() as tmp:
            a.write(Path(tmp));rows=csv_rows(Path(tmp)/'plan_tilt_direction_summary.csv')
            self.assertTrue(all(r['central_abs_lt10_fraction']=='' for r in rows))
            self.assertTrue(all(r['hla_band_deg']=='undefined' for r in csv_rows(Path(tmp)/'plan_tilt_direction_histogram.csv')))

    def test_no_db_search_or_collision_replay_and_partitions(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);_,_,source=self.fixture(root)
            before={p:audit.digest(p) for p in source.glob('*.*')}
            with patch('sqlite3.connect',side_effect=AssertionError('DB read')), \
                 patch('bat_pivot_orientation_probe.search',side_effect=AssertionError('search')), \
                 patch('finite_barrel_collision.solve',side_effect=AssertionError('collision replay')):
                result=audit.run([source],root/'out')
            self.assertEqual(result,dict(datasets=1,new_searches=0,db_reads=0))
            self.assertEqual(before,{p:audit.digest(p) for p in before})
            rows=csv_rows(root/'out'/'plan_tilt_direction_summary.csv')
            hist=csv_rows(root/'out'/'plan_tilt_direction_histogram.csv')
            for r in rows:
                key=['dataset','scenario','stratum','handedness','forecast_height_band_m','model','cohort']
                for frame in ('world','batter_relative'):
                    cells=[c for c in hist if c['angle_frame']==frame and all(c[k]==r[k] for k in key)]
                    self.assertEqual(sum(int(c['contacts']) for c in cells),int(r['contacts']))
                    self.assertAlmostEqual(sum(float(c['fraction']) for c in cells),1.)
                if r['stratum']=='overall':
                    for stage in ('handedness','forecast_height','handedness_height'):
                        children=[c for c in rows if c['stratum']==stage and all(c[k]==r[k] for k in ('dataset','scenario','model','cohort'))]
                        self.assertEqual(sum(int(c['contacts']) for c in children),int(r['contacts']))
            with self.assertRaises(FileExistsError):audit.run([source],root/'out')

    def test_seeds_separate_even_when_sample_keys_overlap(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,db,seed0=self.fixture(root)
            sweep.run(source,db,root/'seed1',sample_size=0,sample_seed='1',angles=(0.,15.))
            self.assertEqual(audit.run([seed0,root/'seed1'],root/'out')['datasets'],2)
            rows=csv_rows(root/'out'/'plan_tilt_direction_selection.csv')
            for r in rows:
                if r['stratum']=='overall':self.assertEqual(int(r['trials']),2)
            with self.assertRaisesRegex(ValueError,'duplicate_seed'):audit.run([seed0,seed0],root/'duplicate')

    def test_invalid_clocks_retained(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,db=source_db(root)
            p=source/'observation_update_inputs.jsonl';rows=[json.loads(x) for x in p.read_text().splitlines()]
            for r in rows:
                if r['observations']:r['observations'][-1]['time_sec']=r['plan']['nominal_time_sec']-.001
            p.write_text(''.join(json.dumps(r)+'\n' for r in rows))
            sweep.run(source,db,root/'sweep',sample_size=0)
            audit.run([root/'sweep'],root/'out')
            self.assertEqual(csv_rows(root/'out'/'plan_tilt_direction_summary.csv'),[])
            for r in csv_rows(root/'out'/'plan_tilt_direction_selection.csv'):
                self.assertEqual(r['evaluable_trials'],'0');self.assertEqual(r['trials'],r['clock_invalid_trials'])

    def test_corrupt_vectors_rows_summary_and_manifest_rejected(self):
        for mode in ('vector','duplicate','missing','summary','histogram','manifest','kinematics'):
            with self.subTest(mode=mode),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);_,_,source=self.fixture(root)
                if mode in ('vector','duplicate','missing'):
                    p=source/'plan_tilt_states.jsonl';rows=[json.loads(x) for x in p.read_text().splitlines()]
                    if mode=='vector':
                        m=next(m for r in rows for m in r['responses'].values() if m['collision_status']=='impulse_contact');m['hla_deg']+=1
                    if mode=='duplicate':rows.insert(1,rows[0])
                    if mode=='missing':rows.pop()
                    p.write_text(''.join(json.dumps(r)+'\n' for r in rows))
                else:
                    name={'summary':'plan_tilt_summary.csv','histogram':'plan_tilt_ev_vla.csv','manifest':'plan_tilt_sample.csv','kinematics':'plan_tilt_kinematics.csv'}[mode]
                    p=source/name;rows=csv_rows(p)
                    if mode=='summary':rows.pop(0)
                    if mode=='histogram':rows.pop(0)
                    if mode=='manifest':rows[0]['selection_hash']='bad'
                    if mode=='kinematics':rows[0]['avg_planned_speed_ms']='1000'
                    replace_csv(p,rows)
                with self.assertRaises(ValueError):audit.run([source],root/'out')
                self.assertFalse((root/'out').exists())


if __name__=='__main__':unittest.main()
