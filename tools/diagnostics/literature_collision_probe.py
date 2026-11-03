#!/usr/bin/env python3
"""Single-preset literature-formalism checks and explicitly limited trend comparisons."""
import argparse
import json
import math
from collections import defaultdict
from pathlib import Path

from bat_mass_presets import literature_wood_collision
from contact_3d import ContactGeometry
from contact_collision_probe import angles, write_csv
from finite_barrel_collision import solve
from ground_spin_probe import check_metrics
from rotating_bat_probe import add, mul, norm


def planar_reference(speed,psi_deg,en,et,preset):
    """Kensrud et al. Eq.(4),(7)-(9), stationary nonspinning baseball.

    Independent planar reference, not a measured per-impact data point.
    """
    m,r,alpha=preset['ball_mass_kg'],preset['ball_radius_m'],preset['ball_inertia_factor']
    mass,ip,iz=preset['bat_mass_kg'],preset['transverse_inertia_kg_m2'],preset['axial_inertia_kg_m2']
    b,R=preset['impact_from_cm_m'],preset['barrel_radius_m']
    psi=math.radians(psi_deg);n=(0.,math.cos(psi),math.sin(psi));t=(0.,-math.sin(psi),math.cos(psi))
    ry=m*(1/mass+b*b/ip);rx=m*alpha/(1+alpha)*(1/mass+b*b/ip+R*R/iz)
    vn=(1+en)*speed*math.cos(psi)/(1+ry)
    vt=-(1+et)*alpha*speed*math.sin(psi)/((1+alpha)*(1+rx))
    spin=-(vt)/(alpha*r)
    out=add(mul(n,vn),mul(t,vt));ev,_,theta=angles(out)
    return dict(ev_ms=ev,theta_deg=theta,spin_rpm=spin*30/math.pi,
        normal_velocity_ms=vn,tangent_velocity_ms=vt,omega_rad_s=spin,
        normal_impulse_kg_ms=m*vn,tangent_impulse_kg_ms=m*abs(vt))


def regression(xs,ys):
    if len(xs)<2:return None
    mx=sum(xs)/len(xs);my=sum(ys)/len(ys)
    denominator=sum((x-mx)**2 for x in xs)
    return sum((x-mx)*(y-my) for x,y in zip(xs,ys))/denominator if denominator>0 else None


