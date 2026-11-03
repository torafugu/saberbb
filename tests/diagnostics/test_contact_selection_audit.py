import csv
import tempfile
import unittest
from pathlib import Path
from contact_selection_audit import SelectionAudit


def source(ev=40.,vla=0.,**kwargs):
    return dict(status='contact',collision_status='impulse_contact',contact_region='side',
        _source_z_m=.71,variant='v',adjusted_contact_type='SolidContact',
        ev_ms=ev,vla_deg=vla,hla_deg=0.,normal_z=.2,bat_point_speed_ms=35.,
        normal_speed_fraction=.9,local_abs_b_over_r=.2,**kwargs)


def response(fair=True,success=True):
    return dict(collision_status='impulse_contact' if success else 'not_evaluated:unsupported_cap',
                ev_ms=45. if success else None,vla_deg=10. if success else None,
                hla_deg=0. if fair else 50.)


class SelectionTest(unittest.TestCase):
    def test_source_partition_exposes_weak_excluded_contact(self):
        a=SelectionAudit();r=source(ev=10.,vla=60.);r['contact_region']='cap'
        a.add(r,{'m':response(success=False)})
        a.add(source(),{'m':response()})
        base=('all_swings','v','m')
        allg=a.groups[base+('all_geometry_contacts','source_constant_mass_normal')]
        selected=a.groups[base+('geometry_eligible','source_constant_mass_normal')]
        excluded=a.groups[base+('reason:not_evaluated:unsupported_cap','source_constant_mass_normal')]
        self.assertEqual(allg['contacts'],selected['contacts']+excluded['contacts'])
        self.assertEqual(allg['ev_lt25'],1);self.assertEqual(selected['ev_lt25'],0)
        self.assertNotIn(base+('reason:not_evaluated:unsupported_cap','input_spin_model'),a.groups)

    def test_model_fair_selection_uses_current_model_direction(self):
        a=SelectionAudit();a.add(source(),{'fair':response(),'foul':response(fair=False)})
        self.assertIn(('all_swings','v','fair','model_fair','source_constant_mass_normal'),a.groups)
        self.assertNotIn(('all_swings','v','foul','model_fair','source_constant_mass_normal'),a.groups)

    def test_missing_outcome_is_audited_not_zero(self):
        a=SelectionAudit();r=source();r.update(ev_ms=None,vla_deg=None,collision_status='non_approaching')
        a.add(r,{'m':response(success=False)})
        g=a.groups['all_swings','v','m','all_geometry_contacts','source_constant_mass_normal']
        self.assertEqual(g['contacts'],1);self.assertEqual(g['joint_contacts'],0)
        self.assertEqual(g['n_ev_lt25'],0)

    def test_no_contact_only_in_trial_coverage(self):
        a=SelectionAudit();r=source();r['status']='no_contact';a.add(r,{'m':response(success=False)})
        self.assertFalse(a.groups);self.assertEqual(sum(a.coverage.values()),2)

    def test_written_joint_mass_and_predictors(self):
        a=SelectionAudit();a.add(source(),{'m':response()})
        with tempfile.TemporaryDirectory() as tmp:
            a.write(Path(tmp))
            def read(name):
                with (Path(tmp)/name).open() as f:return list(csv.DictReader(f))
            groups={}
            for r in read('finite_barrel_selection_ev_vla.csv'):
                key=tuple(r[k] for k in ['scope','variant','model','stage','outcome_basis'])
                groups[key]=groups.get(key,0)+int(r['count'])
            for r in read('finite_barrel_selection_summary.csv'):
                key=tuple(r[k] for k in ['scope','variant','model','stage','outcome_basis'])
                self.assertEqual(groups[key],int(r['joint_contacts']))
            p=read('finite_barrel_selection_predictors.csv')
            self.assertTrue(all(float(r['avg_source_z_m'])==.71 for r in p))
