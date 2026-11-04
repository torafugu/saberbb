import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_bat_pivot_orientation_probe import source_db
from test_statcast_distribution_compare import write,row
from test_forecast_contact_geometry import csv_rows
import test_swing_execution_mixture_compare as legacy
import plan_tilt_sweep as sweep
import plan_speed_mixture_compare as compare


class PlannedMixtureTest(unittest.TestCase):
    def fixture(self,root):
        source,db=source_db(root);folder=root/'speed'
        sweep.run(source,db,folder,sample_size=0,angles=(0.,15.),yaws=(0.,),aim_z_offsets=(0.,),speed_ratios=(.4,.6,.8,1.))
        statcast=root/'savant.csv';write(statcast,[row(1),row(2,speed=''),row(3,kind='S')])
        return folder,statcast

    def test_default_grid_complete_weights_and_geometry_families(self):
        with tempfile.TemporaryDirectory() as t:
            folder,_=self.fixture(Path(t));states,_,_,_=compare.load_states(folder,compare.engine.DEFAULT_MODEL)
            mixes=compare.default_mixtures(states)
            self.assertEqual(len(mixes),330)
            for weights in mixes.values():
                self.assertAlmostEqual(sum(weights.values()),1.)
                self.assertGreaterEqual(sum(w for n,w in weights.items() if states[n]['config']['planned_speed_ratio']==1.),.6)
                self.assertEqual(len({states[n]['config']['tilt_deg'] for n in weights}),1)
            for step,minimum in [(0.,.6),(.03,.6),(.05,-.1),(.001,0.)]:
                with self.subTest(step=step),self.assertRaises(ValueError):compare.default_mixtures(states,step,minimum)

    def test_pipeline_validates_cache_and_uses_no_db_or_physics(self):
        with tempfile.TemporaryDirectory() as t:
            root=Path(t);folder,statcast=self.fixture(root)
            with patch('sqlite3.connect',side_effect=AssertionError('DB')), \
                 patch('bat_pivot_orientation_probe.search',side_effect=AssertionError('search')), \
                 patch('finite_barrel_collision.solve',side_effect=AssertionError('solve')):
                r=compare.run(folder,statcast,root/'out',step=.5,min_full=.5)
            self.assertEqual(r['states'],8);self.assertEqual(r['custom_mixtures'],8);self.assertEqual(r['statcast_n'],1)
            self.assertEqual(r['assessments']['not_configured']+r['assessments'].get('empty_cohort',0),16)
            summary=csv_rows(root/'out'/'plan_speed_mixture_summary.csv')
            components=csv_rows(root/'out'/'plan_speed_mixture_components.csv')
            for x in summary:
                if float(x['cohort_probability_per_evaluable_trial'])==0:continue
                c=[v for v in components if all(v[k]==x[k] for k in ['mixture','cohort'])]
                self.assertAlmostEqual(sum(float(v['selected_cohort_weight']) for v in c),1.)
            p=folder/'plan_tilt_summary.csv';rows=csv_rows(p)
            bad=next(r for r in rows if r['scope']=='all_swings' and r['stratum']=='overall')
            bad['trials']=str(int(bad['trials'])+1);write(p,rows)
            with self.assertRaises((ValueError,KeyError,TypeError)):compare.run(folder,statcast,root/'bad')
            self.assertFalse((root/'bad').exists())

    def test_selection_weighting_and_joint_tolerances(self):
        # Different selection probabilities must give selected weights 80/20, not 50/50.
        states=legacy.ExecutionMixtureTest().states()
        with tempfile.TemporaryDirectory() as t:
            root=Path(t);statcast=root/'savant.csv';write(statcast,[row(1)])
            compare.engine.compare(states,{},[],statcast,root/'out',
                default_mixtures={'half':{'normal':.5,'failure':.5}},prefix=compare.PREFIX)
            criteria={'version':1,'max_abs_delta_pp':{r:100. for r in compare.REGIONS},'max_grid_total_variation':1.}
            counts=compare.assess(root/'out',criteria)
            self.assertEqual(counts['within_configured_tolerances'],3)
            selected=next(r for r in csv_rows(root/'out'/'plan_speed_mixture_summary.csv') if r['mixture']=='mixture:half' and r['cohort']=='fair_direction')
            self.assertAlmostEqual(float(selected['cohort_probability_per_evaluable_trial']),.5)
            # A single failing metric must reject joint acceptance.
            region='ev_ge45';delta=abs(100*(float(selected[region+'_fraction'])-float(selected['statcast_'+region+'_fraction'])))
            self.assertGreater(delta,0.)
            criteria['max_abs_delta_pp'][region]=delta/2
            for f in ['plan_speed_mixture_assessment.csv','plan_speed_mixture_envelope.csv']:(root/'out'/f).unlink()
            compare.assess(root/'out',criteria)
            assessment=next(r for r in csv_rows(root/'out'/'plan_speed_mixture_assessment.csv') if r['mixture']=='mixture:half')
            self.assertEqual(assessment['assessment'],'outside_configured_tolerances')
            self.assertIn(region,assessment['failed_metrics'])

    def test_invalid_tolerances_and_custom_weights(self):
        with tempfile.TemporaryDirectory() as t:
            root=Path(t);p=root/'criteria.json'
            for c in [{'version':1},{'version':1,'max_abs_delta_pp':{'unknown':1}},
                      {'version':1,'max_grid_total_variation':True},
                      {'version':1,'max_abs_delta_pp':{'ev_lt25':-1}}]:
                p.write_text(json.dumps(c))
                with self.assertRaises(ValueError):compare.tolerances(p)
            folder,statcast=self.fixture(root);states,_,_,_=compare.load_states(folder,compare.engine.DEFAULT_MODEL)
            name=next(iter(states));p.write_text(json.dumps({'version':1,'mixtures':{'explicit':{name:1.}}}))
            r=compare.run(folder,statcast,root/'custom',mixtures_json=p)
            self.assertEqual(r['custom_mixtures'],1)
            p.write_text(json.dumps({'version':1,'mixtures':{'bad':{name:.5}}}))
            with self.assertRaises(ValueError):compare.run(folder,statcast,root/'bad',mixtures_json=p)
            self.assertFalse((root/'bad').exists())


if __name__=='__main__':unittest.main()
