"""Observed contact speed / signed axial-location census and model outcomes."""
import math
from collections import Counter, defaultdict

import bat_vibration_profile as profile
from contact_3d import band
from contact_collision_probe import write_csv
from contact_selection_audit import geometry_reason, region_flags
from effective_mass_probe import direction
from forecast_contact_geometry import vec
from ground_spin_probe import check_metrics
from rotating_bat_probe import norm

SPEED_EDGES=(20,25,30,35,40,45)
AXIAL_EDGES=(-.12,-.08,-.04,0,.04,.08,.12)
B_EDGES=(-.95,-.75,-.5,-.25,0,.25,.5,.75,.95)
TILT_EDGES=(-60,-45,-30,-15,0,15,30,45,60,75)
KEYS=['scope','variant','contact_region','stratum','speed_band_ms','axial_band_m','b_over_r_band','tilt_band_deg']
METRICS=['bat_point_speed_ms','closest_axial_m','source_z_m','signed_b_over_r',
         'signed_tilt_deg','pitch_speed_ms','point_attack_deg','normal_z']
OUTCOMES=['ev_ms','vla_deg']


def contact_values(r,checks):
    v,a=vec(r,'point_velocity_'),vec(r,'axis_at_entry_')
    speed=norm(v);axial=r['closest_axial_at_entry_m']
    check_metrics(checks,'',dict(saved_speed_ms=abs(speed-r['bat_point_speed_ms']),
        source_coordinate_m=abs(profile.REFERENCE_Z_M+axial-r['_source_z_m'])))
    values=dict(bat_point_speed_ms=speed,closest_axial_m=axial,source_z_m=r['_source_z_m'],
        signed_b_over_r=r.get('local_signed_b_over_r'),
        signed_tilt_deg=math.degrees(math.atan2(a[2],math.hypot(a[0],a[1]))),
        pitch_speed_ms=norm(vec(r,'ball_velocity_')),
        point_attack_deg=math.degrees(math.atan2(v[2],math.hypot(v[0],v[1]))) if speed else None,
        normal_z=float(r['normal_z']))
    if any(v is not None and not math.isfinite(v) for v in values.values()):
        raise ValueError('nonfinite_speed_axial_metric')
    return values


def strata(v):
    speed=band(v['bat_point_speed_ms'],SPEED_EDGES);axial=band(v['closest_axial_m'],AXIAL_EDGES)
    yield 'overall','all','all','all','all'
    yield 'speed_axial',speed,axial,'all','all'
    yield 'speed_axial_impact',speed,axial,band(v['signed_b_over_r'],B_EDGES),'all'
    yield 'speed_axial_tilt',speed,axial,'all',band(v['signed_tilt_deg'],TILT_EDGES)


def add_values(g,values):
    for k,v in values.items():
        if v is not None:
            g['n_'+k]+=1;g['sum_'+k]+=v
            if g['n_'+k]==1:g['min_'+k]=v;g['max_'+k]=v
            else:g['min_'+k]=min(g['min_'+k],v);g['max_'+k]=max(g['max_'+k],v)


class SpeedAxialAudit:
    def __init__(self):
        self.geometry=defaultdict(Counter);self.outcomes=defaultdict(Counter)
        self.coverage=Counter();self.checks=defaultdict(float)
    def add(self,r,results):
        scopes=['all_swings']+(['recorded_batting_only'] if r['adjusted_contact_type']!='UnrecordedBatting' else [])
        reason=geometry_reason(r)
        for scope in scopes:
            for name,m in results.items():
                self.coverage[scope,r['variant'],name,r['status'],reason,m['collision_status']]+=1
        if r['status']!='contact':return
        values=contact_values(r,self.checks)
        for scope in scopes:
            for s,speed,axial,b,tilt in strata(values):
                key=(scope,r['variant'],r['contact_region'],s,speed,axial,b,tilt)
                g=self.geometry[key+(reason,)];g['contacts']+=1;add_values(g,values)
                bases=[('source_constant_mass_normal',r)]+list(results.items())
                for basis,m in bases:
                    g=self.outcomes[key+(basis,m['collision_status'],direction(m))];g['contacts']+=1
                    if m['collision_status']!='impulse_contact':continue
                    add_values(g,{k:m[k] for k in OUTCOMES})
                    for k,flag in region_flags(m).items():
                        if flag is not None:g['n_'+k]+=1;g[k]+=flag
    def write(self,output):
        def records(groups,keys,metrics,regions=False):
            rows=[]
            for key,g in sorted(groups.items()):
                row=dict(zip(keys,key),contacts=g['contacts'])
                for k in metrics:
                    n=g['n_'+k];row['n_'+k]=n
                    row['avg_'+k]=g['sum_'+k]/n if n else None
                    row['min_'+k]=g['min_'+k] if n else None;row['max_'+k]=g['max_'+k] if n else None
                if regions:
                    for k in region_flags({}):
                        n=g['n_'+k];row['n_'+k]=n;row[k+'_contacts']=g[k]
                        row[k+'_fraction']=g[k]/n if n else None
                rows.append(row)
            fields=keys+['contacts']+[p+k for k in metrics for p in ('n_','avg_','min_','max_')]
            if regions:fields += [v for k in region_flags({}) for v in ('n_'+k,k+'_contacts',k+'_fraction')]
            return fields,rows
        fields,rows=records(self.geometry,KEYS+['geometry_reason'],METRICS)
        write_csv(output/'finite_barrel_speed_axial_geometry.csv',fields,rows)
        fields,rows=records(self.outcomes,KEYS+['outcome_basis','collision_status','direction'],OUTCOMES,True)
        write_csv(output/'finite_barrel_speed_axial_outcomes.csv',fields,rows)
        keys=['scope','variant','model','search_status','geometry_reason','model_status']
        write_csv(output/'finite_barrel_speed_axial_coverage.csv',keys+['trial_rows'],[
            dict(zip(keys,k),trial_rows=n) for k,n in sorted(self.coverage.items())])
        write_csv(output/'finite_barrel_speed_axial_checks.csv',['metric','max_error'],[
            dict(metric=k,max_error=v) for k,v in sorted(self.checks.items())])
