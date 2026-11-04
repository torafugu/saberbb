import json
import tempfile
import unittest
from bisect import bisect_right
from collections import Counter
from pathlib import Path

from test_bat_pivot_orientation_probe import source_db
from test_forecast_contact_geometry import csv_rows
from test_statcast_distribution_compare import row,write
import plan_tilt_sweep as sweep
import plan_speed_mixture_compare as compare

TOOLS=Path(__file__).resolve().parents[2]/'tools'/'diagnostics'


class MidpointTest(unittest.TestCase):
    def test_fixed_original_and_preserved_first_moments(self):
        config=json.loads((TOOLS/'plan_midpoint_mixtures.json').read_text())['mixtures']
        original=json.loads((TOOLS/'plan_speed_aim_frozen_mixture.json').read_text())['mixtures']
        self.assertEqual(config['frozen_original'],next(iter(original.values())))
        for weights in config.values():
            speed=Counter();aim=Counter()
            for n,w in weights.items():
                a,s=n.split('_aim_z_')[1].split('mm_speed_r');speed[float(s)]+=w;aim[float(a)/1000]+=w
            self.assertAlmostEqual(sum(weights.values()),1.)
            self.assertAlmostEqual(sum(s*w for s,w in speed.items()),.88)
            self.assertAlmostEqual(sum(a*w for a,w in aim.items()),-.024)
            for n,w in weights.items():
                a,s=n.split('_aim_z_')[1].split('mm_speed_r')
                self.assertAlmostEqual(w,speed[float(s)]*aim[float(a)/1000])

    def test_midpoint_sweep_and_region_boundary_contract(self):
        with tempfile.TemporaryDirectory() as t:
            root=Path(t);src,db=source_db(root)
            result=sweep.run(src,db,root/'sweep',sample_size=0,angles=(15.,),yaws=(0.,),
                aim_z_offsets=(-.03,-.015,0.),speed_ratios=(.6,.8,.9,1.))
            self.assertEqual(result['maximum_searches'],26)
            sweep.run(src,db,root/'endpoints',sample_size=0,angles=(15.,),yaws=(0.,),
                aim_z_offsets=(-.03,0.),speed_ratios=(.6,.8,1.))
            cached=[json.loads(x) for x in (root/'sweep'/'plan_tilt_states.jsonl').read_text().splitlines()]
            for line in (root/'endpoints'/'plan_tilt_states.jsonl').read_text().splitlines():
                endpoint=json.loads(line)
                self.assertIn(endpoint,cached)
            observed=root/'savant.csv'
            write(observed,[row(1,speed=str(42/.44704),angle='15'),
                            row(2,speed=str(32/.44704),angle='-45'),
                            row(3,speed=str(47/.44704),angle='45')])
            compare.run(root/'sweep',observed,root/'out',mixtures_json=TOOLS/'plan_midpoint_mixtures.json')
            rows=csv_rows(root/'out'/'plan_speed_mixture_summary.csv')
            self.assertEqual(len([r for r in rows if r['cohort']=='fair_direction' and r['mixture'].startswith('mixture:')]),4)
            for r in rows:
                if r['cohort']=='fair_direction':
                    for field in ['ev_40to45','ev_25to35','vla_0to30','vla_neg60to_neg30','vla_40to50']:
                        self.assertAlmostEqual(float(r['statcast_'+field+'_fraction']),1/3)
            # Upper bounds excluded, lower bounds included for the new residual regions.
            cells=Counter((bisect_right(compare.engine.EV_EDGES,e),bisect_right(compare.engine.VLA_EDGES,v))
                for e,v in [(40,0),(45,30),(25,-60),(35,-30),(42,40),(42,50)])
            state=dict(config=dict(planned_effort=1.,anchor_fraction=.5,start_offset_sec=0.),
                counts=dict(trials=6,evaluable_trials=6,execution_clock_invalid_trials=0,
                    evaluable_geometry_contacts=6,successful_contacts=6,fair_contacts=6),
                grids={c:cells for c in compare.engine.COHORTS})
            compare.engine.compare({'edges':state},{},[],observed,root/'edges')
            r=next(r for r in csv_rows(root/'edges'/'execution_mixture_summary.csv') if r['cohort']=='fair_direction')
            self.assertAlmostEqual(float(r['ev_40to45_fraction']),.5)
            for field in ['ev_25to35','vla_0to30','vla_neg60to_neg30','vla_40to50']:
                self.assertAlmostEqual(float(r[field+'_fraction']),1/6)


if __name__=='__main__':unittest.main()
