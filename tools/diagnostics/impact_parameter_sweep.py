#!/usr/bin/env python3
"""Frozen local b/R sweep: no database, contact search or population-rate inference."""
import argparse
import csv
import json
import math
from collections import Counter, defaultdict
from itertools import product
from pathlib import Path

import bat_vibration_profile as profile
from contact_3d import ContactGeometry
from contact_collision_probe import cross, dot, write_csv
from contact_recoil import inverse_mass, solve as normal_solve, recoil_checks
from contact_spin import BALL_RADIUS_M
from effective_mass_probe import direction
from finite_barrel_collision import solve as finite_solve
from forecast_contact_geometry import local_impact
from ground_spin_probe import check_metrics
from pitch_spin_source import reconstruct
from rotating_bat_probe import add, sub, mul, norm

MODELS=('normal_profile','finite_mu0','finite_I0.00015_et0.4')
KEYS=['tilt_deg','axial_m','model']
FIELDS=KEYS+['b_over_r','normal_x','normal_y','normal_z','collision_status','direction',
    'ev_ms','hla_deg','vla_deg','outgoing_x_ms','outgoing_y_ms','outgoing_z_ms','spin_rpm']


def normal_for_b(axis,relative,b):
    transverse=sub(relative,mul(axis,dot(relative,axis)))
    if not math.isfinite(b) or abs(b)>=1 or norm(transverse)<1e-12:
        raise ValueError('invalid_local_impact')
    forward=mul(transverse,-1/norm(transverse))
    return add(mul(forward,math.sqrt(1-b*b)),mul(cross(axis,forward),b))


def responses(axis,normal,axial,vin,vbat,omega,spin,lever,checks):
    g=ContactGeometry(0.,axis,normal,axial,axial,.16,'side');g.validate()
    e=profile.profile_e(profile.REFERENCE_Z_M+axial)
    if e is None:
        return {name:dict(collision_status='not_evaluated:out_of_curve',ev_ms=None,hla_deg=None,vla_deg=None) for name in MODELS}
    cm=profile.CM_Z_M-profile.REFERENCE_Z_M;inertia=profile.MASS_KG*profile.GYRATION_M**2
    inv,arm,_=inverse_mass(axis,normal,axial,lever,'free',profile.MASS_KG,profile.GYRATION_M,cm)
    base=normal_solve(g,vin,vbat,inv,e)
    base['spin_rpm']=norm(spin)*30/math.pi if base['ev_ms'] is not None else None
    check_metrics(checks,'normal_',recoil_checks(base,vin,vbat,omega,normal,arm,inertia,'free',profile.MASS_KG,e))
    results={MODELS[0]:base}
    for name,mu,et in [(MODELS[1],0.,0.),(MODELS[2],.2,.4)]:
        m=finite_solve(g,vin,vbat,spin,omega,profile.MASS_KG,inertia,.00015,cm,.07-BALL_RADIUS_M,e,mu,et)
        check_metrics(checks,'finite_',m.get('checks',{}));results[name]=m
    if base['collision_status']==results[MODELS[1]]['collision_status']=='impulse_contact':
        check_metrics(checks,'control_',dict(outgoing_ms=norm(sub(base['outgoing_world_ms'],results[MODELS[1]]['outgoing_world_ms']))))
    elif base['collision_status']!=results[MODELS[1]]['collision_status']:
        raise ValueError('normal_control_status_mismatch')
    return results


