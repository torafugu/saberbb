import json
import tempfile
import unittest
from collections import Counter
from pathlib import Path
from unittest.mock import patch

from test_bat_pivot_orientation_probe import source_db
from test_forecast_contact_geometry import csv_rows
from test_statcast_distribution_compare import row,write
import plan_tilt_sweep as sweep
import plan_speed_mixture_compare as compare
import plan_speed_mixture_bootstrap as boot

TOOLS=Path(__file__).resolve().parents[2]/'tools'/'diagnostics'
LOW=(3,3)  # EV [20,25), VLA [-30,0)
HIGH=(9,10)  # EV [50,55), VLA [60,90)


class BootstrapTest(unittest.TestCase):
    def test_joint_pitch_draws_preserve_opposing_condition_covariance(self):
        # Independent condition resampling would invent low/high fluctuations here.
        pitches={(1,1):{'a':(True,True,LOW),'b':(True,True,HIGH)},
                 (1,2):{'a':(True,True,HIGH),'b':(True,True,LOW)}}
        vectors=boot.pitch_vectors(pitches,{'a':.5,'b':.5})
        obs=Counter({LOW:1,HIGH:1})
        rows=boot.resample(vectors,obs,50,'9',{'vla_ge50_delta_pp':(-3,3)})
        self.assertTrue(all(r['grid_total_variation']==0 for r in rows))
        self.assertTrue(all(r['vla_ge50_delta_pp']==0 for r in rows))
        self.assertEqual(rows,boot.resample(vectors,obs,50,'9',{'vla_ge50_delta_pp':(-3,3)}))

    def test_post_selection_normalization_zero_and_clock_pitches(self):
        pitches={(1,1):{'a':(True,True,LOW),'b':(True,False,None)},
                 (1,2):{'a':(True,True,HIGH),'b':(True,True,HIGH)},
                 (1,3):{'a':(True,False,None),'b':(True,False,None)},
                 (1,4):{'a':(False,False,None),'b':(False,False,None)}}
        vectors=boot.pitch_vectors(pitches,{'a':.25,'b':.75})
        obs=Counter({LOW:1,HIGH:1})
        point=boot.metrics(vectors,Counter({i:1 for i in range(4)}),obs)
        self.assertAlmostEqual(point['ev_lt25_delta_pp'],-30)  # .25 / 1.25 = .2
        self.assertAlmostEqual(point['fair_probability_per_evaluable_trial'],1.25/3)
        self.assertAlmostEqual(point['geometry_contact_probability'],1.25/3)
        self.assertAlmostEqual(point['grid_total_variation'],.3)
        empty=boot.metrics(vectors,Counter({2:4}),obs)
        self.assertIsNone(empty['grid_total_variation'])
        self.assertEqual(empty['fair_probability_per_evaluable_trial'],0)
        self.assertFalse(boot.within(empty,{'grid_total_variation':(0,.25)}))
        invalid=boot.metrics(vectors,Counter({3:4}),obs)
        self.assertIsNone(invalid['geometry_contact_probability'])
        self.assertIsNone(invalid['vla_ge50_delta_pp'])
        with self.assertRaisesRegex(ValueError,'inconsistent_clock'):
            boot.pitch_vectors({(1,1):{'a':(True,False,None),'b':(False,False,None)}},{'a':.5,'b':.5})

    def test_quantiles_and_seed_variation(self):
        self.assertEqual(boot.quantile([0,10],.025),.25)
        self.assertEqual(boot.quantile([0,10],.975),9.75)
        self.assertIsNone(boot.quantile([],.5))
        vectors=[(1,1,((LOW,1.),)),(1,1,((HIGH,1.),))]
        obs=Counter({LOW:1,HIGH:1})
        a=boot.resample(vectors,obs,30,'0',{})
        b=boot.resample(vectors,obs,30,'1',{})
        self.assertNotEqual(a,b)
        self.assertGreater(len({r['ev_lt25_delta_pp'] for r in a}),1)

    def test_full_cache_pipeline_reproduces_compare_without_db_or_search(self):
        with tempfile.TemporaryDirectory() as t:
            root=Path(t);src,db=source_db(root)
            sweep.run(src,db,root/'cache',sample_size=2,angles=(15.,),yaws=(0.,),
                aim_z_offsets=(-.03,-.015,0.),speed_ratios=(.6,.8,.9,1.))
            observed=root/'savant.csv';write(observed,[row(1),row(2,speed='40',angle='60')])
            config=TOOLS/'plan_midpoint_frozen_mixture.json';criteria=TOOLS/'plan_speed_aim_frozen_tolerances.json'
            compare.run(root/'cache',observed,root/'compare',mixtures_json=config,tolerances_json=criteria)
            expected=next(r for r in csv_rows(root/'compare'/'plan_speed_mixture_summary.csv')
                if r['mixture']=='mixture:both_midpoints_same_means' and r['cohort']=='fair_direction')
            # The cache is sufficient; the original DB is deliberately removed.
            db.unlink()
            with patch('sqlite3.connect',side_effect=AssertionError('unexpected DB read')), \
                 patch.object(sweep,'evaluate',side_effect=AssertionError('unexpected search')), \
                 patch.object(sweep,'event',side_effect=AssertionError('unexpected collision replay')):
                result=boot.run(root/'cache',observed,root/'out',config,criteria,replicates=25)
                boot.run(root/'cache',observed,root/'repeat',config,criteria,replicates=25)
            self.assertEqual(result['sampled_trials'],2)
            self.assertEqual(result['new_searches'],0);self.assertEqual(result['db_reads'],0)
            summary={r['metric']:r for r in csv_rows(root/'out'/(boot.PREFIX+'_summary.csv'))}
            for metric in boot.REGIONS:
                actual=float(summary[metric+'_delta_pp']['point_estimate'])
                delta=100*(float(expected[metric+'_fraction'])-float(expected['statcast_'+metric+'_fraction']))
                self.assertAlmostEqual(actual,delta)
            self.assertAlmostEqual(float(summary['grid_total_variation']['point_estimate']),float(expected['grid_total_variation']))
            self.assertAlmostEqual(float(summary['fair_probability_per_evaluable_trial']['point_estimate']),float(expected['cohort_probability_per_evaluable_trial']))
            self.assertEqual((root/'out'/(boot.PREFIX+'_replicates.csv')).read_bytes(),
                             (root/'repeat'/(boot.PREFIX+'_replicates.csv')).read_bytes())
            meta=json.loads((root/'out'/(boot.PREFIX+'_metadata.json')).read_text())
            self.assertEqual(meta['resampling_unit'],'whole_pitch_all_conditions')
            self.assertFalse(meta['statcast_resampled'])
            self.assertEqual(meta['weights'],json.loads(config.read_text())['mixtures']['both_midpoints_same_means'])
            self.assertEqual(meta['tolerances'],json.loads(criteria.read_text()))
            self.assertLess(max(float(r['max_error']) for r in csv_rows(root/'out'/(boot.PREFIX+'_checks.csv'))),1e-7)
            with self.assertRaises(FileExistsError):boot.run(root/'cache',observed,root/'out',config,criteria,replicates=1)
            for n in (0,-1,True,1.5):
                with self.assertRaisesRegex(ValueError,'invalid_bootstrap'):boot.run(root/'cache',observed,root/'bad',config,criteria,replicates=n)
            with self.assertRaisesRegex(ValueError,'exactly_one'):
                boot.run(root/'cache',observed,root/'bad',TOOLS/'plan_midpoint_mixtures.json',criteria,replicates=1)
            self.assertFalse((root/'bad').exists())


if __name__=='__main__':unittest.main()
