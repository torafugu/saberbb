import copy
import json
import math
import tempfile
import unittest
from collections import defaultdict
from pathlib import Path
from unittest.mock import patch

from test_bat_pivot_orientation_probe import source_db
from test_forecast_contact_geometry import csv_rows
import plan_tilt_sweep as sweep
import plan_contact_ev_audit as audit


class ContactEVTest(unittest.TestCase):
    def synthetic(self,tangent_speed=0.,cap=False):
        n=(1,0,0) if cap else (0,1,0)
        vin=(-10,0,tangent_speed) if cap else (0,-10,tangent_speed)
        out=(5,0,tangent_speed) if cap else (0,5,tangent_speed)
        speed=math.sqrt(100+tangent_speed**2)
        row=dict(status='contact',entry_since_release_sec=.4,half_length_m=.16,
            axial_at_entry_m=.2 if cap else 0.,closest_axial_at_entry_m=.16 if cap else 0.,
            contact_region='cap' if cap else 'side',effective_radius_m=.07,min_evaluated_clearance_m=0.,
            relative_speed_ms=speed,approach_speed_ms=10.,normal_speed_fraction=10/speed,bat_point_speed_ms=0.)
        for prefix,values,suffix in [('normal_',n,''),('axis_at_entry_',(1,0,0),''),
                ('ball_velocity_',vin,'_ms'),('point_velocity_',(0,0,0),'_ms')]:
            row.update({prefix+k+suffix:v for k,v in zip('xyz',values)})
        ev,hla,vla=audit.angles(out)
        m=dict(collision_status='not_evaluated:unsupported_cap',ev_ms=None) if cap else dict(
            collision_status='impulse_contact',ev_ms=ev,hla_deg=hla,vla_deg=vla,
            outgoing_world_ms=out,normal_impulse_kg_ms=15*audit.BALL_MASS)
        p=dict(forecast_position_world_m=[0,0,.7],ideal_axis_world=[1,0,0],nominal_time_sec=.4,
            ideal_velocity_world_ms=[0,0,0],execution_angle_error_deg=0)
        r=dict(forecast_plan=p,kinematics=dict.fromkeys(sweep.KINEMATICS,0.),search_status='contact',
            entry_time_sec=.4,contact=row,responses={k:copy.deepcopy(m) for k in ('normal_profile','finite_I0.00015_et0.4')})
        return {'saved_tilt':r,'tilt_0deg':copy.deepcopy(r)}

    def test_geometry_ev_independence_exclusions_and_missing_energy(self):
        a=audit.Audit();models=['normal_profile','finite_I0.00015_et0.4']
        a.add('s',self.synthetic(),models)  # Head-on, EV=5: normal fraction does not define EV.
        a.add('s',self.synthetic(30.),models)  # More oblique, EV>30 from retained tangential motion.
        a.add('s',self.synthetic(cap=True),models)
        with tempfile.TemporaryDirectory() as t:
            out=Path(t);a.write(out);rs=csv_rows(out/'plan_contact_ev_summary.csv')
            head=next(r for r in rs if (r['scenario'],r['model'],r['cohort'],r['stratum'],r['band_1'])==
                ('saved_tilt',models[0],'all_successful','normal_fraction','0.98+'))
            self.assertEqual(head['low_ev'],'1');self.assertAlmostEqual(float(head['avg_ev_ms']),5.)
            self.assertEqual(head['n_ball_rotation_change_j'],'0');self.assertEqual(head['avg_ball_rotation_change_j'],'')
            total=next(r for r in rs if (r['scenario'],r['model'],r['cohort'],r['stratum'])==
                ('saved_tilt',models[0],'geometry_contacts','overall'))
            self.assertEqual(total['contacts'],'3');self.assertEqual(total['successful_contacts'],'2')
            self.assertEqual(total['n_ev_ms'],'2');self.assertEqual(total['n_normal_speed_fraction'],'3')
            excluded=[r for r in csv_rows(out/'plan_contact_ev_selection.csv') if r['contact_region']=='cap']
            self.assertTrue(excluded);self.assertTrue(all(r['collision_status']=='not_evaluated:unsupported_cap' for r in excluded))
            pairs=csv_rows(out/'plan_contact_ev_pairs.csv')
            self.assertTrue(all(float(r['avg_delta_ev_ms'])==0. for r in pairs))

    def fixture(self,root,**kw):
        source,db=source_db(root);out=root/'sweep'
        sweep.run(source,db,out,sample_size=0,**kw)
        return out

    def test_v1_v3_no_db_search_solve_and_denominators(self):
        for kw in ({},{'angles':(0.,15.),'yaws':(0.,),'aim_z_offsets':(-.03,0.,.03)}):
            with self.subTest(kw=kw),tempfile.TemporaryDirectory() as t:
                root=Path(t);source=self.fixture(root,**kw)
                before={p:audit_hash(p) for p in source.iterdir() if p.is_file()}
                with patch('sqlite3.connect',side_effect=AssertionError('DB')), \
                     patch('bat_pivot_orientation_probe.search',side_effect=AssertionError('search')), \
                     patch('finite_barrel_collision.solve',side_effect=AssertionError('finite solve')), \
                     patch('contact_3d.solve_contact',side_effect=AssertionError('normal solve')):
                    result=audit.run([source],root/'out')
                self.assertEqual(result,dict(datasets=1,new_searches=0,db_reads=0,collision_replays=0))
                self.assertEqual(before,{p:audit_hash(p) for p in before})
                rows=csv_rows(root/'out'/'plan_contact_ev_summary.csv')
                hist=csv_rows(root/'out'/'plan_contact_ev_histogram.csv')
                for r in rows:
                    if r['cohort']!='geometry_contacts':
                        cells=[h for h in hist if all(h[k]==r[k] for k in audit.KEYS+['cohort'])]
                        self.assertEqual(sum(int(h['contacts']) for h in cells),int(r['contacts']))
                    if r['stratum']=='overall':
                        for stage in ['normal_fraction','axial_fraction','point_speed','normal_axial','normal_point_speed','forecast_height','handedness']:
                            children=[c for c in rows if c['stratum']==stage and all(c[k]==r[k] for k in ['dataset','scenario','model','cohort'])]
                            for field in audit.COUNTS:
                                self.assertEqual(sum(int(c[field]) for c in children),int(r[field]))
                selected=csv_rows(root/'out'/'plan_contact_ev_selection.csv');totals=defaultdict(int)
                for r in selected:totals[tuple(r[k] for k in ['dataset','scenario','model','stratum'])]+=int(r['trials'])
                self.assertTrue(all(n==2 for n in totals.values()))
                with self.assertRaises(FileExistsError):audit.run([source],root/'out')

    def test_bad_contact_geometry_and_impulse_rejected_before_output(self):
        for field in ['normal_y','point_velocity_y_ms','normal_impulse_world_kg_ms']:
            with self.subTest(field=field),tempfile.TemporaryDirectory() as t:
                root=Path(t);source=self.fixture(root);p=source/'plan_tilt_states.jsonl'
                rows=[json.loads(x) for x in p.read_text().splitlines()]
                r=next(r for r in rows if r.get('search_status')=='contact')
                if field=='normal_impulse_world_kg_ms':
                    m=r['responses']['finite_I0.00015_et0.4'];m[field][1]+=1.
                else:r['contact'][field]+=1.
                p.write_text(''.join(json.dumps(r)+'\n' for r in rows))
                with self.assertRaises(ValueError):audit.run([source],root/'out')
                self.assertFalse((root/'out').exists())

    def test_invalid_clocks_have_no_contact_metrics(self):
        a=audit.Audit();records=self.synthetic()
        for r in records.values():
            r['invalid_reason']='execution_starts_before_plan'
            for field in ['contact','responses','search_status','entry_time_sec','kinematics']:r.pop(field)
        a.add('s',records,['normal_profile','finite_I0.00015_et0.4'])
        self.assertFalse(a.ev_summary)
        self.assertTrue(any(k[3:6]==('normal_fraction','undefined','all') for k in a.ev_selection))


def audit_hash(path):
    from execution_contact_posture import digest
    return digest(path)


if __name__=='__main__':unittest.main()