def run(output):
    p=literature_wood_collision();output=Path(output);output.mkdir(parents=True,exist_ok=False)
    checks=defaultdict(float);points=[];groups=defaultdict(list)
    for speed in (28.,p['normalization_speed_ms'],39.):
        for et in (0.,.4,p['wood_fitted_tangent_e']):
            for mu in (.2,.5):
                for omega_z in (0.,40.):
                    for psi in range(0,31,2):
                        ref=planar_reference(speed,psi,p['normal_e_assumed'],et,p)
                        angle=math.radians(psi);n=(0.,math.cos(angle),math.sin(angle))
                        g=ContactGeometry(.4,(1.,0.,0.),n,0.,0.,.16,'side')
                        # The PDF supplies local bat speed, not the full apparatus
                        # trajectory. These omega values are explicit sensitivities.
                        m=solve(g,(0.,0.,0.),(0.,speed,0.),(0.,0.,0.),(0.,0.,omega_z),
                            p['bat_mass_kg'],p['transverse_inertia_kg_m2'],p['axial_inertia_kg_m2'],
                            -p['impact_from_cm_m'],p['barrel_radius_m'],p['normal_e_assumed'],mu,et,
                            ball_mass=p['ball_mass_kg'],ball_radius=p['ball_radius_m'],inertia_factor=p['ball_inertia_factor'])
                        check_metrics(checks,'coupled_',m.get('checks',{}))
                        # Point-contact angular balance implies vt=-alpha*r*omega.
                        check_metrics(checks,'planar_',dict(angular_relation_ms=abs(ref['tangent_velocity_ms']+
                            p['ball_inertia_factor']*p['ball_radius_m']*ref['omega_rad_s'])))
                        row=dict(speed_ms=speed,et=et,mu=mu,assumed_bat_omega_z_rad_s=omega_z,psi_deg=psi,
                            full_status=m['collision_status'],full_tangent_regime=m.get('tangent_regime'),
                            reference_kind='published_planar_equations_not_raw_observations',
                            reference_ev_ms=ref['ev_ms'],reference_theta_deg=ref['theta_deg'],reference_spin_rpm=ref['spin_rpm'])
                        scale=p['normalization_speed_ms']/speed
                        projection = {}
                        if m['collision_status']=='impulse_contact':
                            x,y,z=m['outgoing_world_ms']
                            projection=dict(ev_ms=math.hypot(y,z),theta_deg=math.degrees(math.atan2(z,y)),
                                spin_rpm=m['outgoing_spin_rad_s'][0]*30/math.pi)
                        row['full_total_ev_ms']=m.get('ev_ms')
                        row['full_total_spin_rpm']=m.get('spin_rpm')
                        row['full_out_of_plane_ms']=m['outgoing_world_ms'][0] if projection else None
                        row['full_off_axis_spin_rpm']=math.hypot(*m['outgoing_spin_rad_s'][1:])*30/math.pi if projection else None
                        for name in ('ev_ms','theta_deg','spin_rpm'):
                            value=projection.get(name)
                            row['full_'+name]=value
                            row['delta_'+name]=value-ref[name] if value is not None else None
                        row['reference_normalized_ev_ms']=ref['ev_ms']*scale
                        row['reference_normalized_spin_rpm']=ref['spin_rpm']*scale
                        row['full_normalized_ev_ms']=row['full_ev_ms']*scale if row['full_ev_ms'] is not None else None
                        row['full_normalized_spin_rpm']=row['full_spin_rpm']*scale if row['full_spin_rpm'] is not None else None
                        points.append(row);groups[speed,et,mu,omega_z].append(row)
    fields=['speed_ms','et','mu','assumed_bat_omega_z_rad_s','psi_deg','full_status','full_tangent_regime','reference_kind',
        'reference_ev_ms','reference_theta_deg','reference_spin_rpm','full_ev_ms','delta_ev_ms','full_theta_deg',
        'delta_theta_deg','full_spin_rpm','delta_spin_rpm','reference_normalized_ev_ms','reference_normalized_spin_rpm',
        'full_normalized_ev_ms','full_normalized_spin_rpm','full_total_ev_ms','full_total_spin_rpm',
        'full_out_of_plane_ms','full_off_axis_spin_rpm']
    write_csv(output/'literature_collision_points.csv',fields,points)
    rows=[]
    for key,rs in sorted(groups.items()):
        successful=[r for r in rs if r['full_status']=='impulse_contact']
        row=dict(zip(['speed_ms','et','mu','assumed_bat_omega_z_rad_s'],key),cases=len(rs),successful_cases=len(successful),
            rejected_cases=len(rs)-len(successful),coulomb_limited_cases=sum(r['full_tangent_regime']=='coulomb_limited' for r in successful))
        for kind in ('reference','full'):
            xs=[r[kind+'_theta_deg'] for r in successful]
            spin=[r[kind+'_normalized_spin_rpm'] for r in successful]
            ev=[r[kind+'_normalized_ev_ms']/.44704 for r in successful]
            row[kind+'_spin_ols_slope_rpm_per_deg']=regression(xs,spin)
            row[kind+'_ev_ols_slope_mph_per_deg']=regression(xs,ev)
        row['pooled_published_spin_slope_rpm_per_deg']=p['pooled_figure3_spin_slope_rpm_per_deg']
        row['pooled_published_ev_slope_mph_per_deg']=p['pooled_figure4_ev_slope_mph_per_deg']
        row['empirical_validation_status']='not_established:wood_specific_raw_data_and_apparatus_trajectory_missing'
        rows.append(row)
    fields=['speed_ms','et','mu','assumed_bat_omega_z_rad_s','cases','successful_cases','rejected_cases','coulomb_limited_cases',
        'reference_spin_ols_slope_rpm_per_deg','reference_ev_ols_slope_mph_per_deg',
        'full_spin_ols_slope_rpm_per_deg','full_ev_ols_slope_mph_per_deg',
        'pooled_published_spin_slope_rpm_per_deg','pooled_published_ev_slope_mph_per_deg','empirical_validation_status']
    write_csv(output/'literature_collision_summary.csv',fields,rows)
    write_csv(output/'literature_collision_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
    metadata=dict(version=1,preset=p,cases=len(points),assumptions=[
        'The complete wood bat/baseball preset comes from a single experiment; no Nathan positional e curve is applied.',
        'en=0.5 is the paper modeling assumption, not a measured normal COR for this wood bat.',
        'et=0.464 is fitted to wood-ball spin observations, so agreement with it is not independent predictive validation.',
        'Planar equations are the published formalism; the full solver also allows axial impulse and normal/axial coupling.',
        'Compare projected YZ speed/angle and signed X spin to planar references; total 3D EV/spin and off-plane terms are reported separately.',
        'Initial bat angular speed and exact apparatus pivot trajectory were not recovered. Omega_z=0/40 are assumptions; local centerline speed is supplied.',
        'Figure 3/4 slopes pool bats; no per-impact or wood-only raw series is supplied. Slope differences are exploratory, never empirical pass/fail.',
        'OLS slopes over a uniform psi grid are not the same regression design as the experimental samples.',
        'The PDF normalization multiplier can be read as the inverse ratio. Use 77mph/input_speed following the stated linear speed scaling; report this interpretation explicitly.',
        'All sphere inputs are the published mass/radius; the collision solver defaults for game diagnostics remain unchanged.'])
    (output/'literature_collision_metadata.json').write_text(json.dumps(metadata,indent=2,allow_nan=False)+'\n')
    return dict(cases=len(points),successful_cases=sum(r['full_status']=='impulse_contact' for r in points),empirical_validation='not_established')


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output-dir',type=Path,required=True);a=p.parse_args()
    try:print(run(a.output_dir))
    except (OSError,ValueError) as exc:p.exit(1,f'Literature diagnostic failed: {exc}\n')
