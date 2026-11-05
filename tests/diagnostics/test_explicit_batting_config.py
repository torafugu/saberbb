import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools/diagnostics'))
from mixed_intent_profile import parse_config,settings_for
from test_mixed_intent_profile import mixed_fixture,schedule
import seeded_holdout_audit as audit
import intent_selection_policy as policy


def configuration():
    return dict(version=1,default_strategy='let_batter_decide',default_preferred_intent='contact_first',
        team_strategies=[dict(team_id=1,strategy='put_ball_in_play'),dict(team_id=5,strategy='seek_line_drive')],
        player_preferences=[dict(player_id=1,preferred_intent='line_drive')])


def configured_fixture(root):
    db,folder=mixed_fixture(root);path=folder/'manifest.json';manifest=json.loads(path.read_text())
    config=configuration();raw=json.dumps(config,indent=2).encode()+b'\n';(folder/'batting_config.json').write_bytes(raw)
    manifest.update(batting_profile='configured-v1',batting_config=dict(file='batting_config.json',sha256=hashlib.sha256(raw).hexdigest()))
    for game in manifest['games']:
        case_path=folder/game['input_file'];case=json.loads(case_path.read_text())
        case['batting_settings']=settings_for('configured-v1',case['schedule'],config)
        case_bytes=json.dumps(case).encode();case_path.write_bytes(case_bytes)
        game['input_sha256']=hashlib.sha256(case_bytes).hexdigest()
    path.write_text(json.dumps(manifest));return db,folder


class ExplicitBattingConfigTest(unittest.TestCase):
    def test_defaults_overrides_and_roster_order_are_independent_of_outcomes(self):
        config=configuration();config['default_preferred_intent']='line_drive';config['player_preferences']=[dict(player_id=101,preferred_intent='contact_first')]
        s=schedule();settings=settings_for('configured-v1',s,config)
        self.assertEqual(settings['away_strategy'],'put_ball_in_play')
        self.assertEqual(settings['player_preferences']['1']['preferred_intent'],'line_drive')
        self.assertEqual(settings['player_preferences']['101']['preferred_intent'],'contact_first')
        s['home_team']['id']=3;self.assertEqual(settings_for('configured-v1',s,config)['home_strategy'],'let_batter_decide')
        s['home_team']['id']=5;s['away_team']['players'].reverse()
        self.assertEqual(settings,settings_for('configured-v1',s,config))

    def test_strict_schema_rejects_duplicates_unknown_outcome_fields_and_bad_ids(self):
        mutations=[lambda c:c.update(version=True),lambda c:c.update(version=2),lambda c:c.update(fair=True),
            lambda c:c.update(default_strategy='maximize_observed_ev'),lambda c:c.update(default_preferred_intent='unknown'),
            lambda c:c['team_strategies'].append(c['team_strategies'][0]),
            lambda c:c['player_preferences'].append(c['player_preferences'][0]),
            lambda c:c['player_preferences'][0].update(player_id=-1),
            lambda c:c['player_preferences'][0].update(player_id=True),
            lambda c:c['player_preferences'][0].update(player_id=2**63),
            lambda c:c['team_strategies'][0].update(team_id=65536),
            lambda c:c['team_strategies'][0].update(strategy='seek_line_drive',actual_normal=[0,0,1])]
        for mutate in mutations:
            config=configuration();mutate(config)
            with self.assertRaises(ValueError):parse_config(json.dumps(config))
        raw=json.dumps(configuration()).replace('"version": 1','"version": 1, "version": 1')
        with self.assertRaises(ValueError):parse_config(raw)
        with self.assertRaises(ValueError):settings_for('configured-v1',schedule())

    def test_frozen_configuration_audits_and_existing_policy_evaluator_accepts_cache(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);db,folder=configured_fixture(root);original=db.read_bytes()
            result=audit.run(db,folder,root/'audit');self.assertEqual(result['provenance_games_verified'],2)
            self.assertEqual(db.read_bytes(),original)
            meta=json.loads((root/'audit/rust_collision_response_metadata.json').read_text())
            self.assertEqual(meta['batter_decision_coverage']['verified_decisions'],2)
            self.assertEqual(meta['holdout_manifest']['batting_profile'],'configured-v1')
            self.assertIn(str(folder/'batting_config.json'),meta['holdout_inputs_sha256'])
            self.assertEqual(policy.run(root/'audit',root/'policy',20,'explicit')['scenarios'],7)

    def test_changed_bytes_missing_configuration_and_path_escape_fail_before_output(self):
        for variant in ('whitespace','missing','escape','incompatible_profile','changed_case'):
            with self.subTest(variant=variant),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);db,folder=configured_fixture(root);path=folder/'manifest.json';manifest=json.loads(path.read_text())
                if variant=='whitespace':
                    f=folder/'batting_config.json';f.write_bytes(f.read_bytes()+b' ')
                elif variant=='missing':manifest.pop('batting_config')
                elif variant=='escape':manifest['batting_config']['file']='../outside.json'
                elif variant=='incompatible_profile':manifest['batting_profile']='default'
                else:
                    g=manifest['games'][0];cp=folder/g['input_file'];case=json.loads(cp.read_text());case['batting_settings']['away_strategy']='seek_line_drive'
                    raw=json.dumps(case).encode();cp.write_bytes(raw);g['input_sha256']=hashlib.sha256(raw).hexdigest()
                path.write_text(json.dumps(manifest))
                with self.assertRaises(ValueError):audit.run(db,folder,root/'out')
                self.assertFalse((root/'out').exists())


if __name__=='__main__':unittest.main()
