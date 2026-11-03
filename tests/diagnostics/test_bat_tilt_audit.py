import math
import tempfile
import unittest
from pathlib import Path
from bat_tilt_audit import geometry, TiltAudit


def contact(tilt=30.):
    rad=math.radians(tilt);a=(math.cos(rad),0.,math.sin(rad))
    n=(-.8*math.sin(rad),.6,.8*math.cos(rad))
    r=dict(status='contact',contact_region='side',variant='v',adjusted_contact_type='SolidContact',
        _source_z_m=.71,local_abs_b_over_r=.8,bat_point_speed_ms=33.,collision_status='impulse_contact',
        ev_ms=35.,vla_deg=60.,hla_deg=50.)
    for prefix,v in [('axis_at_entry_',a),('normal_',n),('outgoing_velocity_',(20.,10.,30.))]:
        for axis,x in zip('xyz',v):r[prefix+axis+('_ms' if 'velocity' in prefix else '')]=x
    return r


class TiltTest(unittest.TestCase):
    def test_orthogonal_basis_recovers_tilt_related_lateral_component(self):
        for tilt in (0.,10.,30.,60.,90.):
            values,status,checks=geometry(contact(tilt))
            self.assertEqual(status,'defined');self.assertAlmostEqual(values['tilt_deg'],tilt)
            self.assertAlmostEqual(values['forward_normal_x'],0.)
            self.assertAlmostEqual(values['circumferential_normal_x'],-.8*math.sin(math.radians(tilt)))
            self.assertLess(max(checks.values()),1e-12)

    def test_axis_reversal_preserves_unsigned_tilt_and_component_vectors(self):
        r=contact();before=geometry(r)[0]
        for a in 'xyz':r['axis_at_entry_'+a]*=-1
        after=geometry(r)[0]
        for k in before:self.assertAlmostEqual(before[k],after[k])

    def test_parallel_forward_axis_is_undefined_and_bad_side_normal_fails(self):
        r=contact()
        for a,x in zip('xyz',(0.,1.,0.)):r['axis_at_entry_'+a]=x
        for a,x in zip('xyz',(1.,0.,0.)):r['normal_'+a]=x
        values,status,checks=geometry(r)
        self.assertEqual(status,'forward_projection_undefined')
        self.assertNotIn('forward_normal_x',values)
        r['normal_x']=0.;r['normal_y']=1.
        with self.assertRaisesRegex(ValueError,'nonperpendicular'):geometry(r)
        r['contact_region']='cap'
        self.assertEqual(geometry(r)[1],'not_side')

    def test_counts_partition_and_unavailable_outcomes_are_not_imputed(self):
        import csv
        a=TiltAudit();r=contact()
        a.add(r,{'m':dict(collision_status='not_evaluated:out_of_curve',ev_ms=None)})
        with tempfile.TemporaryDirectory() as tmp:
            a.write(Path(tmp))
            def read(name):
                with (Path(tmp)/name).open() as f:return list(csv.DictReader(f))
            rows=read('finite_barrel_tilt_outcomes.csv')
            xs=[x for x in rows if x['outcome_basis']=='m']
            self.assertTrue(xs);self.assertTrue(all(x['n_ev_ms']=='0' and x['avg_ev_ms']=='' for x in xs))
            geom=read('finite_barrel_tilt_geometry.csv')
            totals={stage:sum(int(x['contacts']) for x in geom if x['scope']=='all_swings' and x['stratum']==stage)
                    for stage in ('overall','tilt','tilt_impact','tilt_impact_normal_z')}
            self.assertTrue(all(x==1 for x in totals.values()))
