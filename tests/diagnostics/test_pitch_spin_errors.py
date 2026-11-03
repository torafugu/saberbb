import hashlib
import json
import math
import sqlite3
import tempfile
import unittest
from pathlib import Path

from test_forecast_contact_geometry import fixture, csv_rows
from pitch_spin_source import InvalidPitchSpin, reconstruct
import finite_barrel_probe as probe


def source_with_spin(root,value):
    source=fixture(root);dbpath=root/'test.db'
    with sqlite3.connect(dbpath) as db:
        db.execute('CREATE TABLE player_game_pitching(game_id,count_seq,spin_rate,spin_angle,spin_efficiency)')
        db.executemany('INSERT INTO player_game_pitching VALUES(?,?,?,?,?)',[(10,1,2000.,0.,value),(10,2,2500.,90.,.9)])
    return source,dbpath


class PitchSpinErrorTest(unittest.TestCase):
    def test_reconstruction_errors_identify_fields(self):
        for args,reason in [((2000.,0.,1.1),'spin_efficiency_out_of_range'),
            ((2000.,0.,-.1),'spin_efficiency_out_of_range'),((-1.,0.,.8),'negative_spin_rate'),
            ((2000.,math.nan,.8),'non_finite_spin_angle')]:
            with self.assertRaises(InvalidPitchSpin) as caught:reconstruct(*args,(0.,-40.,0.))
            self.assertEqual(caught.exception.reason,reason)
            self.assertIn('spin_efficiency',str(caught.exception))

    def test_strict_error_names_ball_and_flushes_report_before_stopping(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,dbpath=source_with_spin(root,1.1);out=root/'out'
            with self.assertRaises(InvalidPitchSpin) as caught:probe.run(source,out,pitch_db=dbpath)
            message=str(caught.exception)
            for text in ('spin_efficiency_out_of_range','game_id=10','count_seq=1','spin_efficiency=1.1'):
                self.assertIn(text,message)
            rows=csv_rows(out/'finite_barrel_invalid_spin.csv')
            self.assertEqual(len(rows),1);self.assertEqual(rows[0]['reason'],'spin_efficiency_out_of_range')
            self.assertFalse((out/'finite_barrel_summary.csv').exists())

    def test_exclusion_preserves_db_reports_once_and_keeps_all_model_denominators(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,dbpath=source_with_spin(root,1.1);out=root/'out'
            before=hashlib.sha256(dbpath.read_bytes()).hexdigest()
            coverage=probe.run(source,out,pitch_db=dbpath,invalid_spin_policy='exclude')
            self.assertEqual(hashlib.sha256(dbpath.read_bytes()).hexdigest(),before)
            self.assertEqual(coverage['trial_rows'],6)
            invalid=coverage['not_evaluated:invalid_pitch_spin'];self.assertGreater(invalid,0)
            report=csv_rows(out/'finite_barrel_invalid_spin.csv');self.assertEqual(len(report),1)
            self.assertEqual(report[0]['spin_efficiency'],'1.1')
            status=csv_rows(out/'finite_barrel_status.csv')
            self.assertEqual(sum(int(r['states']) for r in status if r['collision_status']=='not_evaluated:invalid_pitch_spin'),invalid*coverage['models'])
            self.assertEqual(sum(int(r['states']) for r in status),6*coverage['models'])
            for r in csv_rows(out/'finite_barrel_summary.csv'):
                self.assertEqual(int(r['states']),int(r['impulse_contacts'])+int(r['unavailable_states']))
            pairs=csv_rows(out/'finite_barrel_spin_pairs.csv')
            self.assertTrue(any(int(r['paired_contacts'])<int(r['states']) for r in pairs))
            meta=json.loads((out/'finite_barrel_metadata.json').read_text())
            self.assertEqual(meta['invalid_spin_policy'],'exclude')
            self.assertEqual(meta['pitch_spin_source']['invalid_unique_pitches'],1)
            self.assertEqual(meta['pitch_spin_source']['unique_pitches'],1)
            self.assertEqual(meta['pitch_spin_source']['invalid_report_sha256'],hashlib.sha256((out/'finite_barrel_invalid_spin.csv').read_bytes()).hexdigest())

    def test_non_numeric_scalar_is_reported_and_missing_keys_are_not_excluded(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,dbpath=source_with_spin(root,None);out=root/'out'
            probe.run(source,out,pitch_db=dbpath,invalid_spin_policy='exclude')
            self.assertEqual(csv_rows(out/'finite_barrel_invalid_spin.csv')[0]['reason'],'non_numeric_pitch_spin_scalar')
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,dbpath=source_with_spin(root,.8)
            with sqlite3.connect(dbpath) as db:db.execute('DELETE FROM player_game_pitching WHERE count_seq=1')
            with self.assertRaisesRegex(ValueError,'pitch_spin_key_missing_or_duplicate'):
                probe.run(source,root/'out',pitch_db=dbpath,invalid_spin_policy='exclude')
        with self.assertRaises(ValueError):probe.run('missing','out',invalid_spin_policy='exclude')


if __name__=='__main__':unittest.main()