def run(output,tilts=(-45.,-30.,0.,30.,45.),axials=(-.1,0.,.1),samples=2001,b_limit=.999,
        pitch_speed=38.,bat_speed=35.,attack_deg=5.,lever=.9,spin_rpm=2200.,spin_angle=0.,spin_efficiency=.9,gyro_sign=0):
    scalars=(b_limit,pitch_speed,bat_speed,attack_deg,lever,spin_rpm,spin_angle,spin_efficiency)
    if (not all(math.isfinite(x) for x in scalars) or not isinstance(samples,int) or samples<3 or samples%2==0
        or not 0<b_limit<1 or pitch_speed<=0 or bat_speed<0 or abs(attack_deg)>=90 or lever<=.16
        or not tilts or len(set(tilts))!=len(tilts) or any(not math.isfinite(x) or abs(x)>=90 for x in tilts)
        or not axials or len(set(axials))!=len(axials) or any(not math.isfinite(x) or abs(x)>.16 for x in axials)):
        raise ValueError('invalid_sweep_parameters')
    vin=(0.,-pitch_speed,0.);attack=math.radians(attack_deg)
    vbat=(0.,bat_speed*math.cos(attack),bat_speed*math.sin(attack))
    spin=reconstruct(spin_rpm,spin_angle,spin_efficiency,vin,gyro_sign)
    output=Path(output);output.mkdir(parents=True,exist_ok=False)
    checks=defaultdict(float);groups=defaultdict(Counter);minima={};intervals=[];count=0
    with (output/'impact_sweep_samples.csv').open('x',newline='') as f:
        writer=csv.DictWriter(f,fieldnames=FIELDS);writer.writeheader()
        for theta,s in product(tilts,axials):
            angle=math.radians(theta);axis=(math.cos(angle),0.,math.sin(angle))
            omega=mul(cross(axis,vbat),1/lever)
            open_intervals={}
            previous_b=None
            for i in range(samples):
                b=-b_limit+2*b_limit*i/(samples-1);n=normal_for_b(axis,sub(vin,vbat),b)
                reconstructed=local_impact(axis,n,sub(vin,vbat),'side')
                check_metrics(checks,'geometry_',dict(b_reconstruction=abs(b-reconstructed),axis_normal_dot=abs(dot(axis,n)),normal_unit=abs(norm(n)-1)))
                for model,m in responses(axis,n,s,vin,vbat,omega,spin,lever,checks).items():
                    d=direction(m);key=(theta,s,model);g=groups[key];g['grid_samples']+=1
                    successful=m['collision_status']=='impulse_contact';fair=d=='fair_direction'
                    weak=successful and m['ev_ms']<25;weak_fair=weak and fair
                    g['successful_samples']+=successful;g['fair_samples']+=fair
                    g['weak_samples']+=weak;g['weak_fair_samples']+=weak_fair
                    out=m.get('outgoing_world_ms');row=dict(zip(KEYS,key),b_over_r=b,
                        normal_x=n[0],normal_y=n[1],normal_z=n[2],collision_status=m['collision_status'],direction=d,
                        ev_ms=m.get('ev_ms'),hla_deg=m.get('hla_deg'),vla_deg=m.get('vla_deg'),
                        outgoing_x_ms=out[0] if out else None,outgoing_y_ms=out[1] if out else None,
                        outgoing_z_ms=out[2] if out else None,spin_rpm=m.get('spin_rpm'))
                    writer.writerow(row);count+=1
                    if successful:
                        for cohort,include in [('all_successful',True),('fair_direction',fair)]:
                            mk=key+(cohort,)
                            if include and (mk not in minima or m['ev_ms']<minima[mk]['ev_ms']):minima[mk]=dict(row,cohort=cohort)
                    if weak_fair:
                        if model not in open_intervals:open_intervals[model]=(b,0)
                        first,npoints=open_intervals[model];open_intervals[model]=(first,npoints+1)
                    elif model in open_intervals:
                        first,npoints=open_intervals.pop(model)
                        intervals.append(dict(zip(KEYS,key),first_sample_b=first,last_sample_b=previous_b,grid_samples=npoints))
                previous_b=b
            for model,(first,npoints) in open_intervals.items():
                intervals.append(dict(tilt_deg=theta,axial_m=s,model=model,first_sample_b=first,last_sample_b=previous_b,grid_samples=npoints))
    rows=[]
    counts=['grid_samples','successful_samples','fair_samples','weak_samples','weak_fair_samples']
    for key,g in sorted(groups.items()):
        r=dict(zip(KEYS,key));r.update({k:g[k] for k in counts})
        for cohort,prefix in [('all_successful','all'),('fair_direction','fair')]:
            m=minima.get(key+(cohort,));r[prefix+'_min_ev_ms']=m['ev_ms'] if m else None
            r[prefix+'_min_ev_b_over_r']=m['b_over_r'] if m else None
        rows.append(r)
    write_csv(output/'impact_sweep_summary.csv',KEYS+counts+['all_min_ev_ms','all_min_ev_b_over_r','fair_min_ev_ms','fair_min_ev_b_over_r'],rows)
    write_csv(output/'impact_sweep_minima.csv',FIELDS+['cohort'],[minima[k] for k in sorted(minima)])
    write_csv(output/'impact_sweep_weak_fair_intervals.csv',KEYS+['first_sample_b','last_sample_b','grid_samples'],intervals)
    write_csv(output/'impact_sweep_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
    (output/'impact_sweep_metadata.json').write_text(json.dumps(dict(version=1,tilts_deg=list(tilts),axials_m=list(axials),samples_per_case=samples,
        b_limit=b_limit,b_step=2*b_limit/(samples-1),pitch_speed_ms=pitch_speed,bat_point_speed_ms=bat_speed,attack_deg=attack_deg,
        lever_m=lever,incoming_velocity_world_ms=vin,bat_point_velocity_world_ms=vbat,incoming_spin_world_rad_s=spin,
        spin_rpm=spin_rpm,spin_angle_deg=spin_angle,spin_efficiency=spin_efficiency,gyro_sign=gyro_sign,
        total_response_rows=count,models=list(MODELS),profile=profile.metadata(),
        note='Uniform signed local b/R grid, not a population or observed pitch distribution. Side contacts only; no search or flight. Approach basis uses projected incoming relative center velocity. Each axial case fixes contact-point world velocity; CM velocity is derived. Sampled minima/interval endpoints are grid approximations; no interpolation, global minimum proof, or statistical rates. R=.07m; normal-profile control is independent of finite radius solver.'),indent=2)+'\n')
    return dict(response_rows=count,cases=len(tilts)*len(axials),samples_per_case=samples)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output-dir',type=Path,required=True)
    p.add_argument('--tilts-deg',type=float,nargs='+',default=[-45,-30,0,30,45])
    p.add_argument('--axials-m',type=float,nargs='+',default=[-.1,0,.1]);p.add_argument('--samples',type=int,default=2001)
    p.add_argument('--b-limit',type=float,default=.999)
    p.add_argument('--pitch-speed-ms',type=float,default=38.);p.add_argument('--bat-speed-ms',type=float,default=35.)
    p.add_argument('--attack-deg',type=float,default=5.);p.add_argument('--lever-m',type=float,default=.9)
    p.add_argument('--spin-rpm',type=float,default=2200.);p.add_argument('--spin-angle-deg',type=float,default=0.)
    p.add_argument('--spin-efficiency',type=float,default=.9);p.add_argument('--gyro-sign',type=int,choices=(-1,0,1),default=0)
    a=p.parse_args()
    try:print(run(a.output_dir,a.tilts_deg,a.axials_m,a.samples,a.b_limit,a.pitch_speed_ms,a.bat_speed_ms,a.attack_deg,a.lever_m,a.spin_rpm,a.spin_angle_deg,a.spin_efficiency,a.gyro_sign))
    except (OSError,ValueError) as exc:p.exit(1,f'Sweep failed: {exc}\n')
