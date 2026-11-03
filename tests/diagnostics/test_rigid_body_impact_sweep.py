import csv
import math
import tempfile
import unittest
from pathlib import Path

from test_impact_parameter_sweep import read
import impact_parameter_sweep as p


class RigidBodySweepTest(unittest.TestCase):
    def test_analytic_speed_gradient_and_shared_cm_pivot(self):
        axis=(1.,0.,0.);v=(0.,35.,0.);lever=.9
        values=[p.point_kinematics(axis,v,lever,s,'rigid_body') for s in (-.1,0.,.1)]
        for s,(point,omega,pivot,center) in zip((-.1,0.,.1),values):
            self.assertAlmostEqual(point[1],35*(1+s/lever))
            self.assertAlmostEqual(omega[2],35/lever)
            for k in (0,1,2):
                self.assertAlmostEqual(center[k],values[1][3][k]);self.assertAlmostEqual(pivot[k],0.)
        # Fixed-point mode deliberately changes body translation between axial cases.
        fixed=[p.point_kinematics(axis,v,lever,s,'fixed_point') for s in (-.1,.1)]
        self.assertEqual(fixed[0][0],fixed[1][0]);self.assertNotEqual(fixed[0][3],fixed[1][3])
        for theta in (-45.,30.):
            a=(math.cos(math.radians(theta)),0.,math.sin(math.radians(theta)))
            reference=(0.,34.,3.)
            zero=p.point_kinematics(a,reference,lever,0.,'rigid_body')
            for s in (-.12,.1):
                point,w,vp,center=p.point_kinematics(a,reference,lever,s,'rigid_body')
                for x,y in zip(center,zero[3]):self.assertAlmostEqual(x,y)
                for x,y in zip(vp,zero[2]):self.assertAlmostEqual(x,y)
                expected=p.add(vp,p.cross(w,p.mul(a,lever+s)))
                for x,y in zip(point,expected):self.assertAlmostEqual(x,y)

    def test_both_modes_match_separate_runs_and_s_zero_is_identical(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);kw=dict(tilts=(0.,30.),axials=(0.,.1),samples=51)
            result=p.run(root/'both',velocity_model='both',**kw)
            self.assertEqual(result['response_rows'],2*2*2*51*3)
            both=read(root/'both','samples')
            for mode in ('fixed_point','rigid_body'):
                p.run(root/mode,velocity_model=mode,**kw)
                self.assertEqual(read(root/mode,'samples'),[r for r in both if r['velocity_model']==mode])
            def zero(mode):
                return [{k:v for k,v in r.items() if k!='velocity_model'} for r in both if r['velocity_model']==mode and float(r['axial_m'])==0]
            self.assertEqual(zero('fixed_point'),zero('rigid_body'))
            checks=read(root/'both','checks')
            self.assertLess(max(float(r['max_error']) for r in checks if r['metric'].startswith('rigid_')),1e-10)
            summary=read(root/'both','summary');intervals=read(root/'both','weak_fair_intervals')
            for s in summary:
                own=[r for r in both if all(r[k]==s[k] for k in p.KEYS)]
                self.assertEqual(len(own),int(s['grid_samples']))
                ints=[r for r in intervals if all(r[k]==s[k] for k in p.KEYS)]
                self.assertEqual(sum(int(r['grid_samples']) for r in ints),int(s['weak_fair_samples']))

    def test_invalid_velocity_model_rejected_before_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            out=Path(tmp)/'bad'
            with self.assertRaises(ValueError):p.run(out,velocity_model='unknown')
            self.assertFalse(out.exists())


if __name__=='__main__':unittest.main()
