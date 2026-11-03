import hashlib
import json
import math
import tempfile
import unittest
from collections import defaultdict
from pathlib import Path
from unittest.mock import patch

from test_forecast_contact_geometry import fixture, csv_rows, replace_csv
from test_swing_prediction_probe import prediction
import bat_axis_origin_audit as audit
from swing_prediction_probe import plan_swing


class BatAxisOriginTest(unittest.TestCase):
    def test_analytic_pivot_and_mirrored_plan(self):
        p=prediction()['input'];p['forecast']['position_world_m']=[0,0,.5]
        p['planned_bat_angle_deg']=45.;p['execution_angle_error_deg']=2.
        row=dict(status='no_contact',collision_status='not_evaluated:no_contact',contact_contract_version='1')
        p['planned_bat_angle_deg']=audit.angle_for_position(p['forecast']['position_world_m'])
        right=plan_swing(p,.9,.16);p['handedness_sign']=-1.;left=plan_swing(p,.9,.16)
        rv,rh=audit.inspect(right,row,defaultdict(float));lv,lh=audit.inspect(left,row,defaultdict(float))
        self.assertEqual((rh,lh),(1,-1));self.assertLess(rv['proxy_pivot_z_m'],0)
        self.assertAlmostEqual(rv['proxy_pivot_z_m'],.5-.9*math.sin(math.radians(45)))
        self.assertEqual(rv,lv)
        self.assertEqual(right['pivot_world_m'][0],-left['pivot_world_m'][0])
        self.assertNotEqual(rv['planned_tilt_deg'],rv['attack_deg'])

    def test_roundtrip_filter_scopes_no_collision_search_and_readonly(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root)
            before={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()}
            with patch('rotating_bat_probe.search',side_effect=AssertionError('search must not run')):
                self.assertEqual(audit.run(source,root/'all'),dict(source_rows=6,selected_rows=6))
            variant='observed_sigma_0.005_r0.9';audit.run(source,root/'focused',[variant])
            full=csv_rows(root/'all'/'bat_axis_origin_summary.csv');focused=csv_rows(root/'focused'/'bat_axis_origin_summary.csv')
            self.assertEqual(focused,[r for r in full if r['variant']==variant])
            overall=[r for r in full if r['stratum']=='overall']
            self.assertEqual(sum(int(r['rows']) for r in overall if r['scope']=='all_swings'),6)
            self.assertEqual(sum(int(r['rows']) for r in overall if r['scope']=='recorded_batting_only'),3)
            self.assertLess(max(float(r['max_error']) for r in csv_rows(root/'all'/'bat_axis_origin_checks.csv')),1e-8)
            self.assertEqual(before,{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()})
            with self.assertRaises(FileExistsError):audit.run(source,root/'all')
            with self.assertRaisesRegex(ValueError,'invalid_variants'):audit.run(source,root/'bad',['bogus'])
            self.assertFalse((root/'bad').exists())

    def test_corrupt_filtered_pose_and_misaligned_rows_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root);path=source/'observation_update_inputs.jsonl'
            original=path.read_text();records=[json.loads(x) for x in original.splitlines()]
            records[0]['plan']['pivot_world_m'][2]+=.1
            path.write_text(''.join(json.dumps(r)+'\n' for r in records))
            with self.assertRaisesRegex(ValueError,'reconstruction_mismatch:pivot_world_m'):
                audit.run(source,root/'bad_pose',['observed_sigma_0.005_r0.9'])
            path.write_text(original)
            rows=csv_rows(source/'observation_update_contacts.csv');rows[0],rows[1]=rows[1],rows[0]
            replace_csv(source/'observation_update_contacts.csv',rows)
            with self.assertRaisesRegex(ValueError,'key_or_geometry_mismatch'):audit.run(source,root/'bad_pair')

    def test_duplicate_missing_and_corrupt_contact_axis(self):
        for mode,pattern in [('duplicate','UNIQUE'),('missing','length_mismatch'),('axis','entry_axis')]:
            with self.subTest(mode=mode),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);source=fixture(root);path=source/'observation_update_inputs.jsonl'
                lines=path.read_text().splitlines();rows=csv_rows(source/'observation_update_contacts.csv')
                if mode=='duplicate':lines[1]=lines[0];rows[1]=dict(rows[0])
                elif mode=='missing':lines.pop()
                else:
                    row=next(r for r in rows if r['status']=='contact');row['axis_at_entry_z']=str(float(row['axis_at_entry_z'])+.1)
                path.write_text('\n'.join(lines)+'\n');replace_csv(source/'observation_update_contacts.csv',rows)
                with self.assertRaisesRegex(ValueError,pattern):audit.run(source,root/'bad')

    def test_phase_clock_and_velocity_corruption_are_rejected(self):
        for field,pattern in [('timing_offset_ms','nominal_clock_sec'),
                              ('rotation_angle_at_entry_deg','phase_rotation_deg'),
                              ('point_velocity_z_ms','entry_point_velocity_ms')]:
            with self.subTest(field=field),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);source=fixture(root)
                rows=csv_rows(source/'observation_update_contacts.csv')
                row=next(r for r in rows if r['status']=='contact')
                row[field]=str(float(row[field])+1.)
                replace_csv(source/'observation_update_contacts.csv',rows)
                with self.assertRaisesRegex(ValueError,pattern):audit.run(source,root/'bad')


if __name__=='__main__':unittest.main()
