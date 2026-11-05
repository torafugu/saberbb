import copy
import csv
import hashlib
import json
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools/diagnostics'))
from mixed_intent_profile import settings_for, BatterAudit
from rust_observed_intent_audit import decide
from test_seeded_holdout_audit import fixture as base_fixture
from test_rust_collision_response_audit import recorded
import rust_collision_response_audit as response
import seeded_holdout_audit as audit
import intent_selection_policy as policy


def schedule(team=1):
    return dict(id=1,season=2027,round_seq=1,
        away_team=dict(id=team,players=[dict(info=dict(id=i)) for i in (1,2)]),
        home_team=dict(id=5,players=[dict(info=dict(id=i)) for i in (101,102)]))


def mixed_fixture(root):
    db,folder=base_fixture(root)
    con=sqlite3.connect(db)
    for seq,pid,top,strategy,preference in [(1,1,True,'put_ball_in_play','line_drive'),
                                         (2,102,False,'seek_line_drive','contact_first')]:
        state=recorded(seq,strategy,preference);state['intent_batter']=dict(batter_id=pid,is_top=top)
        con.execute('DELETE FROM player_game_contact_state WHERE game_id=1 AND count_seq=?',(seq,))
        con.execute('INSERT INTO player_game_contact_state VALUES(1,?,1,?,NULL,?)',(seq,response.SOURCE,json.dumps(state)))
        if seq==2:con.execute('INSERT INTO player_game_pitching VALUES(1,2,2200.,0.,.9)')
    con.commit()
    path=folder/'manifest.json';manifest=json.loads(path.read_text());manifest['batting_profile']='mixed-id-v1'
    for game in manifest['games']:
        case_path=folder/game['input_file'];case=json.loads(case_path.read_text())
        case['schedule']=dict(schedule(),id=game['game_id'])
        case['batting_settings']=settings_for('mixed-id-v1',case['schedule'])
        raw=json.dumps(case).encode();case_path.write_bytes(raw);game['input_sha256']=hashlib.sha256(raw).hexdigest()
        h=hashlib.sha256();count=0
        for row in con.execute('SELECT count_seq,state_version,state_source,invalid_reason,state_json FROM player_game_contact_state WHERE game_id=? ORDER BY count_seq',(game['game_id'],)):
            response.update_contact_hash(h,*row);count+=1
        game.update(contacts_sha256=h.hexdigest(),contact_states=count)
    con.close();path.write_text(json.dumps(manifest));return db,folder


class MixedIntentProfileTest(unittest.TestCase):
    def test_id_only_assignments_cover_all_coach_player_pairs_and_roster_order_is_irrelevant(self):
        pairs=set()
        for team in (1,2,3):
            s=schedule(team);before=copy.deepcopy(s);settings=settings_for('mixed-id-v1',s)
            self.assertEqual(s,before)
            for pid in (1,2):
                pairs.add((settings['away_strategy'],settings['player_preferences'][str(pid)]['preferred_intent']))
            s['away_team']['players'].reverse()
            self.assertEqual(settings,settings_for('mixed-id-v1',s))
        self.assertEqual(len(pairs),6)
        with self.assertRaises(ValueError):settings_for('unknown',s)
        s['home_team']['players'][0]['info']['id']=1
        with self.assertRaises(ValueError):settings_for('mixed-id-v1',s)

    def test_identity_and_saved_decision_must_match_frozen_settings(self):
        s=schedule();case=dict(schedule=s,batting_settings=settings_for('mixed-id-v1',s))
        state=dict(intent_batter=dict(batter_id=1,is_top=True),
            observed_intent_plans=dict(game_id=1,count_seq=1,decision=decide('put_ball_in_play','line_drive')))
        report=BatterAudit();key=report.observe(1,1,state,case,'mixed-id-v1')
        self.assertEqual(key[4],'contact_first');self.assertEqual(report.summaries()[0]['manager_override'],1)
        for mutate in [lambda x:x['intent_batter'].update(batter_id=102),
                       lambda x:x['intent_batter'].update(is_top=1),
                       lambda x:x['observed_intent_plans'].update(count_seq=2),
                       lambda x:x['observed_intent_plans'].update(decision=decide('let_batter_decide','line_drive')),
                       lambda x:x.pop('intent_batter')]:
            bad=copy.deepcopy(state);mutate(bad)
            with self.assertRaises(ValueError):BatterAudit().observe(1,1,bad,case,'mixed-id-v1')

    def test_old_identity_and_unavailable_decision_are_visible_without_fabrication(self):
        report=BatterAudit();self.assertIsNone(report.observe(1,1,{}, {}, 'default'))
        self.assertEqual(report.coverage['missing_batter_context'],1)
        case=dict(schedule=schedule(),batting_settings=settings_for('mixed-id-v1',schedule()))
        report.observe(1,1,dict(intent_batter=dict(batter_id=1,is_top=True)),case,'mixed-id-v1')
        self.assertEqual(report.trace[0]['selected_intent'],None)
        self.assertEqual(report.coverage['verified_decisions'],0)
        self.assertEqual(report.coverage['missing_observed_decision'],1)
        self.assertIsNone(report.summaries()[0]['fair_pct_per_comparable_pitch'])

    def test_native_seeded_audit_verifies_batters_both_sides_and_reports_override_counts(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);db,folder=mixed_fixture(root);before=db.read_bytes()
            result=audit.run(db,folder,root/'out');self.assertEqual(result['provenance_games_verified'],2)
            self.assertEqual(db.read_bytes(),before)
            metadata=json.loads((root/'out/rust_collision_response_metadata.json').read_text())
            self.assertEqual(metadata['batter_decision_coverage']['verified_decisions'],2)
            with (root/'out/rust_collision_response_batters.csv').open() as f:rows=list(csv.DictReader(f))
            self.assertEqual({r['batter_id'] for r in rows},{'1','102'})
            self.assertTrue(all(r['manager_override']=='1' for r in rows))
            self.assertTrue(all(r['comparable_pitches']=='1' for r in rows))
            self.assertEqual(metadata['holdout_manifest']['batting_profile'],'mixed-id-v1')
            result=policy.run(root/'out',root/'policy',20,'mixed')
            self.assertEqual(result['observed_games'],2)
            self.assertEqual(result['scenarios'],7)

    def test_rehashed_case_with_changed_assignment_is_rejected_before_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);db,folder=mixed_fixture(root);path=folder/'manifest.json';m=json.loads(path.read_text())
            game=m['games'][0];cp=folder/game['input_file'];case=json.loads(cp.read_text())
            case['batting_settings']['away_strategy']='seek_line_drive';raw=json.dumps(case).encode();cp.write_bytes(raw)
            game['input_sha256']=hashlib.sha256(raw).hexdigest();path.write_text(json.dumps(m))
            with self.assertRaisesRegex(ValueError,'holdout_strategy_changed'):audit.run(db,folder,root/'out')
            self.assertFalse((root/'out').exists())


if __name__=='__main__':unittest.main()
