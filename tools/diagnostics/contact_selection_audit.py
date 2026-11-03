"""Selection audit using unchanged source outcomes across nested cohorts."""
import bisect
import math
from collections import Counter, defaultdict

import bat_vibration_profile as profile
from contact_collision_probe import write_csv
from effective_mass_probe import direction
from spin_distributions import EV_EDGES, VLA_EDGES, bounds

KEYS = ['scope','variant','model','stage','outcome_basis']
REGIONS = ['ev_lt25','ev_ge45','vla_lt0','vla_ge50','fast_downward']
PREDICTORS = ['source_z_m','bat_point_speed_ms','normal_z','normal_speed_fraction','local_abs_b_over_r']


def geometry_reason(r):
    if r['status'] != 'contact':return 'not_evaluated:'+r['status']
    if r['collision_status'] != 'impulse_contact':return 'not_evaluated:non_approaching'
    if r['contact_region'] != 'side':return 'not_evaluated:unsupported_cap'
    if profile.profile_e(r['_source_z_m']) is None:return 'not_evaluated:out_of_curve'
    return 'eligible'


def region_flags(m):
    ev,vla=m.get('ev_ms'),m.get('vla_deg')
    return dict(ev_lt25=None if ev is None else ev<25,
        ev_ge45=None if ev is None else ev>=45,
        vla_lt0=None if vla is None else vla<0,
        vla_ge50=None if vla is None else vla>=50,
        fast_downward=None if ev is None or vla is None else ev>=40 and vla<0)


class SelectionAudit:
    def __init__(self):
        self.groups=defaultdict(Counter);self.hist=Counter();self.coverage=Counter()
        self.predictors=defaultdict(Counter)

    def record(self,key,r,m):
        g=self.groups[key];g['contacts']+=1
        values={k:m.get(k) for k in ('ev_ms','vla_deg')}
        for k,v in values.items():
            if v is not None:
                if not math.isfinite(v):raise ValueError('nonfinite_selection_outcome')
                g['n_'+k]+=1;g['sum_'+k]+=v
        if all(v is not None for v in values.values()):
            g['joint_contacts']+=1
            self.hist[key+(bisect.bisect_right(EV_EDGES,values['ev_ms']),
                            bisect.bisect_right(VLA_EDGES,values['vla_deg']))]+=1
        flags=region_flags(m)
        for label,flag in flags.items():
            if flag is not None:g['n_'+label]+=1;g[label]+=flag
        predictors=dict(source_z_m=r.get('_source_z_m'),normal_z=r.get('normal_z'),
            **{k:r.get(k) for k in PREDICTORS if k not in ('source_z_m','normal_z')})
        # Source rows carry normal_z as the z component of the contact normal.
        for region in ['all']+[k for k,v in flags.items() if v]:
            p=self.predictors[key+(region,)];p['contacts']+=1
            for label,value in predictors.items():
                if value is not None:
                    if not math.isfinite(value):raise ValueError('nonfinite_selection_predictor')
                    p['n_'+label]+=1;p['sum_'+label]+=value

    def add(self,r,results):
        scopes=['all_swings']+(['recorded_batting_only'] if r['adjusted_contact_type']!='UnrecordedBatting' else [])
        reason=geometry_reason(r)
        for scope in scopes:
            for model,m in results.items():
                self.coverage[scope,r['variant'],model,reason,m['collision_status']]+=1
                if r['status']!='contact':continue
                stages=['all_geometry_contacts','reason:'+reason]
                if reason=='eligible':stages.append('geometry_eligible')
                if m['collision_status']=='impulse_contact':
                    stages.append('model_success')
                    if direction(m)=='fair_direction':stages.append('model_fair')
                for stage in stages:
                    # Same source prediction before and after selection isolates cohort effects.
                    self.record((scope,r['variant'],model,stage,'source_constant_mass_normal'),r,r)
                    if m['collision_status']=='impulse_contact':
                        self.record((scope,r['variant'],model,stage,'input_spin_model'),r,m)

    def write(self,output):
        summary=[];hist=[];predictors=[]
        for key,g in sorted(self.groups.items()):
            row=dict(zip(KEYS,key),contacts=g['contacts'],joint_contacts=g['joint_contacts'],
                missing_joint_contacts=g['contacts']-g['joint_contacts'])
            for k in ('ev_ms','vla_deg'):
                n=g['n_'+k];row['n_'+k]=n;row['avg_'+k]=g['sum_'+k]/n if n else None
            for k in REGIONS:
                n=g['n_'+k];row['n_'+k]=n;row[k+'_contacts']=g[k];row[k+'_fraction']=g[k]/n if n else None
            summary.append(row)
        for key,count in sorted(self.hist.items()):
            group,ei,vi=key[:5],key[5],key[6];n=self.groups[group]['joint_contacts']
            elo,ehi=bounds(EV_EDGES,ei);vlo,vhi=bounds(VLA_EDGES,vi)
            hist.append(dict(zip(KEYS,group),ev_bin=ei,vla_bin=vi,ev_lower_inclusive=elo,
                ev_upper_exclusive=ehi,vla_lower_inclusive=vlo,vla_upper_exclusive=vhi,
                count=count,joint_contacts=n,fraction=count/n))
        for key,g in sorted(self.predictors.items()):
            row=dict(zip(KEYS+['region'],key),contacts=g['contacts'])
            for k in PREDICTORS:
                n=g['n_'+k];row['n_'+k]=n;row['avg_'+k]=g['sum_'+k]/n if n else None
            predictors.append(row)
        write_csv(output/'finite_barrel_selection_summary.csv',KEYS+['contacts','joint_contacts','missing_joint_contacts']+
            [p+k for k in ('ev_ms','vla_deg') for p in ('n_','avg_')]+
            [v for k in REGIONS for v in ('n_'+k,k+'_contacts',k+'_fraction')],summary)
        write_csv(output/'finite_barrel_selection_ev_vla.csv',KEYS+['ev_bin','vla_bin','ev_lower_inclusive',
            'ev_upper_exclusive','vla_lower_inclusive','vla_upper_exclusive','count','joint_contacts','fraction'],hist)
        write_csv(output/'finite_barrel_selection_predictors.csv',KEYS+['region','contacts']+
            [p+k for k in PREDICTORS for p in ('n_','avg_')],predictors)
        keys=['scope','variant','model','geometry_reason','model_status']
        write_csv(output/'finite_barrel_selection_coverage.csv',keys+['trial_rows'],
            [dict(zip(keys,k),trial_rows=n) for k,n in sorted(self.coverage.items())])
