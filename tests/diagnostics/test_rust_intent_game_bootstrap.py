import copy
from collections import Counter,defaultdict
import csv
import json
import os
from pathlib import Path
import random
import subprocess
import sys
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools/diagnostics'))

from test_rust_intent_collision_probe import sample,database_with_spin
from contact_collision_probe import write_csv
import rust_intent_collision_probe as collision
import rust_intent_game_bootstrap as bootstrap


def replace_csv(path,rows):
    with path.open('w',newline='') as f:
        writer=csv.DictWriter(f,fieldnames=list(rows[0]))
        writer.writeheader();writer.writerows(rows)


def source_fixture(root):
    """Make real solver output, then label synthetic game clusters for cache tests."""
    db=root/'test.db';folder=root/'source'
    database_with_spin(db,[(1,sample(),None),(2,sample(seq=2),None)])
    collision.run(db,1,1,folder)
    details=folder/'rust_intent_collision_details.csv'
    rows=bootstrap.read_csv(details)
    # This tests the cached statistical contract, not a second physical replay.
    for row in rows:
        row['game_id']=row['count_seq']
    replace_csv(details,rows)
    path=folder/'rust_intent_collision_metadata.json'
    metadata=json.loads(path.read_text());metadata['last_game_id']=2
    path.write_text(json.dumps(metadata))
    return folder


def rewrite_csv(path,mutate):
    rows=bootstrap.read_csv(path); mutate(rows)
    replace_csv(path,rows)


def synthetic_vectors():
    # 100 pitches in game 1 vs 1 pitch in game 2: ratio of pooled counts differs
    # sharply from an equally weighted mean of per-game percentages.
    result=[]
    for n,geometry_after in [(100,100),(1,0)]:
        a=Counter(plans=n,searched=n,geometry=0)
        b=Counter(plans=n,searched=n,geometry=geometry_after)
        v=tuple([g[k] for g in (a,b) for k in bootstrap.SIDE_FIELDS]+[0]*len(bootstrap.PAIR_FIELDS))
        result.append([v,v])
    return result


