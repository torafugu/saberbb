import csv
import tempfile
import unittest
from pathlib import Path
from geometry_direction_audit import DirectionAudit


def row(condition='exact_observation_control',seq=1,hla=0.,status='contact'):
    return dict(game_id=1,count_seq=seq,lever_m=.9,forecast_condition=condition,variant=condition+'_r0.9',
        adjusted_contact_type='SolidContact',status=status,
        collision_status='impulse_contact' if status=='contact' else 'not_evaluated:'+status,
        ev_ms=20. if status=='contact' else None,vla_deg=60. if status=='contact' else None,
        hla_deg=hla if status=='contact' else None,normal_x='0',normal_y='1',normal_z=.5,
        local_abs_b_over_r=.8,_source_z_m=.71)


class DirectionTest(unittest.TestCase):
    def test_same_pitch_pairs_ignore_order_and_retain_before_geometry(self):
        a=DirectionAudit()
        try:
            a.add(row('observed_sigma_0.005',status='no_contact'),{})
            a.add(row(),{})
            keys,rows=a.transition_rows()
            selected=[r for r in rows if r['scope']=='all_swings' and r['stratum']=='impact_normal_z']
            self.assertEqual(len(selected),1)
            r=selected[0]
            self.assertEqual(r['paired_trials'],1)
            self.assertEqual(r['before_region'],'low_ev_and_high_vla')
            self.assertIn('no_contact',r['after_state'])
            self.assertNotEqual(r['band_1'],'undefined')
            self.assertEqual(r['before_direction'],'fair_direction')
        finally:a.close()

    def test_overlaps_unresolved_and_noncontacts_are_not_misses(self):
        a=DirectionAudit()
        try:
            for seq,status in enumerate(['no_contact','overlap_at_start','unresolved'],1):
                a.add(row(seq=seq),{});a.add(row('observed_sigma_0.005',seq=seq,status=status),{})
            _,rows=a.transition_rows()
            states={r['after_state'] for r in rows if r['stratum']=='overall'}
            self.assertEqual(len(states),3)
        finally:a.close()

    def test_direction_boundary_and_joint_mass(self):
        a=DirectionAudit()
        try:
            for seq,hla in enumerate([-45.,44.999,45.],1):
                r=row(seq=seq,hla=hla);a.add(r,{'model':dict(r)})
            with tempfile.TemporaryDirectory() as tmp:
                a.write(Path(tmp))
                def read(name):
                    with (Path(tmp)/name).open() as f:return list(csv.DictReader(f))
                joint=read('finite_barrel_direction_joint.csv')
                for basis in ['model','source_constant_mass_normal']:
                    xs=[r for r in joint if r['scope']=='all_swings' and r['outcome_basis']==basis]
                    self.assertEqual(sum(int(r['contacts']) for r in xs),3)
                    self.assertEqual(sum(int(r['contacts']) for r in xs if r['direction']=='foul_direction'),2)
        finally:a.close()

    def test_circular_mean_does_not_average_wrap_to_zero(self):
        a=DirectionAudit()
        try:
            a.add(row(seq=1,hla=-179.),{});a.add(row(seq=2,hla=179.),{})
            with tempfile.TemporaryDirectory() as tmp:
                a.write(Path(tmp))
                with (Path(tmp)/'finite_barrel_direction_summary.csv').open() as f:
                    r=next(x for x in csv.DictReader(f) if x['scope']=='all_swings' and x['stratum']=='overall')
                self.assertAlmostEqual(abs(float(r['circular_mean_hla_deg'])),180.)
        finally:a.close()

    def test_unavailable_model_has_no_fabricated_joint_outcome(self):
        a=DirectionAudit()
        try:
            a.add(row(),{'model':dict(collision_status='not_evaluated',ev_ms=None)})
            self.assertTrue(a.summary)
            self.assertTrue(all(k[2]=='source_constant_mass_normal' for k in a.summary))
        finally:a.close()
