import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_forecast_contact_geometry import fixture,csv_rows,replace_csv
import finite_barrel_probe as probe


class PerformanceModesTest(unittest.TestCase):
    def test_tilt_only_outputs_match_full_without_ground_or_zero_replay(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);src=fixture(root)
            probe.run(src,root/'full',ball_spin=(100.,0.,0.))
            with patch.object(probe,'first_ground',side_effect=AssertionError('unneeded ground')):
                with patch.object(probe,'add_spin_pairs',side_effect=AssertionError('unneeded zero replay')):
                    probe.run(src,root/'tilt',ball_spin=(100.,0.,0.),diagnostics=['tilt'])
            for name in ['coverage','checks','geometry','outcomes']:
                filename='finite_barrel_tilt_'+name+'.csv'
                self.assertEqual((root/'full'/filename).read_bytes(),(root/'tilt'/filename).read_bytes())
            self.assertFalse((root/'tilt'/'finite_barrel_summary.csv').exists())
            self.assertFalse((root/'tilt'/'finite_barrel_observation_transitions.csv').exists())
            meta=json.loads((root/'tilt'/'finite_barrel_metadata.json').read_text())
            self.assertFalse(meta['execution']['ground_evaluated'])
            self.assertFalse(meta['execution']['zero_spin_counterpart_evaluated'])

    def test_focused_rows_match_full_and_source_audit_is_not_skipped(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);src=fixture(root)
            probe.run(src,root/'full',ball_spin=(100.,0.,0.))
            model='finite_I0.00015_et0.4';variant='observed_sigma_0.005_r0.9'
            probe.run(src,root/'focused',ball_spin=(100.,0.,0.),diagnostics=['tilt'],model_names=[model],variants=[variant])
            full=[r for r in csv_rows(root/'full'/'finite_barrel_tilt_outcomes.csv') if r['variant']==variant and r['outcome_basis'] in [model,'normal_profile','finite_mu0','source_constant_mass_normal']]
            focused=csv_rows(root/'focused'/'finite_barrel_tilt_outcomes.csv')
            self.assertEqual(full,focused)
            rows=csv_rows(src/'observation_update_contacts.csv')
            # Corrupt a condition the solver filter would otherwise skip.
            row=next(r for r in rows if r['variant']!=variant);row['contact_contract_version']='99'
            replace_csv(src/'observation_update_contacts.csv',rows)
            with self.assertRaisesRegex(ValueError,'unsupported_contact_contract'):
                probe.run(src,root/'bad',diagnostics=['tilt'],variants=[variant])

    def test_invalid_filters_fail_before_output_creation_and_core_subset_works(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);src=fixture(root)
            for kw in [dict(diagnostics=[]),dict(diagnostics=['bogus']),dict(model_names=['bogus']),
                       dict(variants=['bogus']),dict(diagnostics=['direction'],variants=['observed_sigma_0.005_r0.9'])]:
                with self.assertRaises(ValueError):probe.run(src,root/'bad',**kw)
                self.assertFalse((root/'bad').exists())
            probe.run(src,root/'core',diagnostics=['core'],model_names=['finite_I0.00015_et0.4'])
            self.assertTrue((root/'core'/'finite_barrel_summary.csv').exists())

    def test_baseline_prepared_once_for_spin_comparison(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);src=fixture(root)
            original=probe.baseline.event
            with patch.object(probe.baseline,'event',wraps=original) as baseline:
                coverage=probe.run(src,root/'out',ball_spin=(100.,0.,0.))
                self.assertEqual(baseline.call_count,coverage['trial_rows'])
