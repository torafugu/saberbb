#!/usr/bin/env python3
"""Decompose a fixed mixture and paired aim changes from validated cached pitches."""
import argparse
from collections import Counter,defaultdict
from itertools import combinations
import json
import math
from pathlib import Path

from bat_axis_origin_audit import check
from contact_3d import band
from contact_collision_probe import write_csv
from effective_mass_probe import direction
from execution_contact_posture import digest,posture
from plan_contact_ev_audit import geometry
from plan_tilt_direction_audit import Audit,HEIGHT_EDGES
import plan_speed_mixture_compare as planned

engine=planned.engine
PREFIX='plan_high_vla'
COUNTS=('trials','evaluable','clock_invalid','geometry','fair','high_vla','low_ev','high_ev','downward','vla_0to30','ev_40to45')
REGIONS=('high_vla','low_ev','high_ev','downward','vla_0to30','ev_40to45')
KEYS=('scenario','stratum','band_1','band_2')


def facts(record,model,checks):
    valid=record.get('invalid_reason') is None
    contact=valid and record['search_status']=='contact'
    m=record['responses'][model] if valid else None
    fair=valid and direction(m)=='fair_direction'
    values=posture(record['contact'],m,checks) if contact else {}
    if contact:geometry(record['contact'],checks)
    high=fair and m['vla_deg']>=50
    return dict(evaluable=valid,clock_invalid=not valid,geometry=contact,fair=fair,high_vla=high,
        low_ev=fair and m['ev_ms']<25,high_ev=fair and m['ev_ms']>=45,
        downward=fair and m['vla_deg']<0,vla_0to30=fair and 0<=m['vla_deg']<30,
        ev_40to45=fair and 40<=m['ev_ms']<45,
        ev_ms=m['ev_ms'] if fair else None,vla_deg=m['vla_deg'] if fair else None,
        height=band(record['forecast_plan']['forecast_position_world_m'][2],HEIGHT_EDGES),
        normal=band(values.get('normal_elevation_deg'),(-60,-30,-10,0,10,30,60)),
        impact=band(values.get('local_signed_b_over_r'),(-.95,-.75,-.25,0,.25,.75,.95)),
        axial=band(abs(values['signed_axial_fraction']) if values else None,(.25,.5,.75,.9,1.)),
        region=record['contact']['contact_region'] if contact else 'no_entry',
        outcome='clock_invalid' if not valid else 'no_geometry_contact' if not contact else
            'fair_high_vla' if high else 'fair_other' if fair else
            'successful_nonfair' if m['collision_status']=='impulse_contact' else 'collision_not_successful')


def strata(f):
    yield 'overall','all','all'
    yield 'forecast_height',f['height'],'all'
    yield 'normal_elevation',f['normal'],'all'
    yield 'signed_impact',f['impact'],'all'
    yield 'abs_axial_fraction',f['axial'],'all'
    yield 'contact_region',f['region'],'all'
    yield 'height_normal',f['height'],f['normal']


def add_counts(g,f):
    g['trials']+=1
    for field in COUNTS[1:]:g[field]+=f[field]


class ContributionAudit(Audit):
    def __init__(self,model):
        super().__init__();self.model=model;self.raw=defaultdict(Counter);self.pitches={}

    def add(self,dataset,records,models,kinematic_fields):
        super().add(dataset,records,models,kinematic_fields)
        if self.model not in models:raise ValueError('matching_model_required')
        base=records['saved_tilt'];key=(base['game_id'],base['count_seq'])
        if key in self.pitches:raise ValueError('duplicate_contribution_pitch')
        compact={}
        for name,r in records.items():
            if name=='saved_tilt':continue
            f=facts(r,self.model,self.checks);compact[name]=f
            for stage,b1,b2 in strata(f):add_counts(self.raw[name,stage,b1,b2],f)
        self.pitches[key]=compact


def ratio(a,b):return a/b if b else None


