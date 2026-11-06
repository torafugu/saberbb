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
from player_manager_batting import evaluate
from mixed_intent_profile import settings_for,BatterAudit
from test_mixed_intent_profile import mixed_fixture
from test_rust_collision_response_audit import recorded
import seeded_holdout_audit as audit
import rust_collision_response_audit as response
import intent_selection_policy as policy


def derived_fixture(root):
    db,folder=mixed_fixture(root);path=folder/'manifest.json';manifest=json.loads(path.read_text());manifest['batting_profile']='player-context-v1'
    con=sqlite3.connect(db)
    for game in manifest['games']:
        cp=folder/game['input_file'];case=json.loads(cp.read_text());s=case['schedule']
        s['away_team']['manager']=dict(id=91,name='Away Manager',batting_policy='situational')
        s['home_team']['manager']=dict(id=92,name='Home Manager',batting_policy='put_ball_in_play')
        for side,character in [('away','adaptive'),('home','line_drive_oriented')]:
            for player in s[side+'_team']['players']:player['batting_character']=character
        case['batting_settings']=settings_for('player-context-v1',s);raw=json.dumps(case).encode();cp.write_bytes(raw);game['input_sha256']=hashlib.sha256(raw).hexdigest()
        if game['game_id']==1:
            for seq,pid,side in [(1,1,'away'),(2,102,'home')]:
                trace=evaluate(pid,'adaptive' if side=='away' else 'line_drive_oriented',dict(balls=2,strikes=0,outs=0,runners=[False]*3),s[side+'_team']['manager'])
                d=trace['decision'];state=recorded(seq,d['strategy'],d['preferences']['preferred_intent'])
                state.update(intent_batter=dict(batter_id=pid,is_top=side=='away'),batting_selection=trace)
                con.execute('UPDATE player_game_contact_state SET state_json=? WHERE game_id=1 AND count_seq=?',(json.dumps(state),seq))
        h=hashlib.sha256();count=0
        for row in con.execute('SELECT count_seq,state_version,state_source,invalid_reason,state_json FROM player_game_contact_state WHERE game_id=? ORDER BY count_seq',(game['game_id'],)):
            response.update_contact_hash(h,*row);count+=1
        game.update(contacts_sha256=h.hexdigest(),contact_states=count)
    con.commit();con.close();path.write_text(json.dumps(manifest));return db,folder


class PlayerManagerBattingTest(unittest.TestCase):
    def test_cross_language_frozen_contract(self):
        fixture=json.loads((ROOT/'tests/fixtures/player_manager_batting_contract.json').read_text())
        for case in fixture['cases']:self.assertEqual(evaluate(**case['input']),case['expected'])

    def test_context_and_manager_are_independent_and_no_outcome_fields_are_accepted(self):
        situation=dict(balls=2,strikes=0,outs=0,runners=[False]*3)
        free=evaluate(7,'adaptive',situation,None)
        self.assertEqual(free['decision']['selected_intent'],'line_drive')
        self.assertEqual(free['instruction']['reason'],'no_manager')
        manager=dict(id=42,name='Manager',batting_policy='put_ball_in_play')
        forced=evaluate(7,'adaptive',situation,manager)
        self.assertEqual(free['preferences'],forced['preferences'])
        self.assertEqual(forced['decision']['selected_intent'],'contact_first')
        for bad in [dict(situation,strikes=3),dict(situation,balls=True),dict(situation,actual_vla=20)]:
            with self.assertRaises(ValueError):evaluate(7,'adaptive',bad,manager)

    def test_missing_or_modified_people_trace_cannot_override_frozen_roster(self):
        s=dict(away_team=dict(id=1,manager=None,players=[dict(info=dict(id=7),batting_character='adaptive')]),home_team=dict(id=2,players=[]))
        trace=evaluate(7,'adaptive',dict(balls=2,strikes=0,outs=0,runners=[False]*3),None)
        case=dict(schedule=s,batting_settings=settings_for('player-context-v1',s))
        state=dict(intent_batter=dict(batter_id=7,is_top=True),batting_selection=trace,
            observed_intent_plans=dict(game_id=1,count_seq=1,decision=trace['decision']))
        report=BatterAudit();report.observe(1,1,state,case,'player-context-v1');self.assertEqual(report.trace[0]['batting_character'],'adaptive')
        for change in [lambda x:x.pop('batting_selection'),lambda x:x['batting_selection'].update(character='balanced'),
            lambda x:x['batting_selection']['preferences'].update(contact_weight=.99),
            lambda x:x['batting_selection'].update(manager=dict(id=44,name='Injected',batting_policy='seek_line_drive'))]:
            bad=copy.deepcopy(state);change(bad)
            with self.assertRaises(ValueError):BatterAudit().observe(1,1,bad,case,'player-context-v1')

    def test_native_seeded_audit_rechecks_complete_pipeline_and_exports_person_identity(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);db,folder=derived_fixture(root);before=db.read_bytes()
            audit.run(db,folder,root/'audit');self.assertEqual(before,db.read_bytes())
            metadata=json.loads((root/'audit/rust_collision_response_metadata.json').read_text())
            self.assertEqual(metadata['batter_decision_coverage']['verified_decisions'],2)
            checks=metadata['holdout_manifest'];self.assertEqual(checks['batting_profile'],'player-context-v1')
            with (root/'audit/rust_collision_response_batter_decisions.csv').open() as f:rows=list(csv.DictReader(f))
            self.assertEqual({r['manager_id'] for r in rows},{'91','92'})
            self.assertEqual({r['batting_character'] for r in rows},{'adaptive','line_drive_oriented'})
            self.assertTrue(all(r['contact_weight']!='' for r in rows))
            result=policy.run(root/'audit',root/'policy',20,'people');self.assertEqual(result['observed_games'],2)
            with (root/'audit/rust_collision_response_checks.csv').open() as f:metrics=[r['metric'] for r in csv.DictReader(f)]
            self.assertIn('player_manager_selection.preferences.contact_weight',metrics)


if __name__=='__main__':unittest.main()
