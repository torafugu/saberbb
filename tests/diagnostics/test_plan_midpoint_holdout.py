import csv
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from test_bat_pivot_orientation_probe import source_db
from test_forecast_contact_geometry import csv_rows
from test_statcast_distribution_compare import row,write
import plan_tilt_sweep as sweep
import plan_speed_mixture_compare as compare

TOOLS=Path(__file__).resolve().parents[2]/'tools'/'diagnostics'


class MidpointHoldoutTest(unittest.TestCase):
    def test_union_sampling_and_manifest_validation(self):
        with tempfile.TemporaryDirectory() as t:
            root=Path(t);population=root/'population.csv'
            write(population,[dict(game_id=i,count_seq=1,variant='v') for i in range(30)])
            a=root/'seed0.csv';b=root/'seed1.csv'
            write(a,[dict(game_id=i,count_seq=1) for i in range(10)])
            write(b,[dict(game_id=i,count_seq=1) for i in range(5,15)])
            ex=sweep.exclusion_manifest([a,b])
            self.assertEqual(ex['excluded_count'],15)
            self.assertEqual(len(ex['manifests']),2)
            for source in ex['manifests']:
                self.assertEqual(source['excluded_count'],10)
                self.assertEqual(source['sha256'],sweep.digest(source['path']))
            keys,n=sweep.sample_keys(population,'v',10,'2',ex['excluded_keys'])
            self.assertEqual(n,15);self.assertEqual(len(keys),10)
            self.assertFalse(set(keys)&set(ex['excluded_keys']))
            self.assertEqual(sweep.exclusion_manifest([a]),sweep.exclusion_manifest(a))
            self.assertIsNone(sweep.exclusion_manifest([]))
            # Three successive seeds: the third excludes both discovery samples.
            seed0,_=sweep.sample_keys(population,'v',10,'0')
            seed1,_=sweep.sample_keys(population,'v',10,'1',seed0)
            for path,selected in ((a,seed0),(b,seed1)):
                write(path,[dict(game_id=k[0],count_seq=k[1]) for k in selected])
            union=sweep.exclusion_manifest([a,b])
            seed2,n=sweep.sample_keys(population,'v',10,'2',union['excluded_keys'])
            self.assertEqual(union['excluded_count'],20);self.assertEqual(n,10)
            self.assertFalse(set(seed2)&(set(seed0)|set(seed1)))
            for rows,error in [([], 'empty_exclusion'),([dict(game_id=1,count_seq=1)]*2,'duplicate'),
                               ([dict(game_id=-1,count_seq=1)],'invalid')]:
                with b.open('w',newline='') as f:
                    writer=csv.DictWriter(f,fieldnames=['game_id','count_seq']);writer.writeheader();writer.writerows(rows)
                with self.assertRaisesRegex(ValueError,error):sweep.exclusion_manifest([a,b])

    def test_repeatable_cli_cached_union_and_frozen_comparison(self):
        with tempfile.TemporaryDirectory() as t:
            root=Path(t);src,db=source_db(root)
            sweep.run(src,db,root/'prior',sample_size=1,sample_seed='0',angles=(15.,))
            a=root/'prior'/'plan_tilt_sample.csv';b=root/'copy.csv';b.write_bytes(a.read_bytes())
            # Overlap between prior files is counted once; exercise the actual CLI.
            result=subprocess.run([sys.executable,str(TOOLS/'plan_speed_sweep.py'),
                '--input-dir',str(src),'--db',str(db),'--output-dir',str(root/'holdout'),
                '--sample-size','1','--sample-seed','2','--exclude-sample',str(a),'--exclude-sample',str(b),
                '--tilts-deg','15','--speed-ratios','.6','.8','.9','1','--aim-z-mm','-30','-15','0'],
                capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stderr)
            meta_path=root/'holdout'/'plan_tilt_metadata.json'
            meta=json.loads(meta_path.read_text());ex=meta['sampling_exclusion']
            self.assertEqual(ex['excluded_count'],1)
            self.assertEqual(meta['population_trials'],1)
            self.assertEqual(meta['population_trials_before_exclusion'],2)
            self.assertEqual(len(ex['manifests']),2)
            prior={(r['game_id'],r['count_seq']) for r in csv_rows(a)}
            actual={(r['game_id'],r['count_seq']) for r in csv_rows(root/'holdout'/'plan_tilt_sample.csv')}
            self.assertFalse(prior&actual)
            config=TOOLS/'plan_midpoint_frozen_mixture.json'
            weights=json.loads(config.read_text())['mixtures']
            original=json.loads((TOOLS/'plan_midpoint_mixtures.json').read_text())['mixtures']
            self.assertEqual(weights,{'both_midpoints_same_means':original['both_midpoints_same_means']})
            self.assertAlmostEqual(sum(next(iter(weights.values())).values()),1)
            observed=root/'savant.csv';write(observed,[row(1)])
            # Sources can be moved after sampling; audit uses embedded keys and hashes.
            a.unlink();b.unlink()
            result=compare.run(root/'holdout',observed,root/'out',mixtures_json=config,
                tolerances_json=TOOLS/'plan_speed_aim_frozen_tolerances.json')
            self.assertEqual(result['custom_mixtures'],1)
            out=json.loads((root/'out'/'plan_speed_mixture_metadata.json').read_text())
            self.assertEqual(out['mixtures']['mixture:both_midpoints_same_means'],weights['both_midpoints_same_means'])
            self.assertIsNone(out['default_grid'])
            # Preserve the top-level union but tamper a source; cached diagnostics reject it.
            ex['manifests'][0]['excluded_keys']=[[999,1]]
            meta_path.write_text(json.dumps(meta))
            with self.assertRaisesRegex(ValueError,'exclusion_union_mismatch'):
                compare.run(root/'holdout',observed,root/'bad')
            self.assertFalse((root/'bad').exists())


if __name__=='__main__':unittest.main()
