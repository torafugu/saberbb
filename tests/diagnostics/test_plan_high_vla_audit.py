import json
import tempfile
import unittest
from collections import Counter
from pathlib import Path
from unittest.mock import patch

from test_bat_pivot_orientation_probe import source_db
from test_forecast_contact_geometry import csv_rows
from test_statcast_distribution_compare import write,row
import plan_tilt_sweep as sweep
import plan_speed_mixture_compare as compare
import plan_high_vla_audit as audit

TOOLS=Path(__file__).resolve().parents[2]/'tools'/'diagnostics'


def fact(valid=True,contact=False,fair=False,high=False,ev=40.,vla=10.):
    return dict(evaluable=valid,clock_invalid=not valid,geometry=contact,fair=fair,high_vla=high,
        low_ev=fair and ev<25,high_ev=fair and ev>=45,downward=fair and vla<0,
        vla_0to30=fair and 0<=vla<30,ev_40to45=fair and 40<=ev<45,
        ev_ms=ev if fair else None,vla_deg=vla if fair else None,height='0.6..<0.9',
        outcome='clock_invalid' if not valid else 'fair_high_vla' if high else 'fair_other' if fair else 'no_geometry_contact')


class HighVlaAuditTest(unittest.TestCase):
    def test_paired_selection_changes_are_not_common_fair_changes(self):
        states={n:dict(config=dict(tilt_deg=15,yaw_batter_deg=0,planned_speed_ratio=1,aim_z_offset_m=a))
                for n,a in [('center',0),('lower',-.03)]}
        pitches={(1,1):dict(center=fact(contact=True,fair=True,high=True,vla=60),lower=fact()),
                 (1,2):dict(center=fact(),lower=fact(contact=True,fair=True)),
                 (1,3):dict(center=fact(valid=False),lower=fact(valid=False))}
        pairs,transitions=audit.pair_summary(pitches,states,dict(center=.2,lower=.8))
        r=next(r for r in pairs if r['stratum']=='overall')
        self.assertEqual(r['before_scenario'],'center');self.assertEqual(r['after_scenario'],'lower')
        self.assertEqual(r['evaluable'],2);self.assertEqual(r['clock_invalid'],1)
        self.assertEqual(r['geometry_gained'],1);self.assertEqual(r['geometry_lost'],1)
        self.assertEqual(r['fair_probability_delta_pp'],0)
        self.assertEqual(r['high_vla_probability_delta_pp'],-50)
        self.assertEqual(r['before_high_vla_fraction_among_fair'],1)
        self.assertEqual(r['after_high_vla_fraction_among_fair'],0)
        self.assertEqual(r['common_fair'],0);self.assertIsNone(r['common_fair_high_delta_pp'])
        self.assertEqual(sum(t['trials'] for t in transitions if t['stratum']=='overall'),3)
        pitches[(1,4)]=dict(center=fact(contact=True,fair=True,ev=42,vla=20),
                           lower=fact(contact=True,fair=True,high=True,ev=35,vla=55))
        r=next(r for r in audit.pair_summary(pitches,states,dict(center=.2,lower=.8))[0] if r['stratum']=='overall')
        self.assertEqual(r['common_fair'],1);self.assertEqual(r['common_fair_high_gained'],1)
        self.assertEqual(r['common_fair_mean_delta_ev_ms'],-7)
        self.assertEqual(r['common_fair_mean_delta_vla_deg'],35)
        self.assertEqual(r['common_fair_high_delta_pp'],100)

    def test_cache_reconstruction_partitions_and_readonly_pipeline(self):
        with tempfile.TemporaryDirectory() as t:
            root=Path(t);src,db=source_db(root)
            sweep.run(src,db,root/'cache',sample_size=2,angles=(15.,),yaws=(0.,),
                aim_z_offsets=(-.03,-.015,0.),speed_ratios=(.6,.8,.9,1.))
            config=TOOLS/'plan_midpoint_frozen_mixture.json'
            observed=root/'savant.csv';write(observed,[row(1)])
            compare.run(root/'cache',observed,root/'compare',mixtures_json=config)
            expected=next(r for r in csv_rows(root/'compare'/'plan_speed_mixture_summary.csv')
                if r['mixture']=='mixture:both_midpoints_same_means' and r['cohort']=='fair_direction')
            db.unlink()
            with patch('sqlite3.connect',side_effect=AssertionError('DB read')), \
                 patch.object(sweep,'evaluate',side_effect=AssertionError('search')), \
                 patch.object(sweep,'event',side_effect=AssertionError('collision solve')):
                result=audit.run(root/'cache',root/'out',config)
            self.assertEqual(result['components'],12);self.assertEqual(result['new_searches'],0)
            summary={r['metric']:r for r in csv_rows(root/'out'/(audit.PREFIX+'_summary.csv'))}
            mapping=dict(high_vla='vla_ge50',low_ev='ev_lt25',high_ev='ev_ge45',downward='vla_lt0',vla_0to30='vla_0to30',ev_40to45='ev_40to45')
            for field,column in mapping.items():
                self.assertAlmostEqual(float(summary[field]['fraction_among_mixture_fair']),float(expected[column+'_fraction']))
            self.assertAlmostEqual(float(summary['fair']['probability_per_evaluable_trial']),float(expected['cohort_probability_per_evaluable_trial']))
            components=csv_rows(root/'out'/(audit.PREFIX+'_components.csv'))
            strata=csv_rows(root/'out'/(audit.PREFIX+'_strata.csv'))
            for stage in {'overall'}|{r['stratum'] for r in strata}:
                selected=components if stage=='overall' else [r for r in strata if r['stratum']==stage]
                self.assertAlmostEqual(sum(float(r['fair_contribution_fraction']) for r in selected),1)
                self.assertAlmostEqual(sum(float(r['high_vla_contribution_pp']) for r in selected),100*float(summary['high_vla']['fraction_among_mixture_fair']))
            pairs=csv_rows(root/'out'/(audit.PREFIX+'_pairs.csv'))
            self.assertEqual(len([r for r in pairs if r['stratum']=='overall']),12)
            transitions=csv_rows(root/'out'/(audit.PREFIX+'_transitions.csv'))
            for r in pairs:
                subset=[t for t in transitions if all(t[k]==r[k] for k in ('before_scenario','after_scenario','stratum','forecast_height_band_m'))]
                self.assertEqual(sum(int(t['trials']) for t in subset),int(r['trials']))
            checks=csv_rows(root/'out'/(audit.PREFIX+'_checks.csv'))
            self.assertLess(max(float(r['max_error']) for r in checks),1e-7)
            metadata=json.loads((root/'out'/(audit.PREFIX+'_metadata.json')).read_text())
            self.assertEqual(metadata['weights'],json.loads(config.read_text())['mixtures']['both_midpoints_same_means'])
            with self.assertRaises(FileExistsError):audit.run(root/'cache',root/'out',config)
            with self.assertRaisesRegex(ValueError,'one_fixed'):audit.run(root/'cache',root/'bad',TOOLS/'plan_midpoint_mixtures.json')
            self.assertFalse((root/'bad').exists())


if __name__=='__main__':unittest.main()