def contributions(raw,states,weights,checks):
    n=next(iter(states.values()))['counts']['evaluable_trials']
    mass,prob=engine.mix(states,weights,'fair_direction')
    if prob<=0:raise ValueError('empty_mixture_fair_cohort')
    totals={field:math.fsum(w*raw[name,'overall','all','all'][field]/n for name,w in weights.items()) for field in ('geometry','fair')+REGIONS}
    check(checks,'mixture_fair_probability',abs(totals['fair']-prob),1e-7)
    predicates=dict(high_vla=lambda e,v:v>engine.VLA_EDGES.index(50),low_ev=lambda e,v:e<=engine.EV_EDGES.index(25),
        high_ev=lambda e,v:e>engine.EV_EDGES.index(45),downward=lambda e,v:v<=engine.VLA_EDGES.index(0),
        vla_0to30=lambda e,v:engine.VLA_EDGES.index(0)<v<=engine.VLA_EDGES.index(30),
        ev_40to45=lambda e,v:engine.EV_EDGES.index(40)<e<=engine.EV_EDGES.index(45))
    for field,predicate in predicates.items():
        expected=math.fsum(v for (e,a),v in mass.items() if predicate(e,a))
        check(checks,'mixture_'+field+'_probability',abs(totals[field]-expected),1e-7)
    rows=[]
    for (name,stage,b1,b2),g in sorted(raw.items()):
        if name not in weights:continue
        w=weights[name];c=states[name]['config']
        r=dict(zip(KEYS,(name,stage,b1,b2)),planned_speed_ratio=c['planned_speed_ratio'],aim_z_mm=c['aim_z_offset_m']*1000,
            trial_weight=w,**{field:g[field] for field in COUNTS},
            geometry_probability_in_band=ratio(g['geometry'],g['evaluable']),fair_probability_in_band=ratio(g['fair'],g['evaluable']),
            weighted_fair_probability=w*g['fair']/n,
            fair_contribution_fraction=w*g['fair']/n/prob,
            high_vla_fraction_among_band_fair=ratio(g['high_vla'],g['fair']),
            high_vla_share_of_mixture_high=ratio(w*g['high_vla']/n,totals['high_vla']))
        for field in REGIONS:r[field+'_contribution_pp']=100*w*g[field]/n/prob
        rows.append(r)
    # Every stratum is a separate partition, never sum different strata together.
    for stage in sorted({r['stratum'] for r in rows}):
        selected=[r for r in rows if r['stratum']==stage]
        check(checks,'partition_'+stage+'_fair',abs(math.fsum(r['weighted_fair_probability'] for r in selected)-prob),1e-7)
        for field in REGIONS:
            check(checks,'partition_'+stage+'_'+field,abs(math.fsum(r[field+'_contribution_pp'] for r in selected)-100*totals[field]/prob),1e-7)
    summary=[dict(metric=field,probability_per_evaluable_trial=totals[field],
                  fraction_among_mixture_fair=totals[field]/prob if field in REGIONS else None) for field in totals]
    return summary,[r for r in rows if r['stratum']=='overall'],[r for r in rows if r['stratum']!='overall']


def pair_summary(pitches,states,weights):
    families=defaultdict(list)
    for name in weights:
        c=states[name]['config'];families[c['tilt_deg'],c['yaw_batter_deg'],c['planned_speed_ratio']].append(name)
    pairs=[];transitions=[]
    for family,names in sorted(families.items()):
        names=sorted(names,key=lambda n:states[n]['config']['aim_z_offset_m'],reverse=True)
        for before,after in combinations(names,2):
            groups=defaultdict(Counter);ts=Counter()
            for pitch in pitches.values():
                a,b=pitch[before],pitch[after]
                if a['evaluable']!=b['evaluable'] or a['height']!=b['height']:raise ValueError('unmatched_pair_population')
                for stage,height in (('overall','all'),('forecast_height',a['height'])):
                    g=groups[stage,height];g['trials']+=1;g['evaluable']+=a['evaluable'];g['clock_invalid']+=a['clock_invalid']
                    ts[stage,height,a['outcome'],b['outcome']]+=1
                    for prefix,f in (('before',a),('after',b)):
                        for field in ('geometry','fair')+REGIONS:g[prefix+'_'+field]+=f[field]
                    g['geometry_gained']+=not a['geometry'] and b['geometry'];g['geometry_lost']+=a['geometry'] and not b['geometry']
                    g['fair_gained']+=not a['fair'] and b['fair'];g['fair_lost']+=a['fair'] and not b['fair']
                    if a['fair'] and b['fair']:
                        g['common_fair']+=1;g['common_fair_high_gained']+=not a['high_vla'] and b['high_vla']
                        g['common_fair_high_lost']+=a['high_vla'] and not b['high_vla']
                        g['common_fair_delta_ev_sum']+=b['ev_ms']-a['ev_ms'];g['common_fair_delta_vla_sum']+=b['vla_deg']-a['vla_deg']
            identity=dict(before_scenario=before,after_scenario=after,planned_speed_ratio=family[2],
                before_aim_z_mm=states[before]['config']['aim_z_offset_m']*1000,after_aim_z_mm=states[after]['config']['aim_z_offset_m']*1000)
            for (stage,height),g in sorted(groups.items()):
                fields=('trials','evaluable','clock_invalid','common_fair','geometry_gained','geometry_lost','fair_gained','fair_lost','common_fair_high_gained','common_fair_high_lost')
                r=dict(identity,stratum=stage,forecast_height_band_m=height,**{f:g[f] for f in fields})
                for field in ('geometry','fair')+REGIONS:
                    a,b=g['before_'+field],g['after_'+field]
                    r['before_'+field]=a;r['after_'+field]=b
                    r[field+'_probability_delta_pp']=100*(b-a)/g['evaluable'] if g['evaluable'] else None
                r['before_high_vla_fraction_among_fair']=ratio(g['before_high_vla'],g['before_fair'])
                r['after_high_vla_fraction_among_fair']=ratio(g['after_high_vla'],g['after_fair'])
                r['common_fair_high_delta_pp']=ratio(100*(g['common_fair_high_gained']-g['common_fair_high_lost']),g['common_fair'])
                r['common_fair_mean_delta_ev_ms']=ratio(g['common_fair_delta_ev_sum'],g['common_fair'])
                r['common_fair_mean_delta_vla_deg']=ratio(g['common_fair_delta_vla_sum'],g['common_fair'])
                pairs.append(r)
            for (stage,height,a,b),count in sorted(ts.items()):
                transitions.append(dict(identity,stratum=stage,forecast_height_band_m=height,before_outcome=a,after_outcome=b,trials=count))
    if not pairs:raise ValueError('requires_multiple_aims_in_one_speed_geometry')
    return pairs,transitions


