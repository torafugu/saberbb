"""Required restitution reduction and paired fixed-e collisions on saved events."""
from collections import Counter, defaultdict

from contact_collision_probe import write_csv
from effective_mass_probe import strata

FIXED_E=(0.,.1,.2,.3,.4,.5)
KEYS=['scope','variant','stage','band_1','band_2']


def direction(m):
    if m is None:return 'not_evaluated'
    x,y,_=m['outgoing_world_ms']
    return 'fair_direction' if y>abs(x) else 'undefined_direction' if x==y==0 else 'foul_direction'


def ev_band(m):
    if m is None:return 'not_evaluated'
    return '<25' if m['ev_ms']<25 else '25..<40' if m['ev_ms']<40 else '40+'


def upper_band(m):
    if m['weak_fair_possible'] is None:return 'not_evaluated'
    if not m['weak_fair_possible']:return 'not_possible'
    hi=m['weak_e_hi']
    if not 0<=hi<=.5:raise ValueError('invalid_weak_upper_bound')
    if hi==.5:return '0.5'
    for lo,top in ((0.,.1),(.1,.2),(.2,.3),(.3,.4),(.4,.5)):
        if hi<top:return f'{lo:g}..<{top:g}'
    raise ValueError('unclassified_weak_upper_bound')


class SensitivityAggregate:
    def __init__(self):
        self.denominators=defaultdict(Counter);self.upper=defaultdict(Counter)
        self.fixed=defaultdict(Counter);self.transitions=Counter()

    def add(self,r,m,fixed):
        if fixed and set(fixed)!=set(FIXED_E):raise ValueError('incomplete_fixed_restitution_values')
        if (m['weak_fair_possible'] is None)!= (not fixed):raise ValueError('fixed_evaluation_status_mismatch')
        before=fixed.get(.5);before_fair=direction(before)=='fair_direction'
        strong_before=before_fair and before['ev_ms']>=40
        scopes=['all_swings']+(['recorded_batting_only'] if r['adjusted_contact_type']!='UnrecordedBatting' else [])
        for scope in scopes:
            for stage,a,b in strata(r):
                key=(scope,r['variant'],stage,a,b);den=self.denominators[key]
                den['states']+=1;den['weak_possible']+=m['weak_fair_possible'] or 0
                den['baseline_fair_weak_possible']+=bool(before_fair and m['weak_fair_possible'])
                u=self.upper[key+(upper_band(m),m['weak_e_hi_open'])];u['states']+=1
                u['baseline_fair']+=before_fair;u['baseline_strong_fair']+=strong_before
                if m['weak_fair_possible']:
                    for k in ('weak_e_lo','weak_e_hi'):u[k]+=m[k]
                    u['required_reduction_infimum']+=.5-m['weak_e_hi']
                    u['lo_open']+=m['weak_e_lo_open']
                for e in FIXED_E:
                    after=fixed.get(e);g=self.fixed[key+(e,)];g['states']+=1
                    g['geometry_contacts']+=r['status']=='contact';g['impulse_contacts']+=after is not None
                    bd=direction(before);ad=direction(after)
                    g[ad]+=1;g['baseline_fair']+=before_fair;g['baseline_strong_fair']+=strong_before
                    self.transitions[key+(e,bd,ev_band(before),ad,ev_band(after))]+=1
                    if after is not None:
                        g['ev']+=after['ev_ms']
                        if after['vla_deg'] is not None:g['vla']+=after['vla_deg'];g['vla_n']+=1
                    if ad=='fair_direction':
                        g['fair_ev']+=after['ev_ms'];g['weak_fair']+=after['ev_ms']<25;g['strong_fair']+=after['ev_ms']>=40
                    if before_fair:
                        g['baseline_fair_after_fair']+=ad=='fair_direction'
                        g['baseline_fair_after_weak']+=ad=='fair_direction' and after['ev_ms']<25
                    if before_fair and ad=='fair_direction':
                        g['paired_fair']+=1;g['paired_before_ev']+=before['ev_ms'];g['paired_after_ev']+=after['ev_ms']
                    if strong_before:
                        g['strong_to_weak_fair']+=ad=='fair_direction' and after['ev_ms']<25
                        g['strong_to_medium_fair']+=ad=='fair_direction' and 25<=after['ev_ms']<40
                        g['strong_to_strong_fair']+=ad=='fair_direction' and after['ev_ms']>=40
                        g['strong_to_nonfair']+=ad!='fair_direction'

    def write(self,output):
        upper_keys=KEYS+['weak_e_hi_band','weak_e_hi_open'];rows=[]
        for key,g in sorted(self.upper.items(),key=lambda kv:tuple('' if x is None else str(x) for x in kv[0])):
            den=self.denominators[key[:5]];possible=key[5] not in ('not_possible','not_evaluated')
            r=dict(zip(upper_keys,key),states=g['states'],stratum_states=den['states'],
                stratum_weak_possible_contacts=den['weak_possible'],
                stratum_baseline_fair_weak_possible_contacts=den['baseline_fair_weak_possible'],
                baseline_fair_contacts=g['baseline_fair'],baseline_strong_fair_contacts=g['baseline_strong_fair'],
                weak_e_lo_open_contacts=g['lo_open'] if possible else None,
                pct_of_weak_possible=100*g['states']/den['weak_possible'] if possible and den['weak_possible'] else None,
                pct_of_baseline_fair_weak_possible=100*g['baseline_fair']/den['baseline_fair_weak_possible'] if possible and den['baseline_fair_weak_possible'] else None)
            for k in ('weak_e_lo','weak_e_hi','required_reduction_infimum'):r['avg_'+k]=g[k]/g['states'] if possible else None
            rows.append(r)
        upper_fields=upper_keys+['states','stratum_states','stratum_weak_possible_contacts','stratum_baseline_fair_weak_possible_contacts',
            'baseline_fair_contacts','baseline_strong_fair_contacts','weak_e_lo_open_contacts','pct_of_weak_possible',
            'pct_of_baseline_fair_weak_possible','avg_weak_e_lo','avg_weak_e_hi','avg_required_reduction_infimum']
        write_csv(output/'restitution_bounds_weak_hi.csv',upper_fields,rows)
        rows=[];fixed_keys=KEYS+['restitution_e']
        for key,g in sorted(self.fixed.items()):
            r=dict(zip(fixed_keys,key),states=g['states'],geometry_contacts=g['geometry_contacts'],impulse_contacts=g['impulse_contacts'],
                fair_contacts=g['fair_direction'],foul_contacts=g['foul_direction'],undefined_direction_contacts=g['undefined_direction'],
                not_evaluated_contacts=g['not_evaluated'],weak_fair_contacts=g['weak_fair'],strong_fair_contacts=g['strong_fair'],
                baseline_fair_contacts=g['baseline_fair'],baseline_fair_after_fair_contacts=g['baseline_fair_after_fair'],
                baseline_fair_after_weak_contacts=g['baseline_fair_after_weak'],baseline_strong_fair_contacts=g['baseline_strong_fair'],
                strong_to_weak_fair_contacts=g['strong_to_weak_fair'],strong_to_strong_fair_contacts=g['strong_to_strong_fair'],
                strong_to_medium_fair_contacts=g['strong_to_medium_fair'],
                strong_to_nonfair_contacts=g['strong_to_nonfair'],paired_fair_contacts=g['paired_fair'],defined_vla_contacts=g['vla_n'],
                avg_ev_ms=g['ev']/g['impulse_contacts'] if g['impulse_contacts'] else None,
                avg_vla_deg=g['vla']/g['vla_n'] if g['vla_n'] else None,
                avg_fair_ev_ms=g['fair_ev']/g['fair_direction'] if g['fair_direction'] else None,
                weak_pct_of_fair=100*g['weak_fair']/g['fair_direction'] if g['fair_direction'] else None,
                strong_pct_of_fair=100*g['strong_fair']/g['fair_direction'] if g['fair_direction'] else None,
                baseline_fair_to_weak_pct=100*g['baseline_fair_after_weak']/g['baseline_fair'] if g['baseline_fair'] else None,
                strong_to_weak_pct=100*g['strong_to_weak_fair']/g['baseline_strong_fair'] if g['baseline_strong_fair'] else None,
                before_paired_fair_ev_ms=g['paired_before_ev']/g['paired_fair'] if g['paired_fair'] else None,
                after_paired_fair_ev_ms=g['paired_after_ev']/g['paired_fair'] if g['paired_fair'] else None)
            rows.append(r)
        # Keys are explicit even when an input cohort is empty.
        fixed_fields=fixed_keys+['states','geometry_contacts','impulse_contacts','fair_contacts','foul_contacts','undefined_direction_contacts',
            'not_evaluated_contacts','weak_fair_contacts','strong_fair_contacts','baseline_fair_contacts','baseline_fair_after_fair_contacts',
            'baseline_fair_after_weak_contacts','baseline_strong_fair_contacts','strong_to_weak_fair_contacts','strong_to_strong_fair_contacts',
            'strong_to_medium_fair_contacts',
            'strong_to_nonfair_contacts','paired_fair_contacts','defined_vla_contacts','avg_ev_ms','avg_vla_deg','avg_fair_ev_ms','weak_pct_of_fair',
            'strong_pct_of_fair','baseline_fair_to_weak_pct','strong_to_weak_pct','before_paired_fair_ev_ms','after_paired_fair_ev_ms']
        write_csv(output/'restitution_bounds_fixed.csv',fixed_fields,rows)
        keys=fixed_keys+['before_direction','before_ev_band','after_direction','after_ev_band']
        write_csv(output/'restitution_bounds_fixed_transitions.csv',keys+['states'],
            [dict(zip(keys,key),states=n) for key,n in sorted(self.transitions.items())])
