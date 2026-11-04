import json
import tempfile
import unittest
from pathlib import Path

from test_bat_pivot_orientation_probe import source_db
from test_forecast_contact_geometry import csv_rows
from test_statcast_distribution_compare import row,write
import plan_tilt_sweep as sweep
import plan_speed_mixture_compare as compare


class FixedHoldoutTest(unittest.TestCase):
    def test_sampling_exclusion_before_ranking_and_invalid_keys(self):
        with tempfile.TemporaryDirectory() as t:
            p=Path(t)/'population.csv'
            write(p,[dict(game_id=i,count_seq=1,variant='v') for i in range(30)])
            original,_=sweep.sample_keys(p,'v',10,'0')
            # Even the same seed must produce a disjoint sample after exclusion.
            holdout,n=sweep.sample_keys(p,'v',10,'0',set(original))
            self.assertEqual(n,20);self.assertFalse(set(holdout)&set(original))
            self.assertEqual(holdout,sweep.sample_keys(p,'v',10,'0',set(original))[0])
            with self.assertRaises(ValueError):sweep.sample_keys(p,'v',1,'1',{(99,1)})
            with self.assertRaises(ValueError):sweep.sample_keys(p,'v',1,'1',{(i,1) for i in range(30)})

    def test_disjoint_pipeline_frozen_weights_and_overlap_rejection(self):
        with tempfile.TemporaryDirectory() as t:
            root=Path(t);src,db=source_db(root)
            kw=dict(sample_size=1,angles=(15.,),yaws=(0.,),aim_z_offsets=(-.03,0.),speed_ratios=(.6,.8,1.))
            sweep.run(src,db,root/'original',sample_seed='0',**kw)
            prior=root/'original'/'plan_tilt_sample.csv'
            result=sweep.run(src,db,root/'holdout',sample_seed='1',exclude_sample=prior,**kw)
            self.assertEqual(result['maximum_searches'],7)
            before=csv_rows(prior);after=csv_rows(root/'holdout'/'plan_tilt_sample.csv')
            keys=lambda rows:{(r['game_id'],r['count_seq']) for r in rows}
            self.assertFalse(keys(before)&keys(after))
            meta=json.loads((root/'holdout'/'plan_tilt_metadata.json').read_text())
            self.assertEqual(meta['sampling_exclusion']['excluded_count'],1)
            self.assertEqual(meta['population_trials_before_exclusion'],2)
            observed=root/'savant.csv';write(observed,[row(1)])
            tools=Path(__file__).resolve().parents[2]/'tools'/'diagnostics'
            config=tools/'plan_speed_aim_frozen_mixture.json';criteria=tools/'plan_speed_aim_frozen_tolerances.json'
            r=compare.run(root/'holdout',observed,root/'out',mixtures_json=config,tolerances_json=criteria)
            self.assertEqual(r['custom_mixtures'],1)
            out=json.loads((root/'out'/'plan_speed_mixture_metadata.json').read_text())
            frozen=compare.engine.read_json(config)['mixtures']
            self.assertEqual(out['mixtures']['mixture:'+next(iter(frozen))],next(iter(frozen.values())))
            self.assertEqual(out['default_grid'],None)
            # A modified manifest contract cannot silently introduce discovery pitches.
            sample=after[0];meta['sampling_exclusion']['excluded_keys']=[[int(sample['game_id']),int(sample['count_seq'])]]
            (root/'holdout'/'plan_tilt_metadata.json').write_text(json.dumps(meta))
            with self.assertRaisesRegex(ValueError,'overlaps_excluded'):compare.run(root/'holdout',observed,root/'bad')
            self.assertFalse((root/'bad').exists())


if __name__=='__main__':unittest.main()
