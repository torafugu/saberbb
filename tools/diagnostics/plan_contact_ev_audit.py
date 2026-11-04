#!/usr/bin/env python3
"""Explain low-EV scarcity using cached contact geometry and impulses only."""
import argparse
import json
import math
from collections import Counter,defaultdict
from pathlib import Path

from bat_axis_origin_audit import check
from contact_3d import ContactGeometry,band
from contact_collision_probe import BALL_MASS,angles,dot,write_csv
from effective_mass_probe import direction
from execution_contact_posture import posture
from forecast_contact_geometry import vec
from plan_tilt_direction_audit import Audit as DirectionAudit,load_inputs,number,vector
from plan_tilt_sweep import KINEMATICS
from rotating_bat_probe import add,mul,norm,sub

PREFIX='plan_contact_ev'
KEYS=['dataset','scenario','model','stratum','band_1','band_2']
NORMAL_EDGES=(.25,.5,.75,.9,.98)
AXIAL_EDGES=(.25,.5,.75,.9,1.)
SPEED_EDGES=(20,25,30,35,40)
EV_EDGES=(0,10,20,25,30,35,40,45,50,55,60)
METRICS=('relative_speed_ms','approach_speed_ms','normal_speed_fraction','bat_point_speed_ms',
    'signed_axial_fraction','local_signed_b_over_r','normal_z','normal_elevation_deg',
    'ev_ms','vla_deg','normal_only_ev_ms','tangential_delta_ev_ms',
    'normal_impulse_kg_ms','tangent_impulse_kg_ms','ball_rotation_change_j','total_loss_j')
COUNTS=('contacts','successful_contacts','fair_contacts','low_ev','high_ev','downward','high_vla')


def geometry(row,checks):
    """Use the saved centerline velocity convention; never invoke a solver."""
    n=vector(vec(row,'normal_'));a=vector(vec(row,'axis_at_entry_'))
    half=number(row['half_length_m'])
    ContactGeometry(number(row['entry_since_release_sec']),a,n,number(row['axial_at_entry_m']),
                    number(row['closest_axial_at_entry_m']),half,row['contact_region']).validate()
    radius=number(row['effective_radius_m']);gap=number(row['min_evaluated_clearance_m'])
    if radius<=0 or radius+gap<=0:raise ValueError('invalid_entry_radius')
    vin=vector(vec(row,'ball_velocity_'));bat=vector(vec(row,'point_velocity_'));rel=sub(vin,bat)
    speed=norm(rel);approach=max(0.,-dot(rel,n));fraction=approach/speed if speed else None
    if fraction is not None and fraction>1+1e-8:raise ValueError('invalid_normal_speed_fraction')
    values=dict(relative_speed_ms=speed,approach_speed_ms=approach,
        normal_speed_fraction=min(1.,fraction) if fraction is not None else None,
        bat_point_speed_ms=norm(bat),signed_axial_fraction=row['closest_axial_at_entry_m']/half)
    for field in ('relative_speed_ms','approach_speed_ms','normal_speed_fraction','bat_point_speed_ms'):
        if values[field] is not None:check(checks,'contact_'+field,abs(number(row[field])-values[field]),1e-7)
    return values


def strata(values,plan):
    v=values or {};normal=band(v.get('normal_speed_fraction'),NORMAL_EDGES)
    axial=band(abs(v['signed_axial_fraction']) if v.get('signed_axial_fraction') is not None else None,AXIAL_EDGES)
    speed=band(v.get('bat_point_speed_ms'),SPEED_EDGES)
    yield 'overall','all','all'
    yield 'normal_fraction',normal,'all'
    yield 'axial_fraction',axial,'all'
    yield 'point_speed',speed,'all'
    yield 'normal_axial',normal,axial
    yield 'normal_point_speed',normal,speed
    yield 'forecast_height',band(vector(plan['forecast_position_world_m'])[2],(0,.3,.6,.9,1.2,1.5)),'all'
    yield 'handedness','right' if plan['ideal_axis_world'][0]>0 else 'left','all'


