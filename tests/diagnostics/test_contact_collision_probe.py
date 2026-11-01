import csv
import hashlib
import importlib.util
import math
import re
import sqlite3
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('probe', ROOT/'tools/diagnostics/contact_collision_probe.py')
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


def contact(**changes):
    row = dict(contact_offset_x_m=0,contact_offset_z_m=0,actual_bat_angle_deg=30,
               hla_handedness_sign=1,bat_speed=35,pitch_speed=40,launch_speed=46,
               launch_angle=10,final_hla_deg=0,approach_variation_deg=0,pull_bias_deg=0,
               timing_angle_error=0,hla_model_version=2,signed_thickness_offset_m=0)
    row.update(changes)
    return row


class CollisionPhysicsTest(unittest.TestCase):
    def test_centered_finite_mass_impulse_and_shared_output_vector(self):
        outgoing, relative, delta = probe.collide((0,-40,0),(0,35,0),(0,1,0),.5,.6)
        expected_delta = 1.5*75/(1+.145/.6)
        self.assertAlmostEqual(relative,-75)
        self.assertAlmostEqual(delta,expected_delta)
        self.assertAlmostEqual(outgoing[1],-40+expected_delta)
        bat_after = 35 - delta*.145/.6
        self.assertAlmostEqual(.145*outgoing[1]+.6*bat_after, .145*(-40)+.6*35)
        self.assertAlmostEqual(outgoing[1]-bat_after, -.5*relative)
        ev,hla,vla = probe.angles(outgoing)
        self.assertAlmostEqual(ev,outgoing[1])
        self.assertEqual((hla,vla),(0,0))

    def test_stationary_infinite_mass_elastic_control_matches_reference(self):
        for radial in (-.99,-.5,0,.5,.99):
            theta = math.radians(30)
            nx,ny,nz = -math.sin(theta)*radial,math.sqrt(1-radial*radial),math.cos(theta)*radial
            outgoing,_,_ = probe.collide((0,-1,0),(0,0,0),(nx,ny,nz),1,math.inf)
            self.assertAlmostEqual(probe.dot(outgoing,outgoing),1)
            _,hla,_ = probe.angles(outgoing)
            self.assertAlmostEqual(hla,math.degrees(math.atan2(2*ny*nx,2*ny*ny-1)))

    def test_galilean_invariance_and_separating_contact(self):
        vin,bat,n = (2,-40,1),(0,35,4),(0,1,0)
        base,_,_ = probe.collide(vin,bat,n,.5,.6)
        shift = (3,8,-2)
        moved,_,_ = probe.collide(tuple(v+s for v,s in zip(vin,shift)),
                                tuple(v+s for v,s in zip(bat,shift)),n,.5,.6)
        for a,b,s in zip(base,moved,shift):
            self.assertAlmostEqual(b,a+s)
        out,relative,delta = probe.collide((0,40,0),(0,0,0),n,.5,.6)
        self.assertEqual(out,(0,40,0))
        self.assertGreater(relative,0)
        self.assertEqual(delta,0)
        self.assertEqual(probe.angles((0,0,0)),(0,None,None))
        with self.assertRaises(ValueError):
            probe.collide(vin,bat,(0,2,0),.5,.6)

    def test_reconstruction_mirroring_and_input_gates(self):
        row = contact(contact_offset_z_m=.03,approach_variation_deg=3)
        row['signed_thickness_offset_m'] = .03*math.cos(math.radians(30))
        right,reason = probe.prepare(row)
        self.assertIsNone(reason)
        self.assertEqual(right['signed_thickness_mismatch'],0)
        self.assertAlmostEqual(probe.dot(right['normal'],right['normal']),1)
        self.assertAlmostEqual(right['attack_deg'],10-.6*math.degrees(math.asin(.03/.07)))
        left,_ = probe.prepare(dict(row,hla_handedness_sign=-1))
        a,_,_ = probe.collide(right['incoming'],right['bat'],right['normal'],.5,.6)
        b,_,_ = probe.collide(left['incoming'],left['bat'],left['normal'],.5,.6)
        self.assertAlmostEqual(a[0],-b[0])
        self.assertAlmostEqual(a[1],b[1])
        self.assertAlmostEqual(a[2],b[2])
        for invalid in [dict(row,pitch_speed=None),dict(row,bat_speed=math.nan),
                        dict(row,hla_model_version=1),dict(row,contact_offset_z_m=1)]:
            self.assertIsNotNone(probe.prepare(invalid)[1])


class DatabaseReportTest(unittest.TestCase):
    def test_paired_population_coverage_reports_and_read_only_database(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp)
            path = folder/'space # question?.db'
            db = sqlite3.connect(path)
            for table in ('batting','pitching','fielding'):
                ddl = (ROOT/f'migrations/ddl/player_game_{table}_table.sql').read_text()
                db.executescript(re.sub(r'^DROP TABLE .*?;\s*','',ddl))
            def insert(table,data):
                defaults = {r[1]:(0 if 'REAL' in r[2] or 'INT' in r[2] else '')
                            for r in db.execute(f'pragma table_info({table})') if r[3]}
                defaults.update(data)
                db.execute(f'insert into {table} ('+','.join(defaults)+') values ('
                           +','.join('?' for _ in defaults)+')',list(defaults.values()))
            for seq,kind in enumerate(probe.KINDS,1):
                values = contact()
                values.pop('pitch_speed')
                values.update(game_id=1,count_seq=seq,adjusted_contact_type=kind,result='Out')
                insert('player_game_batting',values)
                insert('player_game_pitching',dict(game_id=1,count_seq=seq,speed=40))
            insert('player_game_batting',dict(game_id=1,count_seq=4,adjusted_contact_type='WeakContact'))
            insert('player_game_batting',dict(game_id=2,count_seq=1,adjusted_contact_type='SolidContact'))
            # Extra throws must not increase the direction population.
            for seq in (1,2):
                insert('player_game_fielding',dict(game_id=1,count_seq=1,seq=seq))
            db.commit()
            db.close()
            before = hashlib.sha256(path.read_bytes()).hexdigest()
            out = folder/'reports'
            coverage = probe.run(path,1,1,out)
            self.assertEqual(coverage['contacts'],4)
            self.assertEqual(coverage['comparable_contacts'],3)
            self.assertEqual(coverage['excluded_contacts'],1)
            self.assertEqual(coverage['pitches'],4)
            def read(name):
                with (out/name).open() as f:
                    return list(csv.DictReader(f))
            summaries = read('collision_summary.csv')
            self.assertEqual(len(summaries),21)
            self.assertTrue(all(int(r['contacts'])==1 for r in summaries))
            self.assertEqual(len(read('collision_contacts.csv')),21)
            self.assertEqual(sum(int(r['contacts']) for r in read('direction_coverage.csv')),4)
            self.assertEqual(sum(int(r['contacts']) for r in read('direction_transitions.csv')),4)
            self.assertEqual(len(read('excluded_inputs.csv')),1)
            self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(),before)
            with self.assertRaises(FileExistsError):
                probe.run(path,1,1,out)
            empty = probe.run(path,20,30,folder/'empty')
            self.assertEqual(empty['contacts'],0)
            with self.assertRaises(ValueError):
                probe.run(path,2,1,folder/'invalid')


if __name__ == '__main__':
    unittest.main()
