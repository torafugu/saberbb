import argparse
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_forecast_contact_geometry import fixture,csv_rows
from test_swing_prediction_probe import ROOT
from diagnostic_db import add_db_argument


class DiagnosticDbTest(unittest.TestCase):
    def parser(self,optional=False):
        p=argparse.ArgumentParser();add_db_argument(p,'--pitch-db' if optional else '--db',not optional)
        return p

    def test_environment_expansion_override_and_blank(self):
        with patch.dict(os.environ,{'SABERBB_DB':'~/data/with space/saberbb.db'}):
            p=self.parser()
            self.assertEqual(p.parse_args([]).db,Path('~/data/with space/saberbb.db').expanduser())
            self.assertEqual(p.parse_args(['--db','override.db']).db,Path('override.db'))
        with patch.dict(os.environ,{'SABERBB_DB':' '}):
            self.assertIsNone(self.parser(True).parse_args([]).pitch_db)
            with self.assertRaises(SystemExit),patch('sys.stderr'):self.parser().parse_args([])

    def test_finite_cli_environment_and_explicit_spin_precedence(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root)
            import sqlite3
            db=sqlite3.connect(root/'test.db')
            db.execute('CREATE TABLE player_game_pitching(game_id,count_seq,spin_rate,spin_angle,spin_efficiency)')
            for seq in (1,2):db.execute('INSERT INTO player_game_pitching VALUES(10,?,2200,0,.9)',(seq,))
            db.commit();db.close()
            env=dict(os.environ,SABERBB_DB=str(root/'test.db'))
            base=[sys.executable,str(ROOT/'tools/diagnostics/finite_barrel_probe.py'),'--input-dir',str(source),
                '--diagnostics','tilt','--variants','observed_sigma_0.005_r0.9','--models','finite_I0.00015_et0.4']
            for mode,extra in [('env',[]),('explicit',['--pitch-db',str(root/'test.db')]),('spin',['--ball-spin-rad-s','0','0','0'])]:
                use_env=dict(env)
                if mode!='env':use_env['SABERBB_DB']=str(root/'missing.db')
                result=subprocess.run(base+['--output-dir',str(root/mode)]+extra,env=use_env,capture_output=True,text=True)
                self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(csv_rows(root/'env'/'finite_barrel_tilt_outcomes.csv'),csv_rows(root/'explicit'/'finite_barrel_tilt_outcomes.csv'))
            result=subprocess.run(base+['--output-dir',str(root/'conflict'),'--pitch-db',str(root/'test.db'),
                '--ball-spin-rad-s','0','0','0'],env=env,capture_output=True,text=True)
            self.assertNotEqual(result.returncode,0)
            self.assertFalse((root/'conflict').exists())

    def test_all_database_clis_advertise_environment(self):
        names=['contact_collision_probe','contact_state_probe','rotating_bat_probe','execution_frame_probe',
            'swing_prediction_probe','observation_update_probe','symmetric_height_probe','stance_axis_probe',
            'bat_pivot_orientation_probe','finite_barrel_probe']
        for name in names:
            with self.subTest(name=name):
                r=subprocess.run([sys.executable,str(ROOT/'tools/diagnostics'/f'{name}.py'),'--help'],capture_output=True,text=True)
                self.assertEqual(r.returncode,0,r.stderr);self.assertIn('SABERBB_DB',r.stdout)
