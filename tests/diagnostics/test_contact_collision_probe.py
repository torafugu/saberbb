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


class RelativeGeometryTest(unittest.TestCase):
    def test_fixed_reference_limit_matches_old_cylinder_normal(self):
        for theta_deg in (-60,0,30,75):
            theta = math.radians(theta_deg)
            axis = (math.cos(theta),0,math.sin(theta))
            tangent = (-math.sin(theta),0,math.cos(theta))
            for radial in (-.99,-.5,0,.5,.99):
                offset = tuple(.12*a + radial*.07*t for a,t in zip(axis,tangent))
                geometry,reason = probe.relative_contact_geometry((0,-40,0),(0,0,0),axis,offset)
                self.assertIsNone(reason)
                expected = (-math.sin(theta)*radial,math.sqrt(1-radial*radial),math.cos(theta)*radial)
                for actual,value in zip(geometry['normal'],expected):
                    self.assertAlmostEqual(actual,value)
                self.assertLess(geometry['contact_radius_error_m'],1e-12)

    def test_upward_motion_changes_center_normal_and_vla(self):
        alpha = math.radians(15)
        incoming = (0,-40,0)
        bat = (0,35*math.cos(alpha),35*math.sin(alpha))
        entry,reason = probe.relative_contact_geometry(incoming,bat,(1,0,0),(0,0,0))
        self.assertIsNone(reason)
        self.assertGreater(entry['normal'][2],0)
        self.assertLess(entry['first_contact_time_offset_sec'],entry['closest_approach_time_sec'])
        outgoing,relative,_ = probe.collide(incoming,bat,entry['normal'],.5,.6)
        self.assertLess(relative,0)
        self.assertGreater(probe.angles(outgoing)[2],0)
        old,_,_ = probe.collide(incoming,bat,(0,1,0),.5,.6)
        self.assertEqual(probe.angles(old)[2],0)

    def test_normal_entry_geometry_invariance_and_mirroring(self):
        # Multiple tilted axes, face phases and signed offsets, with the saved-data transform.
        for sign in (-1,1):
            for theta in (-45,0,35,70):
                for z in (-.04,0,.04):
                    row = contact(hla_handedness_sign=sign,actual_bat_angle_deg=theta,
                                  contact_offset_x_m=.03,contact_offset_z_m=z,
                                  approach_variation_deg=8)
                    fixed,reason = probe.prepare(row)
                    self.assertIsNone(reason)
                    entry,reason = probe.relative_contact_geometry(fixed['incoming'],fixed['bat'],fixed['axis'],fixed['offset'])
                    self.assertIsNone(reason)
                    self.assertAlmostEqual(probe.dot(entry['normal'],entry['normal']),1)
                    self.assertAlmostEqual(probe.dot(entry['normal'],fixed['axis']),0)
                    self.assertLess(entry['contact_radius_error_m'],1e-12)
                    relative = tuple(i-b for i,b in zip(fixed['incoming'],fixed['bat']))
                    self.assertLessEqual(probe.dot(entry['normal'],relative),1e-10)
                    shifted = tuple(o+.2*a for o,a in zip(fixed['offset'],fixed['axis']))
                    invariant,_ = probe.relative_contact_geometry(fixed['incoming'],fixed['bat'],fixed['axis'],shifted)
                    for actual,value in zip(invariant['normal'],entry['normal']):
                        self.assertAlmostEqual(actual,value)
                    # Mirror all world X coordinates, including axis and saved offset.
                    mirror = lambda v: (-v[0],v[1],v[2])
                    mirrored,_ = probe.relative_contact_geometry(mirror(fixed['incoming']),mirror(fixed['bat']),
                                                                 mirror(fixed['axis']),mirror(fixed['offset']))
                    for actual,value in zip(mirrored['normal'],mirror(entry['normal'])):
                        self.assertAlmostEqual(actual,value)

    def test_degenerate_and_no_intersection_are_explicit(self):
        geometry,reason = probe.relative_contact_geometry((1,0,0),(0,0,0),(1,0,0),(0,0,0))
        self.assertIsNone(geometry)
        self.assertEqual(reason,'relative_no_transverse_motion')
        geometry,reason = probe.relative_contact_geometry((0,-40,0),(0,0,0),(1,0,0),(0,0,.08))
        self.assertIsNone(geometry)
        self.assertEqual(reason,'relative_outside_contact_radius')
        grazing,reason = probe.relative_contact_geometry((0,-40,0),(0,0,0),(1,0,0),(0,0,.07))
        self.assertIsNone(reason)
        self.assertAlmostEqual(grazing['closest_approach_time_sec'],grazing['first_contact_time_offset_sec'])
        self.assertAlmostEqual(grazing['normal'][1],0)


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
            degenerate = contact(bat_speed=0, pitch_speed=0)
            degenerate.pop('pitch_speed')
            degenerate.update(game_id=1,count_seq=5,adjusted_contact_type='SolidContact')
            insert('player_game_batting',degenerate)
            insert('player_game_pitching',dict(game_id=1,count_seq=5,speed=0))
            insert('player_game_batting',dict(game_id=2,count_seq=1,adjusted_contact_type='SolidContact'))
            # Extra throws must not increase the direction population.
            for seq in (1,2):
                insert('player_game_fielding',dict(game_id=1,count_seq=1,seq=seq))
            db.commit()
            db.close()
            before = hashlib.sha256(path.read_bytes()).hexdigest()
            out = folder/'reports'
            coverage = probe.run(path,1,1,out)
            self.assertEqual(coverage['contacts'],5)
            self.assertEqual(coverage['comparable_contacts'],3)
            self.assertEqual(coverage['excluded_contacts'],2)
            self.assertEqual(coverage['pitches'],5)
            self.assertEqual(coverage['probe_version'],2)
            self.assertEqual(coverage['trial_variants'],12)
            self.assertEqual(coverage['old_comparable_contacts'],4)
            def read(name):
                with (out/name).open() as f:
                    return list(csv.DictReader(f))
            summaries = read('collision_summary.csv')
            self.assertEqual(len(summaries),39)
            self.assertTrue(all(int(r['contacts'])==1 for r in summaries))
            self.assertEqual(len(read('collision_contacts.csv')),39)
            self.assertEqual(sum(int(r['contacts']) for r in read('direction_coverage.csv')),5)
            self.assertEqual(sum(int(r['contacts']) for r in read('direction_transitions.csv')),5)
            self.assertEqual(len(read('excluded_inputs.csv')),2)
            checks = read('geometry_checks.csv')
            self.assertEqual(len(checks),2)
            self.assertTrue(all(int(r['contacts'])==3 for r in checks))
            self.assertTrue(all(float(r['max_normal_length_error']) < 1e-12 for r in checks))
            details = read('collision_contacts.csv')
            relatives = [r for r in details if r['geometry_model']=='relative']
            self.assertTrue(all(float(r['relative_normal_ms']) <= 1e-10 for r in relatives))
            self.assertTrue(all(float(r['contact_radius_error_m']) < 1e-12 for r in relatives))
            currents = [r for r in details if r['variant']=='current']
            self.assertTrue(all(r['nx']=='' and r['first_contact_time_offset_sec']=='' for r in currents))
            self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(),before)
            with self.assertRaises(FileExistsError):
                probe.run(path,1,1,out)
            empty = probe.run(path,20,30,folder/'empty')
            self.assertEqual(empty['contacts'],0)
            with self.assertRaises(ValueError):
                probe.run(path,2,1,folder/'invalid')


if __name__ == '__main__':
    unittest.main()
