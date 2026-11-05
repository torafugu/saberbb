import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools/diagnostics'))
from test_rust_collision_response_audit import recorded
from test_rust_intent_collision_probe import database_with_spin
from test_forecast_contact_geometry import csv_rows
import rust_collision_response_audit as response
import seeded_holdout_audit as audit
import rust_intent_game_bootstrap as bootstrap


def fixture(root):
    db=root/'db';database_with_spin(db,[(1,recorded(),None)])
    con=sqlite3.connect(db)
    con.execute('INSERT INTO player_game_contact_state SELECT 2,count_seq,state_version,state_source,invalid_reason,state_json FROM player_game_contact_state')
    con.execute('INSERT INTO player_game_contact_state VALUES(3,1,1,?, ?, NULL)',(response.SOURCE,'synthetic_invalid'))
    con.commit()
    folder=root/'run';(folder/'cases').mkdir(parents=True)
    manifest=dict(version=1,source='seeded_intent_holdout_v1',status='complete',master_seed='2026110501',
        requested_rounds=1,completed_rounds=1,exclude_through_game_id=0,max_pitches=10000,
        model=response.collision.MODEL,engine_source_sha256='a'*64,build_sha256='b'*64,error=None,games=[])
    manifest['frozen_policy_sha256']=audit.digest(audit.FROZEN_POLICIES)
    for game in (1,3):
        seed=str(audit.derive_seed(2026110501,game));name=f'cases/game_{game}.json'
        case=dict(version=1,engine_source_sha256='a'*64,build_sha256='b'*64,game_seed=seed,max_pitches=10000,
            schedule=dict(id=game,season=2026,round_seq=1),
            batting_settings=dict(away_strategy='let_batter_decide',home_strategy='let_batter_decide',player_preferences={}))
        raw=json.dumps(case,sort_keys=True).encode();(folder/name).write_bytes(raw)
        h=hashlib.sha256();count=0
        for seq,version,source,invalid,payload in con.execute('SELECT count_seq,state_version,state_source,invalid_reason,state_json FROM player_game_contact_state WHERE game_id=? ORDER BY count_seq',(game,)):
            response.update_contact_hash(h,seq,version,source,invalid,payload);count+=1
        manifest['games'].append(dict(game_id=game,game_seed=seed,input_file=name,input_sha256=hashlib.sha256(raw).hexdigest(),
            status='saved',result_sha256='c'*64,contacts_sha256=h.hexdigest(),contact_states=count))
    (folder/'manifest.json').write_text(json.dumps(manifest));con.close()
    return db,folder


class SeededHoldoutAuditTest(unittest.TestCase):
    def test_declared_round_plan_must_match_exact_case_population(self):
        for plan,valid in [([dict(season=2026,round_seq=1,game_ids=[1,3])],True),
            ([dict(season=2026,round_seq=1,game_ids=[1,2])],False),
            ([dict(season=2027,round_seq=1,game_ids=[1,3])],False),
            ([dict(season=2026,round_seq=1,game_ids=[1,3,3])],False),([],False)]:
            with tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);db,folder=fixture(root);path=folder/'manifest.json'
                manifest=json.loads(path.read_text());manifest['round_plan']=plan;path.write_text(json.dumps(manifest))
                if valid:self.assertEqual(audit.run(db,folder,root/'out')['provenance_games_verified'],2)
                else:
                    with self.assertRaises(ValueError):audit.run(db,folder,root/'out')
                    self.assertFalse((root/'out').exists())

    def test_provenance_filters_exact_game_set_and_preserves_database(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);db,folder=fixture(root);before=hashlib.sha256(db.read_bytes()).hexdigest()
            result=audit.run(db,folder,root/'out')
            self.assertEqual(result['states'],2);self.assertEqual(result['matched_states'],1)
            self.assertEqual(result['provenance_games_verified'],2)
            self.assertEqual(result['master_seed'],'2026110501')
            self.assertEqual(before,hashlib.sha256(db.read_bytes()).hexdigest())
            meta=json.loads((root/'out/rust_collision_response_metadata.json').read_text())
            self.assertEqual(meta['selected_game_ids'],[1,3])
            self.assertEqual(meta['holdout_manifest']['status'],'complete')
            statistics=bootstrap.run(root/'out',root/'bootstrap',20,'zero-game')
            self.assertEqual(statistics['observed_games'],2)
            self.assertEqual(statistics['comparable_games'],1)
            self.assertGreater(statistics['intervals_unavailable'],0)
            with self.assertRaises(FileExistsError):audit.run(db,folder,root/'out')

    def test_failed_incomplete_overlapping_or_bad_seed_manifest_rejected(self):
        for mutate in [lambda m:m.update(status='failed'),lambda m:m.update(completed_rounds=0),
            lambda m:m.update(exclude_through_game_id=1),lambda m:m['games'][0].update(game_seed='9'),
            lambda m:m['games'][0].update(status='prepared'),lambda m:m['games'].append(m['games'][0]),
            lambda m:m['games'][0].update(input_file='../outside.json')]:
            with tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);db,folder=fixture(root);path=folder/'manifest.json';m=json.loads(path.read_text());mutate(m);path.write_text(json.dumps(m))
                with self.assertRaises(ValueError):audit.run(db,folder,root/'out')
                self.assertFalse((root/'out').exists())

    def test_input_mutation_and_even_semantically_equal_db_payload_change_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);db,folder=fixture(root)
            case=folder/'cases/game_1.json';case.write_text(case.read_text()+'\n')
            with self.assertRaisesRegex(ValueError,'case_hash_mismatch'):audit.run(db,folder,root/'out')
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);db,folder=fixture(root);con=sqlite3.connect(db)
            con.execute("UPDATE player_game_contact_state SET state_json=state_json || ' ' WHERE game_id=1");con.commit();con.close()
            with self.assertRaisesRegex(ValueError,'seeded_contact_provenance_mismatch'):audit.run(db,folder,root/'out')
            self.assertFalse((root/'out').exists())

    def test_environment_cli_and_binary_digest_contract(self):
        h=hashlib.sha256();response.update_contact_hash(h,7,1,'abc',None,None)
        self.assertEqual(h.hexdigest(),'a748868b4a64c88d24f7946edcdf42184265176a310daa18f3bc3dc96091ebc8')
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);db,folder=fixture(root)
            cmd=[sys.executable,str(ROOT/'tools/diagnostics/seeded_holdout_audit.py'),'--run-dir',str(folder),'--output-dir',str(root/'out')]
            p=subprocess.run(cmd,env=dict(os.environ,SABERBB_DB=str(db)),capture_output=True,text=True)
            self.assertEqual(p.returncode,0,p.stderr)
            p=subprocess.run(cmd,env=dict(os.environ,SABERBB_DB=''),capture_output=True,text=True)
            self.assertNotEqual(p.returncode,0);self.assertIn('SABERBB_DB is not set',p.stderr)


if __name__=='__main__':unittest.main()
