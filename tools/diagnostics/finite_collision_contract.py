"""Generate deterministic Python reference cases for the Rust diagnostic collision port."""
import argparse
import copy
from dataclasses import asdict
import json
import math
from pathlib import Path
import random
import sys

from bat_vibration_profile import profile_e
from finite_barrel_collision import solve
from contact_3d import ContactGeometry
from pitch_spin_source import reconstruct
from rust_collision_response_audit import reference_diagnostics,solve_input
from batting_intent import frozen_planner
from rust_execution_contact_audit import compare_execution
from collections import defaultdict
from rotating_bat_probe import rotate


def cases():
    base=dict(geometry=asdict(ContactGeometry(.4,(1.,0.,0.),(0.,1.,0.),.08,.08,.16,'side')),
        vin=(8.,-40.,-8.),centerline_velocity=(0.,35.,0.),ball_omega=(100.,30.,-20.),bat_omega=(0.,0.,20.),
        parameters=dict(mass=.885,transverse_inertia=.885*.23**2,axial_inertia=.00015,cm_offset=.564-.71,
            barrel_radius=.034,restitution=.5,mu=.2,et=.4,ball_mass=.145,ball_radius=.036,inertia_factor=.4))
    core=[]
    def add(name,record):
        try:expected=solve_input(record);error=None
        except ValueError as exc:expected=None;error=str(exc)
        core.append(dict(name=name,input=record,expected=expected,error=error))
    for radius in (0.,.034):
        for mu in (0.,.001,.2,10.):
            for et in (0.,.4):
                record=copy.deepcopy(base);record['parameters'].update(barrel_radius=radius,mu=mu,et=et)
                add(f'r{radius}_mu{mu}_et{et}',record)
    rng=random.Random(20261105)
    for i in range(24):
        r=copy.deepcopy(base);theta=rng.uniform(-math.pi,math.pi);s=rng.uniform(-.15,.12)
        r['geometry'].update(normal_world=(0.,math.cos(theta),math.sin(theta)),axial_center_m=s,closest_axial_m=s)
        normal=r['geometry']['normal_world'];r['centerline_velocity']=(0.,30.,3.)
        r['vin']=tuple(v-rng.uniform(5,50)*n for v,n in zip(r['centerline_velocity'],normal))
        r['ball_omega']=tuple(rng.uniform(-200,200) for _ in range(3));r['bat_omega']=(10.,-5.,20.)
        r['parameters'].update(axial_inertia=rng.choice((.00015,.0003)),restitution=rng.uniform(.2,.5),mu=rng.choice((0.,.001,.2,10.)))
        add(f'rotated_{i}',r)
    r=copy.deepcopy(base);rotation=tuple(.63*x/math.sqrt(14) for x in (1.,2.,3.))
    for key in ('axis_world','normal_world'):r['geometry'][key]=rotate(r['geometry'][key],rotation,1.)
    for key in ('vin','centerline_velocity','ball_omega','bat_omega'):r[key]=rotate(r[key],rotation,1.)
    add('full_world_rotation',r)
    for name,modify in [
        ('non_approaching',lambda r:r.update(vin=(0.,40.,0.))),
        ('energy_increasing',lambda r:(r.update(vin=(.32,-30.,3.),centerline_velocity=(0.,30.,3.)),r['parameters'].update(restitution=1.,mu=10.,et=0.))),
        ('cap',lambda r:r['geometry'].update(normal_world=(1.,0.,0.),axial_center_m=.18,closest_axial_m=.16,region='cap')),
        ('invalid_mass',lambda r:r['parameters'].update(mass=0.)),
        ('invalid_sphere',lambda r:r['parameters'].update(ball_radius=0.)),
        ('invalid_frame',lambda r:r['geometry'].update(axis_world=(2.,0.,0.))),
        ('invalid_mu',lambda r:r['parameters'].update(mu=-1.))]:
        r=copy.deepcopy(base);modify(r);add(name,r)
    spin=[]
    for efficiency in (0.,.9,1.,1.1):
        for sign in (-1,0,1):
            raw=dict(spin_rate=2200.,spin_angle=45.,spin_efficiency=efficiency,gyro_sign=sign);velocity=(2.,-40.,-3.)
            try:expected=reconstruct(raw['spin_rate'],raw['spin_angle'],efficiency,velocity,sign);error=None
            except ValueError as exc:expected=None;error=getattr(exc,'reason',str(exc))
            spin.append(dict(input=raw,velocity=velocity,expected=expected,error=error))
    for raw,velocity in [(dict(spin_rate=None,spin_angle=0.,spin_efficiency=.9,gyro_sign=0),(0.,-40.,0.)),
        (dict(spin_rate=2200.,spin_angle=0.,spin_efficiency=.9,gyro_sign=0),(0.,0.,1.))]:
        try:expected=reconstruct(raw['spin_rate'],raw['spin_angle'],raw['spin_efficiency'],velocity,raw['gyro_sign']);error=None
        except ValueError as exc:expected=None;error=getattr(exc,'reason',str(exc))
        spin.append(dict(input=raw,velocity=velocity,expected=expected,error=error))
    # Reuse the existing synthetic pipeline states, not empirical outcomes.
    sys.path.insert(0,str(Path(__file__).resolve().parents[2]/'tests/diagnostics'))
    from test_rust_intent_collision_probe import sample
    from test_contact_state_probe import state as legacy_state
    integration=[]
    for strategy,preference,clock in [('let_batter_decide','line_drive',False),('seek_line_drive','contact_first',False),
        ('put_ball_in_play','line_drive',False),('let_batter_decide','contact_first',True)]:
        for raw_spin in [(2200.,0.,.9),(2200.,0.,1.1)]:
            state=sample(strategy=strategy,preference=preference,clock_invalid=clock)
            # These legacy diagnostic fields are unrelated to the new response inputs.
            state=dict(legacy_state(),**state)
            rows,error=compare_execution(state,1,1,frozen_planner(),defaultdict(float));assert error is None
            expected=reference_diagnostics(state,rows,raw_spin,defaultdict(float))
            integration.append(dict(state=state,spin=raw_spin,expected=expected))
    points=[.53,.54,.55,.71,.835,.84,.85]
    return dict(version=1,core=core,spin=spin,profile=[dict(z=z,expected=profile_e(z)) for z in points],integration=integration)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    data=cases();parts=['{"version":1']
    for key in ('core','spin','profile','integration'):
        parts.append(json.dumps(key)+':[\n'+',\n'.join(json.dumps(c,separators=(',',':'),allow_nan=False) for c in data[key])+'\n]')
    a.output.write_text(',\n'.join(parts)+'}\n')