def run(input_dir,output,mixtures_json,model=engine.DEFAULT_MODEL):
    config=engine.read_json(mixtures_json)
    if config.get('version')!=1 or not isinstance(config.get('mixtures'),dict) or len(config['mixtures'])!=1:raise ValueError('requires_one_fixed_mixture')
    label,weights=next(iter(config['mixtures'].items()))
    if not label.strip():raise ValueError('empty_mixture_label')
    audit=ContributionAudit(model)
    states,source,paths,checks=planned.load_states(Path(input_dir),model,audit=audit)
    weights=engine.validate_weights(weights,states)
    summary,components,stratified=contributions(audit.raw,states,weights,checks)
    pairs,transitions=pair_summary(audit.pitches,states,weights)
    metadata=dict(version=1,status='fixed_mixture_descriptive_decomposition',model=model,cohort='fair_direction',
        mixture='mixture:'+label,weights=weights,source_metadata=source,
        inputs={str(p):digest(p) for p in list(paths)+[Path(mixtures_json)]},
        new_searches=0,db_reads=0,collision_replays=0,contribution_denominator='global mixture fair probability after trial weighting',
        paired_comparison='unweighted matched pitches within same tilt/yaw/speed; after aim minus before aim',
        limitations=[
            'Contributions partition simulated high-angle outcomes, not the excess relative to Statcast; observed strategic labels are unavailable.',
            'Each stratum is a separate partition. Sum contributions within one stratum only; do not sum across strata.',
            'Forecast height is available before the swing; normal/impact/axial/region are post-contact diagnostics, not permissible policy inputs.',
            'Paired marginal conditional fractions can differ through selection; common-fair changes isolate the shared fair subset only.',
            'Clock-invalid cases are not misses. No fair contacts means conditional fractions are undefined, never zero-filled.',
            'Pair rows are unweighted counterfactual diagnostics; fixed trial weights remain unchanged and no optimal policy is selected.',
            'Counterfactual effects are internal to this physical model, not estimated real-world causal effects.',
            'Same players/games, physics eligibility and fair-direction versus final-fair limitations remain. No new statistical acceptance test.'
        ])
    output=Path(output);output.mkdir(parents=True,exist_ok=False)
    for suffix,rows in [('summary',summary),('components',components),('strata',stratified),('pairs',pairs),('transitions',transitions),
                        ('checks',[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])]:
        write_csv(output/(PREFIX+'_'+suffix+'.csv'),list(rows[0]),rows)
    (output/(PREFIX+'_metadata.json')).write_text(json.dumps(metadata,indent=2)+'\n')
    return dict(status=metadata['status'],sampled_trials=source['sampled_trials'],components=len(components),paired_rows=len(pairs),new_searches=0,db_reads=0)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-dir',type=Path,required=True);p.add_argument('--output-dir',type=Path,required=True)
    p.add_argument('--mixtures-json',type=Path,required=True);p.add_argument('--model',default=engine.DEFAULT_MODEL)
    a=p.parse_args()
    try:print(json.dumps(run(a.input_dir,a.output_dir,a.mixtures_json,a.model),indent=2))
    except (OSError,ValueError,KeyError,TypeError,ZeroDivisionError) as exc:p.exit(1,f'High-VLA cache audit failed: {exc}\n')
