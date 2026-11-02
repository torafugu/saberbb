import copy
import json
import math
import tempfile
import unittest
from collections import defaultdict
from pathlib import Path

from test_forecast_contact_geometry import fixture, csv_rows, replace_csv
import contact_point_speed_probe as probe
import forecast_contact_geometry as geometry


def analytic_event(axial=0.,phase=0.,normal=None,vin=(0.,-40.,-2.)):
    a=probe.rotate((1.,0.,0.),(0.,0.,10.),phase)
    if normal is None:normal=(-a[1],a[0],0.)
    velocity=probe.cross((0.,0.,10.),probe.mul(a,.9+axial))
    plan=dict(actual_axis_world=(1.,0.,0.),actual_omega_world_rad_s=(0.,0.,10.),pivot_velocity_world_ms=(0.,0.,0.),
        actual_velocity_world_ms=(0.,9.,0.),lever_m=.9,half_length_m=.16,nominal_time_sec=.4)
    record=dict(status='contact',entry_since_release_sec=.4+phase,entry_time_offset_ms=str(phase*1000),
        timing_offset_ms='0',axial_at_entry_m=axial,closest_axial_at_entry_m=axial,half_length_m=.16,contact_region='side')
    for prefix,v in [('axis_at_entry_',a),('normal_',normal),('ball_velocity_',vin),('point_velocity_',velocity)]:
        for axis,value in zip('xyz',v):record[prefix+axis+('_ms' if 'velocity' in prefix else '')]=str(value)
    c=probe.ContactGeometry(.4+phase,a,normal,axial,axial,.16,'side')
    record['collision_status']=probe.solve_contact(c,vin,velocity)['collision_status']
    return record,plan


class ContactPointSpeedTest(unittest.TestCase):
    def test_nominal_center_identical_and_axial_effect_analytic(self):
        row,p=analytic_event();m=probe.compare(row,p,defaultdict(float))
        self.assertEqual(m[probe.MODES[0]],m[probe.MODES[1]])
        row,p=analytic_event(axial=.08);m=probe.compare(row,p,defaultdict(float))
        before,after=(m[k] for k in probe.MODES)
        expected_delta=1.5/(1+.145/.6)*(-.8)
        self.assertAlmostEqual(after['impulse_delta_ms']-before['impulse_delta_ms'],expected_delta)
        self.assertLess(after['ev_ms'],before['ev_ms'])
        # The intervention never edits the event geometry or incoming velocity.
        self.assertEqual(row['closest_axial_at_entry_m'],.08)
        self.assertEqual(row['normal_y'],'1.0')

    def test_phase_reconstruction_and_nonapproaching_control(self):
        row,p=analytic_event(phase=.01);checks=defaultdict(float)
        probe.compare(row,p,checks);self.assertLess(max(checks.values()),1e-10)
        bad=copy.deepcopy(p);bad['nominal_time_sec']+=.001
        with self.assertRaises(ValueError):probe.compare(row,bad,defaultdict(float))
        row,p=analytic_event(phase=math.pi/10,normal=(0.,-1.,0.),vin=(0.,0.,0.))
        result=probe.compare(row,p,defaultdict(float))
        self.assertEqual(result[probe.MODES[0]]['collision_status'],'impulse_contact')
        self.assertEqual(result[probe.MODES[1]]['collision_status'],'non_approaching')
        self.assertIsNone(result[probe.MODES[1]]['vla_deg'])
        row.update(variant='test',adjusted_contact_type='SolidContact',forecast_error_z_m=0.,local_abs_b_over_r=.1,
            signed_axial_fraction=0.,normal_z=0.,normal_speed_fraction=1.)
        a=probe.Accumulator();a.add(row,result)
        self.assertTrue(all(g['paired_impulse_contacts']==0 for g in a.pairs.values()))

    def test_integration_baseline_reproduced_scopes_closure_and_readonly(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=fixture(root)
            before={p.name:probe.digest(p) for p in source.iterdir()}
            output=root/'out';coverage=probe.run(source,output)
            self.assertEqual(coverage['trial_rows'],6);self.assertEqual(coverage['plan_rows'],6)
            self.assertEqual(coverage['collision_trial_rows'],12)
            status=csv_rows(output/'contact_speed_status.csv')
            self.assertEqual(sum(int(r['states']) for r in status),12)
            summary=csv_rows(output/'contact_speed_summary.csv')
            for scope,n in [('all_swings',2),('recorded_batting_only',1)]:
                rs=[r for r in summary if r['scope']==scope and r['stage']=='overall']
                self.assertEqual(len(rs),6)
                self.assertTrue(all(int(r['states'])==n for r in rs))
            geometry.run(source,root/'geometry')
            old=csv_rows(root/'geometry/forecast_geometry_chain.csv')
            for r in summary:
                if r['stage']!='overall':continue
                if r['avg_incoming_z_ms']:
                    self.assertAlmostEqual(float(r['avg_incoming_z_ms'])+float(r['avg_impulse_z_ms']),float(r['avg_outgoing_z_ms']))
                if r['velocity_mode']!=probe.MODES[0]:continue
                o=next(x for x in old if x['scope']==r['scope'] and x['variant']==r['variant'] and x['stage']=='overall')
                for k in ['avg_ev_ms','avg_vla_deg','impulse_contacts']:
                    if r[k]:self.assertAlmostEqual(float(r[k]),float(o[k]))
            self.assertEqual(before,{p.name:probe.digest(p) for p in source.iterdir()})
            checks=csv_rows(output/'contact_speed_checks.csv')
            self.assertLess(max(float(r['max_error']) for r in checks),1e-7)
            with self.assertRaises(FileExistsError):probe.run(source,output)

    def test_missing_duplicate_and_corrupt_plan_are_rejected(self):
        for change in ('missing','duplicate_plan','wrong_key','wrong_phase','wrong_reference','duplicate_contact'):
            with self.subTest(change=change),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);source=fixture(root);path=source/'observation_update_inputs.jsonl'
                records=[json.loads(x) for x in path.read_text().splitlines()]
                if change=='missing':records.pop()
                elif change=='duplicate_plan':records[-1]=copy.deepcopy(records[0])
                elif change=='wrong_key':records[0]['count_seq']=99
                elif change=='wrong_phase':
                    for r in records:r['plan']['nominal_time_sec']+=.01
                elif change=='wrong_reference':records[0]['plan']['actual_velocity_world_ms'][1]+=1
                else:
                    c=source/'observation_update_contacts.csv';rows=csv_rows(c);rows[-1]=copy.deepcopy(rows[0]);replace_csv(c,rows)
                path.write_text(''.join(json.dumps(r)+'\n' for r in records))
                with self.assertRaises(ValueError):probe.run(source,root/'out')

    def test_noncontact_preserved_without_fabricated_collision(self):
        row=dict(status='no_contact');result=probe.compare(row,{},defaultdict(float))
        for m in result.values():
            self.assertEqual(m['collision_status'],'not_evaluated:no_contact')
            self.assertTrue(all(m[k] is None for k in probe.METRICS))


if __name__=='__main__':unittest.main()
