import copy
import json
import tempfile
import unittest
from pathlib import Path

from test_bat_pivot_orientation_probe import source_db
from test_statcast_distribution_compare import write, row
from test_forecast_contact_geometry import csv_rows
import swing_execution_probe as probe
import swing_execution_mixture_compare as compare


class ExecutionMixtureTest(unittest.TestCase):
    def states(self):
        result={}
        for name,n,f,cell in [('normal',100,80,(8,4)),('failure',100,20,(3,4))]:
            result[name]=dict(config=dict(planned_effort=1.,anchor_fraction=.5,start_offset_sec=0.),
                counts=dict(trials=n+5,evaluable_trials=n,execution_clock_invalid_trials=5,
                            evaluable_geometry_contacts=f,successful_contacts=f,fair_contacts=f),
                grids={c:{cell:f} for c in compare.COHORTS})
        return result

    def test_selection_precedes_conditional_normalization(self):
        states=self.states()
        mass,p=compare.mix(states,{'normal':.5,'failure':.5},'fair_direction')
        self.assertAlmostEqual(p,.5)
        self.assertAlmostEqual(mass[8,4]/p,.8)
        self.assertAlmostEqual(mass[3,4]/p,.2)
        self.assertEqual(states['normal']['counts']['trials'],105)
        self.assertAlmostEqual(compare.mix(states,{'normal':1},'fair_direction')[1],.8)

    def test_bad_weights_and_incompatible_populations_rejected(self):
        states=self.states()
        for weights in ({},{'missing':1},{'normal':.8},{'normal':float('nan')},
                        {'normal':-1,'failure':2},{'normal':True}):
            with self.subTest(weights=weights),self.assertRaises(ValueError):
                compare.mix(states,weights,'fair_direction')
        for field in ('planned_effort','anchor_fraction','start_offset_sec'):
            changed=copy.deepcopy(states);changed['failure']['config'][field]+=.1
            with self.assertRaisesRegex(ValueError,'same_plan'):
                compare.mix(changed,{'normal':.5,'failure':.5},'fair_direction')
        changed=copy.deepcopy(states);changed['failure']['counts']['evaluable_trials']-=1
        with self.assertRaises(ValueError):compare.mix(changed,{'normal':.5,'failure':.5},'fair_direction')

    def test_rebin_edges_and_tails(self):
        from bisect import bisect_right
        grid={}
        for ev,vla in [(0,-90),(25,0),(45,50),(90,90)]:
            grid[bisect_right(compare.OBS_EV,ev),bisect_right(compare.OBS_VLA,vla)]=1
        self.assertEqual(compare.rebin_observed(grid),{(1,1):1,(4,4):1,(8,9):1,(11,11):1})

    def fixture(self,root):
        src,db=source_db(root);out=root/'execution'
        probe.run(src,db,out,efforts=(1.,),execution_ratios=(1.,.85))
        statcast=root/'savant.csv'
        write(statcast,[row(1),row(2,speed=''),row(3,kind='S')])
        return out,statcast

    def test_full_pipeline_and_deviation_fallback_agree(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,statcast=self.fixture(root)
            states,_,_=compare.load_states(source,'all_swings',compare.DEFAULT_VARIANT,compare.DEFAULT_MODEL)
            names=list(states);config=root/'mix.json'
            config.write_text(json.dumps(dict(version=1,mixtures={'half':dict.fromkeys(names,.5)})))
            result=compare.run(source,statcast,root/'out',config)
            self.assertEqual(result['custom_mixtures'],1);self.assertEqual(result['statcast_n'],1)
            normal=csv_rows(root/'out'/'execution_mixture_summary.csv')
            compare.run(source,statcast,root/'fallback',config,
                        histogram=source/'swing_execution_deviation_ev_vla.csv')
            self.assertEqual(normal,csv_rows(root/'fallback'/'execution_mixture_summary.csv'))
            for name in ('state:'+names[0],'state:'+names[1],'mixture:half'):
                cells=[r for r in csv_rows(root/'out'/'execution_mixture_ev_vla.csv')
                       if r['mixture']==name and r['cohort']=='fair_direction']
                selected=next(r for r in normal if r['mixture']==name and r['cohort']=='fair_direction')
                self.assertAlmostEqual(sum(float(r['probability_per_evaluable_trial']) for r in cells),
                                       float(selected['cohort_probability_per_evaluable_trial']))
                if float(selected['cohort_probability_per_evaluable_trial']):
                    self.assertAlmostEqual(sum(float(r['simulation_fraction']) for r in cells),1.)
            audit={r['metric']:int(r['count']) for r in csv_rows(root/'out'/'statcast_coverage.csv')}
            self.assertEqual(audit['excluded_missing_either'],1);self.assertEqual(audit['not_in_play'],1)
            with self.assertRaises(FileExistsError):compare.run(source,statcast,root/'out',config)

    def test_corrupt_histogram_rejected_before_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,statcast=self.fixture(root)
            path=source/'swing_execution_ev_vla.csv';rows=csv_rows(path)
            execution=next(r for r in rows if r['orientation']=='execution')
            execution['contacts']=str(int(execution['contacts'])+1);write(path,rows)
            with self.assertRaisesRegex(ValueError,'histogram'):compare.run(source,statcast,root/'bad')
            self.assertFalse((root/'bad').exists())

    def test_empty_cohort_and_duplicate_json(self):
        states=self.states();states['failure']['grids']['fair_direction']={}
        self.assertEqual(compare.mix(states,{'failure':1.},'fair_direction'),({},0))
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'duplicate.json';path.write_text('{"a":1,"a":2}')
            with self.assertRaisesRegex(ValueError,'duplicate_json_key'):compare.read_json(path)


if __name__=='__main__':unittest.main()