class Audit(DirectionAudit):
    def __init__(self):
        super().__init__()
        self.ev_summary=defaultdict(Counter);self.ev_selection=Counter()
        self.ev_histogram=Counter();self.model_pairs=defaultdict(Counter)

    def add(self,dataset,records,models,kinematic_fields=KINEMATICS):
        # Retain full source-count, vector, histogram and kinematic reconciliation.
        # DirectionAudit uses ev_hist for its own source histogram validation.
        super().add(dataset,records,models,kinematic_fields)
        for name,r in records.items():
            invalid=r.get('invalid_reason');row=r.get('contact');plan=r['forecast_plan']
            status=invalid or r['search_status'];contact=status=='contact'
            base=geometry(row,self.checks) if contact else None
            stages=list(strata(base,plan))
            responses=r.get('responses',{}) if not invalid else {}
            for model in models:
                m=responses.get(model,dict(collision_status='not_evaluated:'+status,ev_ms=None))
                success=m['collision_status']=='impulse_contact';fair=direction(m)=='fair_direction'
                region=row['contact_region'] if contact else 'no_entry'
                for stage,b1,b2 in stages:
                    key=(dataset,name,model,stage,b1,b2)
                    self.ev_selection[key+(status,region,m['collision_status'],direction(m))]+=1
                if not contact:continue
                values={**base,**posture(row,m,self.checks)}
                if success:
                    n=vec(row,'normal_');vin=vec(row,'ball_velocity_')
                    jn=vector(m['normal_impulse_world_kg_ms']) if 'normal_impulse_world_kg_ms' in m else mul(n,number(m['normal_impulse_kg_ms']))
                    normal_ev=norm(add(vin,mul(jn,1/BALL_MASS)))
                    values.update(normal_only_ev_ms=normal_ev,tangential_delta_ev_ms=m['ev_ms']-normal_ev,
                                  normal_impulse_kg_ms=number(m['normal_impulse_kg_ms']))
                    for field in ('tangent_impulse_kg_ms','total_loss_j'):
                        if field in m:values[field]=number(m[field])
                    if 'ball_rotation_after_j' in m:
                        values['ball_rotation_change_j']=number(m['ball_rotation_after_j'])-number(m['ball_rotation_before_j'])
                    if 'system_before_j' in m:
                        check(self.checks,'cached_energy_balance_j',abs(number(m['system_after_j'])-number(m['system_before_j'])+number(m['total_loss_j'])),1e-7)
                for stage,b1,b2 in stages:
                    key=(dataset,name,model,stage,b1,b2)
                    for cohort in ('geometry_contacts','all_successful','fair_direction'):
                        if cohort=='all_successful' and not success or cohort=='fair_direction' and not fair:continue
                        g=self.ev_summary[key+(cohort,)];g['contacts']+=1;g['successful_contacts']+=success;g['fair_contacts']+=fair
                        if success:
                            for label,yes in [('low_ev',m['ev_ms']<25),('high_ev',m['ev_ms']>=45),('downward',m['vla_deg']<0),('high_vla',m['vla_deg']>=50)]:g[label]+=yes
                        for field in METRICS:
                            if values.get(field) is not None:g[field]+=number(values[field]);g['n_'+field]+=1
                    if success:
                        for cohort in ('all_successful','fair_direction'):
                            if cohort=='fair_direction' and not fair:continue
                            self.ev_histogram[key+(cohort,band(m['ev_ms'],EV_EDGES))]+=1
            # Compare models on exactly the same cached contact, with explicit cohort.
            before=responses.get('normal_profile');after=responses.get('finite_I0.00015_et0.4')
            if not before or not after or any(m['collision_status']!='impulse_contact' for m in (before,after)):continue
            for stage,b1,b2 in stages:
                for cohort in ('common_successful','common_fair'):
                    if cohort=='common_fair' and not all(direction(m)=='fair_direction' for m in (before,after)):continue
                    g=self.model_pairs[dataset,name,stage,b1,b2,cohort];g['contacts']+=1
                    g['to_low_ev']+=before['ev_ms']>=25 and after['ev_ms']<25
                    g['from_low_ev']+=before['ev_ms']<25 and after['ev_ms']>=25
                    for label,m in [('before',before),('after',after)]:
                        g[label+'_ev_ms']+=m['ev_ms'];g[label+'_low_ev']+=m['ev_ms']<25;g[label+'_fair']+=direction(m)=='fair_direction'

    def write(self,out):
        rows=[]
        for key,g in sorted(self.ev_summary.items()):
            r=dict(zip(KEYS+['cohort'],key),**{f:g[f] for f in COUNTS})
            for field in METRICS:
                n=g['n_'+field];r['n_'+field]=n;r['avg_'+field]=g[field]/n if n else None
            rows.append(r)
        write_csv(out/(PREFIX+'_summary.csv'),KEYS+['cohort']+list(COUNTS)+[p+f for f in METRICS for p in ('n_','avg_')],rows)
        keys=KEYS+['search_status','contact_region','collision_status','direction']
        write_csv(out/(PREFIX+'_selection.csv'),keys+['trials'],[dict(zip(keys,k),trials=n) for k,n in sorted(self.ev_selection.items())])
        rows=[]
        for key,n in sorted(self.ev_histogram.items()):
            total=self.ev_summary[key[:-1]]['contacts']
            rows.append(dict(zip(KEYS+['cohort','ev_band_ms'],key),contacts=n,cohort_contacts=total,fraction=n/total))
        write_csv(out/(PREFIX+'_histogram.csv'),KEYS+['cohort','ev_band_ms','contacts','cohort_contacts','fraction'],rows)
        rows=[];keys=['dataset','scenario','stratum','band_1','band_2','cohort']
        for key,g in sorted(self.model_pairs.items()):
            n=g['contacts'];rows.append(dict(zip(keys,key),before_model='normal_profile',after_model='finite_I0.00015_et0.4',
                contacts=n,to_low_ev=g['to_low_ev'],from_low_ev=g['from_low_ev'],
                **{s+'_'+f:g[s+'_'+f] for s in ('before','after') for f in ('low_ev','fair')},
                avg_before_ev_ms=g['before_ev_ms']/n,avg_after_ev_ms=g['after_ev_ms']/n,
                avg_delta_ev_ms=(g['after_ev_ms']-g['before_ev_ms'])/n))
        fields=['before_model','after_model','contacts','to_low_ev','from_low_ev','before_low_ev','before_fair','after_low_ev','after_fair','avg_before_ev_ms','avg_after_ev_ms','avg_delta_ev_ms']
        write_csv(out/(PREFIX+'_pairs.csv'),keys+fields,rows)


