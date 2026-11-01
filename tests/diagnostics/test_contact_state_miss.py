import csv
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0,str(Path(__file__).resolve().parents[2]/'tools/diagnostics'))
import contact_state_miss as miss
from contact_collision_probe import relative_contact_geometry


def sample(delay=.02, z=0, sign=1):
    aim=(0,0,z);bat=(0,40,0);incoming=(0,-40,0);axis=(sign,0,0)
    state={'input':dict(legacy_contact_offset_xz_m=[.3,z],bat_angle_deg=0,
        timing_offset_sec=delay,ball_velocity_world_ms=incoming,aim_error_world_m=aim,
        reference_time_sec_since_release=.4), 'bat_axis_world':axis,'bat_velocity_world_ms':bat}
    relative=(0,40*delay,z)
    entry,reason=relative_contact_geometry(incoming,bat,axis,relative)
    return miss.diagnose(dict(game_id=1,count_seq=1,adjusted_contact_type='SwungAndMiss'),
                         state,relative,entry,reason)


class MissDiagnosticsTest(unittest.TestCase):
    def test_strict_thresholds_and_length_precedence(self):
        def classify(x,z,angle=0):
            return miss.legacy_group(dict(legacy_contact_offset_xz_m=[x,z],bat_angle_deg=angle))[0]
        self.assertEqual(classify(.16,.07),'contact')
        self.assertEqual(classify(-.160001,.2),'length_miss')
        self.assertEqual(classify(0,-.070001),'thickness_miss')
        self.assertEqual(classify(0,.17,90),'length_miss')

    def test_timing_changes_entry_time_without_changing_impact(self):
        r=sample()
        self.assertEqual(r['legacy_group'],'length_miss')
        self.assertEqual(r['classification_consistent'],1)
        self.assertEqual(r['b_over_r'],0)
        self.assertEqual(r['delta_b_over_r'],0)
        self.assertAlmostEqual(r['delta_first_entry_ms'],10)
        self.assertAlmostEqual(r['first_entry_offset_ms'],9.125)
        self.assertAlmostEqual(r['closest_time_slope_ms_per_ms'],.5)
        self.assertLess(r['closest_phase_identity_error_sec'],1e-12)
        self.assertEqual(r['within_legacy_axial_window'],1)
        self.assertEqual(sample(sign=-1)['first_entry_offset_ms'],r['first_entry_offset_ms'])

    def test_misses_kept_and_empty_headers_and_null_correlation(self):
        hit=sample();nohit=sample(z=.08)
        self.assertEqual(nohit['cylinder_intersection'],0)
        self.assertIsNone(nohit['first_entry_offset_ms'])
        self.assertGreater(abs(nohit['b_over_r']),1)
        with tempfile.TemporaryDirectory() as folder:
            folder=Path(folder)
            miss.write_reports(folder,[hit,nohit])
            with (folder/'state_miss_summary.csv').open() as f:
                total=next(r for r in csv.DictReader(f) if r['timing_band']=='all')
            self.assertEqual(total['valid_states'],'2')
            self.assertEqual(total['intersection_pct'],'50.0')
            self.assertEqual(total['geometry_states'],'2')
            self.assertEqual(total['timing_entry_pearson_r'],'')
            folder=folder/'empty'
            folder.mkdir()
            miss.write_reports(folder,[])
            self.assertEqual(len((folder/'state_miss_contacts.csv').read_text().splitlines()),1)
            self.assertEqual(len((folder/'state_miss_summary.csv').read_text().splitlines()),1)
        self.assertEqual(miss.relation([(1,2),(2,4),(3,6)]),(1.0,2.0))
        self.assertEqual(miss.relation([(1,2),(1,2)]),(None,None))

    def test_phase_identity_for_oblique_motion_and_degenerate_case(self):
        state={'input':dict(legacy_contact_offset_xz_m=[0,0],bat_angle_deg=0,
            timing_offset_sec=.01,ball_velocity_world_ms=[2,-30,-3],
            aim_error_world_m=[.001,0,.02],reference_time_sec_since_release=.4),
            'bat_axis_world':[1,0,0],'bat_velocity_world_ms':[0,35,6]}
        relative=[.001,.35,.08]
        row=dict(game_id=1,count_seq=2,adjusted_contact_type='UnrecordedBatting')
        entry,reason=relative_contact_geometry(state['input']['ball_velocity_world_ms'],
            state['bat_velocity_world_ms'],state['bat_axis_world'],relative)
        r=miss.diagnose(row,state,relative,entry,reason)
        self.assertLess(r['b_phase_identity_error_m'],1e-12)
        self.assertLess(r['closest_phase_identity_error_sec'],1e-12)
        self.assertIsNone(r['classification_consistent'])
        state['input']['ball_velocity_world_ms']=[1,35,6]
        r=miss.diagnose(row,state,relative,None,'relative_no_transverse_motion')
        self.assertIsNone(r['b_over_r'])
        self.assertIsNone(r['closest_phase_identity_error_sec'])


if __name__=='__main__':unittest.main()
