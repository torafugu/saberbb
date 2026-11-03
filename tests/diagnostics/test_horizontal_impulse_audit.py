import csv
import tempfile
import unittest
from pathlib import Path

from contact_collision_probe import BALL_MASS, angles
from horizontal_impulse_audit import decompose, HorizontalAudit
import test_finite_barrel_probe as finite_test


def row():
    r=dict(game_id=1,count_seq=1,variant='test',status='contact',collision_status='impulse_contact',
           adjusted_contact_type='SolidContact',local_abs_b_over_r=.8,normal_z=0.,_source_z_m=.71)
    for prefix,v in [('ball_velocity_',(8.,-40.,-8.)),('point_velocity_',(0.,35.,0.)),
                     ('normal_',(0.,1.,0.)),('axis_at_entry_',(1.,0.,0.))]:
        for a,x in zip('xyz',v):r[prefix+a+('_ms' if 'velocity' in prefix else '')]=x
    r.update(impulse_delta_ms=80.,ev_ms=None,hla_deg=None,vla_deg=None)
    out=(8.,40.,-8.)
    for a,x in zip('xyz',out):r['outgoing_velocity_'+a+'_ms']=x
    r['ev_ms'],r['hla_deg'],r['vla_deg']=angles(out)
    return r


class HorizontalTest(unittest.TestCase):
    def test_actual_solver_vectors_close_for_friction_and_spin(self):
        helper=finite_test.FiniteBarrelTest()
        for mu in (0.,.2):
            for et in (0.,.4):
                m=helper.solve(mu=mu,et=et,spin=(100.,30.,-20.))
                values,_,_,error,ortho=decompose(row(),m)
                self.assertLess(error,1e-10);self.assertLess(ortho,1e-10)
                for a in 'xyz':
                    self.assertAlmostEqual(values['outgoing_'+a],values['incoming_'+a]+values['normal_delta_'+a]+values['tangent_delta_'+a])
                if mu==0:self.assertTrue(all(abs(values['tangent_delta_'+a])<1e-10 for a in 'xyz'))

    def test_source_normal_closes_and_corrupt_vectors_are_rejected(self):
        r=row();_,_,regime,error,_=decompose(r,r,source=True)
        self.assertEqual(regime,'forward_fair_cone');self.assertLess(error,1e-10)
        m=finite_test.FiniteBarrelTest().solve()
        m['tangent_impulse_world_kg_ms']=(1.,0.,0.)
        with self.assertRaisesRegex(ValueError,'closure'):decompose(r,m)

    def test_horizontal_regimes_and_boundary(self):
        r=row()
        for out,expected in [((10.,0.,0.),'nonforward'),((10.,5.,0.),'forward_lateral_dominant'),
                             ((5.,10.,0.),'forward_fair_cone'),((10.,10.,0.),'forward_lateral_dominant')]:
            vin=(8.,-40.,-8.)
            # Use scalar normal along Y to keep tangent orthogonal.
            jn=(out[1]-vin[1])*BALL_MASS
            jt=((out[0]-vin[0])*BALL_MASS,0.,(out[2]-vin[2])*BALL_MASS)
            ev,hla,vla=angles(out)
            m=dict(outgoing_world_ms=out,normal_impulse_world_kg_ms=(0.,jn,0.),
                   tangent_impulse_world_kg_ms=jt,ev_ms=ev,hla_deg=hla,vla_deg=vla)
            self.assertEqual(decompose(r,m)[2],expected)

    def test_missing_model_coverage_and_bounded_examples(self):
        a=HorizontalAudit(sample_limit=1);r=row()
        a.add(r,{'finite':dict(collision_status='not_evaluated:unsupported_cap')})
        a.add(r,{'finite':finite_test.FiniteBarrelTest().solve()})
        self.assertEqual(len(a.examples),1)
        self.assertEqual(sum(g['contacts'] for k,g in a.summary.items() if k[0]=='all_swings' and k[2]=='finite' and k[3]=='overall'),1)
        with tempfile.TemporaryDirectory() as tmp:
            a.write(Path(tmp))
            with (Path(tmp)/'finite_barrel_horizontal_coverage.csv').open() as f:rows=list(csv.DictReader(f))
            self.assertTrue(any(x['collision_status']=='not_evaluated:unsupported_cap' for x in rows))