def run(input_dirs,output):
    audit=Audit()
    datasets,inputs=load_inputs(input_dirs,audit)
    out=Path(output);out.mkdir(parents=True,exist_ok=False);audit.write(out)
    write_csv(out/(PREFIX+'_checks.csv'),['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(audit.checks.items())])
    metadata=dict(version=1,status='cached_contact_ev_audit_not_calibrated',new_searches=0,db_reads=0,collision_replays=0,
        datasets=datasets,inputs=inputs,normal_fraction_edges=NORMAL_EDGES,axial_fraction_edges=AXIAL_EDGES,
        point_speed_edges_ms=SPEED_EDGES,ev_edges_ms=EV_EDGES,
        conventions=dict(relative_velocity='saved ball center velocity minus saved bat centerline point velocity',
            normal_fraction='max(0,-dot(relative_velocity,bat_to_ball_normal))/norm(relative_velocity)',
            axial_fraction='closest axial coordinate / evaluated bat segment half length; not distance from sweet spot',
            local_impact='signed instantaneous cylinder-plane b/R; undefined for caps',
            normal_only_ev='norm(v_in + saved J_normal / ball_mass); holds this model normal impulse fixed'),
        limitations=['Only all_swings. Each seed remains separate; no pooling of overlapping samples.',
            'Each trial appears in every stratum and model. Sum partitions within one stratum only.',
            'Geometry cohorts include exclusions; EV means/counts use successful responses only. Missing metrics are blank with n=0.',
            'No-contact and clock-invalid trials have undefined contact geometry, never zero-valued speed or axial position.',
            'Centerline relative speed is not ball/bat surface slip; spin and finite-radius correction are not included in this fraction.',
            'Normal-only EV is an impulse decomposition, not a separately solved frictionless counterfactual.',
            'Energy/spin metrics exist only where cached. Rotation change can be negative; total loss is whole-system dissipation.',
            'Cache consistency is checked; first-contact search accuracy is inherited, not independently verified.',
            'No DB, contact search, collision solve, Statcast fitting, or production changes.'])
    (out/(PREFIX+'_metadata.json')).write_text(json.dumps(metadata,indent=2)+'\n')
    return dict(datasets=len(datasets),new_searches=0,db_reads=0,collision_replays=0)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-dir',type=Path,nargs='+',required=True);p.add_argument('--output-dir',type=Path,required=True)
    a=p.parse_args()
    try:print(json.dumps(run(a.input_dir,a.output_dir),indent=2))
    except (OSError,ValueError,KeyError,TypeError,ZeroDivisionError) as exc:p.exit(1,f'Cached contact EV audit failed: {exc}\n')
