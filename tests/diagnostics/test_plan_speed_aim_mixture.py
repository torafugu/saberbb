import json
import tempfile
import unittest
from collections import Counter
from pathlib import Path
from unittest.mock import patch

from test_bat_pivot_orientation_probe import source_db
from test_statcast_distribution_compare import row,write
from test_forecast_contact_geometry import csv_rows
import plan_tilt_sweep as sweep
import plan_speed_mixture_compare as compare


class SpeedAimMixtureTest(unittest.TestCase):
    def fixture(self,root):
        src,db=source_db(root);folder=root/'joint'
        sweep.run(src,db,folder,sample_size=0,angles=(0.,15.),yaws=(0.,),
            aim_z_offsets=(-.03,0.),speed_ratios=(.4,.6,.8,1.))
        return folder

    def test_full_joint_grid_and_marginals(self):
        with tempfile.TemporaryDirectory() as t:
            folder=self.fixture(Path(t));states,_,_,_=compare.load_states(folder,compare.engine.DEFAULT_MODEL)
            mixes=compare.joint_mixtures(states)
            self.assertEqual(len(mixes),770)
            for weights in mixes.values():
                self.assertAlmostEqual(sum(weights.values()),1.)
                speed=Counter();aim=Counter();families=set()
                for n,w in weights.items():
                    c=states[n]['config'];speed[c['planned_speed_ratio']]+=w;aim[c['aim_z_offset_m']]+=w
                    families.add((c['tilt_deg'],c['yaw_batter_deg']))
                self.assertEqual(len(families),1);self.assertGreaterEqual(speed[1.]+1e-12,.6)
                for n,w in weights.items():
                    c=states[n]['config']
                    self.assertAlmostEqual(w,speed[c['planned_speed_ratio']]*aim[c['aim_z_offset_m']])
            # Independent factors use the actual joint response, not a product of outcome distributions.
            weights=next(w for w in mixes.values() if len(w)==4)
            mass,p=compare.engine.mix(states,weights,'fair_direction')
            expected=sum(w*states[n]['counts']['fair_contacts']/states[n]['counts']['evaluable_trials'] for n,w in weights.items())
            self.assertAlmostEqual(p,expected);self.assertAlmostEqual(sum(mass.values()),expected)
            broken=dict(states);broken.pop(next(iter(broken)))
            with self.assertRaisesRegex(ValueError,'missing_speed_aim_cell'):compare.joint_mixtures(broken)

    def test_cached_comparison_and_omitted_grid(self):
        with tempfile.TemporaryDirectory() as t:
            root=Path(t);folder=self.fixture(root);observed=root/'savant.csv';write(observed,[row(1)])
            with patch('sqlite3.connect',side_effect=AssertionError('DB')), \
                 patch('bat_pivot_orientation_probe.search',side_effect=AssertionError('search')), \
                 patch('finite_barrel_collision.solve',side_effect=AssertionError('solve')):
                result=compare.run(folder,observed,root/'out',step=.5,min_full=.5,mix_aim_offsets=True)
            self.assertEqual(result['custom_mixtures'],24);self.assertEqual(result['states'],16)
            self.assertFalse((root/'out'/'plan_speed_mixture_ev_vla.csv').exists())
            meta=json.loads((root/'out'/'plan_speed_mixture_metadata.json').read_text())
            self.assertEqual(meta['speed_aim_weight_assumption'],'independent_trial_weights_within_tilt_yaw')
            self.assertFalse(meta['ev_vla_grid_written'])
            self.assertTrue(csv_rows(root/'out'/'plan_speed_mixture_components.csv'))
            # Omitting grid storage must leave all computed metrics unchanged.
            states,source,paths,_=compare.load_states(folder,compare.engine.DEFAULT_MODEL)
            compare.engine.compare(states,source,paths,observed,root/'withgrid',
                default_mixtures=compare.joint_mixtures(states,.5,.5),prefix=compare.PREFIX)
            self.assertEqual(csv_rows(root/'out'/'plan_speed_mixture_summary.csv'),
                             csv_rows(root/'withgrid'/'plan_speed_mixture_summary.csv'))


if __name__=='__main__':unittest.main()