class IntentGameBootstrapTest(unittest.TestCase):
    def test_whole_game_draw_is_shared_paired_and_pooled(self):
        vectors=synthetic_vectors()
        point=bootstrap.metrics(bootstrap.totals(vectors,Counter({0:1,1:1}))[0])
        self.assertAlmostEqual(point['geometry_pct_per_searched_delta_pp'],10000/101)
        self.assertIsNone(point['downward_pct_among_fair_delta_pp'])
        random.seed(123);before=random.getstate()
        draws=list(bootstrap.bootstrap(vectors,50,'fixed'))
        self.assertEqual(random.getstate(),before)
        self.assertEqual(draws,list(bootstrap.bootstrap(vectors,50,'fixed')))
        allowed={0.,100.,10000/101}
        for x,y in draws:
            self.assertEqual(x,y)  # Same game multiplicities across all strata.
            self.assertTrue(any(abs(x['geometry_pct_per_searched_delta_pp']-v)<1e-10 for v in allowed))

    def test_identical_intents_have_zero_differences_in_every_draw(self):
        sides=Counter(plans=10,searched=10,geometry=8,successful=8,fair=6,downward=2,high_vla=1,line_window=3,
            fair_n_ev_ms=6,fair_sum_ev_ms=240.,fair_n_vla_deg=6,fair_sum_vla_deg=60.,fair_n_spin_rpm=6,fair_sum_spin_rpm=12000.)
        pair=Counter(common_fair=6)
        vector=tuple([sides[k] for _ in range(2) for k in bootstrap.SIDE_FIELDS]+[pair[k] for k in bootstrap.PAIR_FIELDS])
        for draw in bootstrap.bootstrap([[vector],[tuple(2*x for x in vector)]],20,'zero'):
            self.assertTrue(all(value==0 for value in draw[0].values()))

    def test_zero_effects_and_point_reaggregation_match_collision_cache(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);folder=source_fixture(root)
            meta,aggregate,games,checks,_=bootstrap.load_cache(folder)
            self.assertEqual(sorted(games),[1,2])
            self.assertLess(max(checks.values()),1e-8)
            keys,vectors=bootstrap.game_vectors(aggregate,games)
            point=bootstrap.totals(vectors,Counter({0:1,1:1}))
            source=aggregate.summaries()
            for key,v in zip(keys,point):
                a=next(r for r in source if r['scope']=='all_plans' and r['intent']=='contact_first' and tuple(r[k] for k in bootstrap.GROUP_FIELDS)==key)
                b=next(r for r in source if r['scope']=='all_plans' and r['intent']=='line_drive' and tuple(r[k] for k in bootstrap.GROUP_FIELDS)==key)
                expected=b['geometry_pct_per_searched']-a['geometry_pct_per_searched']
                self.assertAlmostEqual(bootstrap.metrics(v)['geometry_pct_per_searched_delta_pp'],expected)
            result=bootstrap.run(folder,root/'out',30,'same')
            self.assertEqual(result['observed_games'],2)
            self.assertEqual(result['matched_pitches'],2)
            summary=bootstrap.read_csv(root/'out/rust_intent_game_bootstrap_summary.csv')
            self.assertTrue(all(r['valid_replicates']=='30' for r in summary))
            metadata=json.loads((root/'out/rust_intent_game_bootstrap_metadata.json').read_text())
            for key in ('db_reads','new_searches','collision_replays'):
                self.assertEqual(metadata[key],0)
            self.assertEqual(metadata['source_metadata']['model'],collision.MODEL)
            with self.assertRaises(FileExistsError):bootstrap.run(folder,root/'out',30)

    def test_corruption_and_missing_pairs_are_rejected_without_outputs(self):
        mutations=[
            ('details.csv',lambda rows:rows.pop()),
            ('details.csv',lambda rows:rows[0].update(selected='False')),
            ('details.csv',lambda rows:rows[0].update(downward='True' if rows[0]['downward']=='False' else 'False')),
            ('details.csv',lambda rows:rows[0].update(aim_z_offset_m='0.1')),
            ('details.csv',lambda rows:rows[0].update(forecast_height_band_m='<0.6')),
            ('summary.csv',lambda rows:rows[0].update(plans='999')),
            ('pairs.csv',lambda rows:rows[0].update(common_fair='999')),
            ('status.csv',lambda rows:rows[0].update(plans='999')),
            ('coverage.csv',lambda rows:rows[0].update(count='999')),
            ('checks.csv',lambda rows:rows[0].update(max_error='nan'))]
        for suffix,mutate in mutations:
            with self.subTest(suffix=suffix),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);folder=source_fixture(root)
                rewrite_csv(folder/('rust_intent_collision_'+suffix),mutate)
                with self.assertRaises(ValueError):bootstrap.run(folder,root/'out',2)
                self.assertFalse((root/'out').exists())

    def test_sparse_cohort_undefined_replicates_never_become_zero_or_conditional_ci(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);folder=source_fixture(root)
            # Put game 2 in a distinct strategy cohort; some whole-game draws
            # then omit a cohort completely, making its denominator undefined.
            detail=folder/'rust_intent_collision_details.csv'
            rows=bootstrap.read_csv(detail)
            for r in rows:
                if r['game_id']=='2':r.update(strategy='seek_line_drive',decision_reason='manager_line_drive_priority',selected=str(r['intent']=='line_drive'))
            replace_csv(detail,rows)
            aggregate=collision.Aggregate();statuses=Counter()
            for start in range(0,len(rows),2):
                pair=[bootstrap.parse_row(r) for r in rows[start:start+2]]
                aggregate.add(pair)
                for r in pair:statuses[r['intent'],r['selected'],r['collision_status']]+=1
            for suffix,data in [('summary',aggregate.summaries()),('pairs',aggregate.paired_rows()),
                ('status',[dict(intent=k[0],selected=k[1],collision_status=k[2],plans=v) for k,v in sorted(statuses.items())])]:
                replace_csv(folder/('rust_intent_collision_'+suffix+'.csv'),data)
            bootstrap.run(folder,root/'out',50,'sparse')
            summary=bootstrap.read_csv(root/'out/rust_intent_game_bootstrap_summary.csv')
            for r in summary:
                self.assertGreater(int(r['undefined_replicates']),0)
                self.assertEqual(r['ci_low'],'');self.assertEqual(r['ci_high'],'')
                self.assertEqual(r['interval_status'],'undefined_replicates')

    def test_cli_reproducibility_without_db_environment_and_frozen_hash_rejection(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);folder=source_fixture(root)
            cmd=[sys.executable,str(ROOT/'tools/diagnostics/rust_intent_game_bootstrap.py'),
                '--input-dir',str(folder),'--replicates','20','--bootstrap-seed','test']
            for name in ('a','b'):
                p=subprocess.run(cmd+['--output-dir',str(root/name)],env=dict(os.environ,SABERBB_DB=''),capture_output=True,text=True)
                self.assertEqual(p.returncode,0,p.stderr)
            self.assertEqual((root/'a/rust_intent_game_bootstrap_summary.csv').read_bytes(),(root/'b/rust_intent_game_bootstrap_summary.csv').read_bytes())
            path=folder/'rust_intent_collision_metadata.json';meta=json.loads(path.read_text())
            key=next(k for k in meta['inputs'] if Path(k).name=='pre_swing_aim_seed2_frozen.json')
            meta['inputs'][key]='0'*64;path.write_text(json.dumps(meta))
            with self.assertRaisesRegex(ValueError,'frozen_source_hash_mismatch'):bootstrap.run(folder,root/'bad',20)


if __name__=='__main__':unittest.main()
