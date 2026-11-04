#!/usr/bin/env python3
"""Compare hypothetical trial-weighted planned-speed mixtures with Statcast."""
import argparse
import csv
import json
import math
from collections import Counter,defaultdict
from pathlib import Path

from contact_collision_probe import write_csv
from execution_contact_posture import digest
from plan_tilt_direction_audit import Audit,load_inputs
import swing_execution_mixture_compare as engine

PREFIX='plan_speed_mixture'
REGIONS=('ev_lt25','ev_ge45','vla_lt0','vla_ge50')


def load_states(folder,model):
    audit=Audit();datasets,inputs=load_inputs([folder],audit)
    dataset,record=next(iter(datasets.items()));meta=record['source_metadata']
    if meta['version']!=4 or model not in meta['models']:raise ValueError('requires_planned_speed_v4_and_matching_model')
    if (meta['plan_effort'],meta['execution_ratio'],meta['start_delay_sec'])!=(1.,1.,0.):raise ValueError('unsupported_execution_contract')
    states={};el=engine.labels(engine.EV_EDGES);vl=engine.labels(engine.VLA_EDGES)
    for name,c in meta['scenarios'].items():
        counts=dict(audit.replay[dataset,name,model])
        if counts['evaluable_trials']<=0:raise ValueError('no_evaluable_trials')
        grids={cohort:Counter() for cohort in engine.COHORTS}
        for (ds,scenario,m,cohort,e,v),count in audit.ev_hist.items():
            if (ds,scenario,m)==(dataset,name,model):grids[cohort][el.index(e),vl.index(v)]=count
        config=dict(c,planned_effort=1.,anchor_fraction=meta['anchor_fraction'],start_offset_sec=0.)
        states[name]=dict(config=config,counts=counts,grids=grids)
    return states,meta,[Path(p) for p in inputs],audit.checks


def compositions(total,length):
    if length==0:
        if total==0:yield ()
        return
    if length==1:
        yield (total,);return
    for n in range(total+1):
        for tail in compositions(total-n,length-1):yield (n,)+tail


def default_mixtures(states,step=.05,min_full=.6):
    if (isinstance(step,bool) or not isinstance(step,(int,float)) or not math.isfinite(step) or not 0<step<=1
        or isinstance(min_full,bool) or not isinstance(min_full,(int,float)) or not math.isfinite(min_full) or not 0<=min_full<=1):
        raise ValueError('invalid_weight_grid')
    units=round(1/step)
    if not math.isclose(units*step,1.,abs_tol=1e-12):raise ValueError('weight_step_must_divide_one')
    groups=defaultdict(list)
    for name,state in states.items():
        c=state['config'];groups[c['tilt_deg'],c['yaw_batter_deg'],c['aim_z_offset_m']].append(name)
    result={}
    for family,names in sorted(groups.items()):
        full=[n for n in names if states[n]['config']['planned_speed_ratio']==1.]
        if len(full)!=1:raise ValueError('requires_one_full_speed_state_per_geometry')
        slow=sorted((n for n in names if n!=full[0]),key=lambda n:states[n]['config']['planned_speed_ratio'])
        max_slow=units-math.ceil(min_full*units-1e-12)
        count=math.comb(max_slow+len(slow),len(slow)) if slow else 1
        if len(result)+count>10000:raise ValueError('too_many_mixtures_use_coarser_step')
        for total in range(max_slow+1 if slow else 1):
            for parts in compositions(total,len(slow)):
                weights={n:v/units for n,v in zip(slow,parts) if v}
                weights[full[0]]=(units-total)/units
                label=f'tilt_{family[0]:g}_yaw_{family[1]:g}_aim_z_{family[2]*1000:g}mm_units_'+ '_'.join(map(str,parts+(units-total,)))
                result[label]=weights
    return result


def tolerances(path):
    if path is None:return None
    c=engine.read_json(path)
    if c.get('version')!=1 or set(c)-{'version','max_abs_delta_pp','max_grid_total_variation'}:raise ValueError('invalid_tolerances_config')
    limits=c.get('max_abs_delta_pp',{})
    if not isinstance(limits,dict) or set(limits)-set(REGIONS):raise ValueError('unknown_tolerance_region')
    values=list(limits.values())+([c['max_grid_total_variation']] if 'max_grid_total_variation' in c else [])
    if not values or any(isinstance(v,bool) or not isinstance(v,(int,float)) or not math.isfinite(v) or v<0 for v in values):
        raise ValueError('invalid_tolerance_value')
    return c


def joint_mixtures(states,step=.1,min_full=.6):
    """Explicit independent speed/aim trial weights, separately for each tilt/yaw."""
    families=defaultdict(dict)
    for name,state in states.items():
        c=state['config'];key=(c['tilt_deg'],c['yaw_batter_deg'])
        cell=(c['aim_z_offset_m'],c['planned_speed_ratio'])
        if cell in families[key]:raise ValueError('duplicate_speed_aim_cell')
        families[key][cell]=name
    result={}
    for family,cells in sorted(families.items()):
        aims=sorted({a for a,r in cells});speeds=sorted({r for a,r in cells})
        if len(aims)<2:raise ValueError('joint_mixture_requires_multiple_aim_offsets')
        if set(cells)!={(a,r) for a in aims for r in speeds}:raise ValueError('missing_speed_aim_cell')
        representative={cells[aims[0],r]:states[cells[aims[0],r]] for r in speeds}
        speed_mixes=default_mixtures(representative,step,min_full)
        units=round(1/step);aim_count=math.comb(units+len(aims)-1,len(aims)-1)
        if len(result)+len(speed_mixes)*aim_count>10000:raise ValueError('too_many_mixtures_use_coarser_step')
        for i,weights in enumerate(speed_mixes.values()):
            speed_weights={states[n]['config']['planned_speed_ratio']:w for n,w in weights.items()}
            for parts in compositions(units,len(aims)):
                joint={cells[a,r]:speed_weights.get(r,0.)*part/units
                       for a,part in zip(aims,parts) for r in speeds if part and speed_weights.get(r,0.)}
                label=f'joint_tilt_{family[0]:g}_yaw_{family[1]:g}_speed_{i}_aim_units_'+ '_'.join(map(str,parts))
                result[label]=engine.validate_weights(joint,states)
    return result


def assess(output,criteria):
    with (output/(PREFIX+'_summary.csv')).open(newline='') as f:
        rows=[r for r in csv.DictReader(f) if r['cohort']=='fair_direction']
    assessments=[]
    for r in rows:
        a=dict(mixture=r['mixture']);failed=[];nonempty=float(r['cohort_probability_per_evaluable_trial'])>0
        for region in REGIONS:
            delta=100*(float(r[region+'_fraction'])-float(r['statcast_'+region+'_fraction'])) if nonempty else None
            a[region+'_delta_pp']=delta
            limit=(criteria or {}).get('max_abs_delta_pp',{}).get(region)
            a[region+'_tolerance_pp']=limit
            if limit is not None and (delta is None or abs(delta)>limit):failed.append(region)
        tv=float(r['grid_total_variation']) if nonempty else None
        a['grid_total_variation']=tv;a['max_grid_total_variation']=(criteria or {}).get('max_grid_total_variation')
        if a['max_grid_total_variation'] is not None and (tv is None or tv>a['max_grid_total_variation']):failed.append('grid_total_variation')
        a['assessment']='empty_cohort' if not nonempty else 'not_configured' if criteria is None else 'outside_configured_tolerances' if failed else 'within_configured_tolerances'
        a['failed_metrics']=';'.join(failed);assessments.append(a)
    write_csv(output/(PREFIX+'_assessment.csv'),list(assessments[0]),assessments)
    # Descriptive extrema are not an assertion that a single mixture attains all targets.
    envelopes=[]
    for kind in ('state:','mixture:'):
        selected=[r for r in rows if r['mixture'].startswith(kind) and r['grid_total_variation']!='']
        if not selected:continue
        for region in REGIONS:
            vals=[float(r[region+'_fraction']) for r in selected];target=float(selected[0]['statcast_'+region+'_fraction'])
            envelopes.append(dict(set=kind[:-1],metric=region,conditions=len(selected),minimum_fraction=min(vals),
                maximum_fraction=max(vals),statcast_fraction=target,target_inside_range=min(vals)<=target<=max(vals)))
    fields=['set','metric','conditions','minimum_fraction','maximum_fraction','statcast_fraction','target_inside_range']
    write_csv(output/(PREFIX+'_envelope.csv'),fields,envelopes)
    return Counter(r['assessment'] for r in assessments)


def run(input_dir,statcast_csv,output,mixtures_json=None,tolerances_json=None,step=.05,min_full=.6,
        model=engine.DEFAULT_MODEL,bunt_labels=None,mix_aim_offsets=False):
    criteria=tolerances(tolerances_json)
    states,source,paths,checks=load_states(Path(input_dir),model)
    if mix_aim_offsets and mixtures_json:raise ValueError('choose_explicit_mixtures_or_independent_grid')
    defaults=None if mixtures_json else (joint_mixtures(states,step,min_full) if mix_aim_offsets else default_mixtures(states,step,min_full))
    result=engine.compare(states,source,paths,statcast_csv,output,mixtures_json,
        variant=source['variant'],model=model,bunt_labels=bunt_labels,default_mixtures=defaults,
        prefix=PREFIX,comparison_kind='planned_speed_aim_trial_mixture' if mix_aim_offsets else 'planned_speed_trial_mixture',
        emit_grid=not mix_aim_offsets)
    out=Path(output);counts=assess(out,criteria)
    write_csv(out/(PREFIX+'_checks.csv'),['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
    path=out/(PREFIX+'_metadata.json');meta=engine.read_json(path)
    meta.update(new_searches=0,db_reads=0,collision_replays=0,tolerances=criteria,assessment_counts=dict(counts),
        default_grid=None if mixtures_json else dict(weight_step=step,minimum_full_speed_trial_weight=min_full),
        speed_aim_weight_assumption='independent_trial_weights_within_tilt_yaw' if mix_aim_offsets else None,
        ev_vla_grid_written=not mix_aim_offsets,
        interpretation='Hypothetical trial composition only; no batter strategy, RNG, fitted frequency, or automatic best-mixture selection.')
    if tolerances_json:meta['inputs'][str(tolerances_json)]=digest(tolerances_json)
    meta['limitations']+=['Default grid bounds are explicit sensitivity assumptions, not measured MLB swing-speed frequencies.',
        'Envelope ranges are separate marginal extrema across geometries/mixtures; simultaneous attainability requires assessment of one mixture.',
        'Configured tolerances are decision thresholds supplied by the user, not confidence intervals or statistical significance.',
        'A match does not establish cohort equivalence or prove actual strategic frequencies. Context-dependent strategies can differ from these constant weights.']
    path.write_text(json.dumps(meta,indent=2)+'\n')
    return dict(result,new_searches=0,db_reads=0,assessments=dict(counts))


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-dir',type=Path,required=True);p.add_argument('--statcast-csv',type=Path,required=True)
    p.add_argument('--output-dir',type=Path,required=True);p.add_argument('--mixtures-json',type=Path)
    p.add_argument('--tolerances-json',type=Path);p.add_argument('--weight-step',type=float,default=.05)
    p.add_argument('--minimum-full-speed-weight',type=float,default=.6)
    p.add_argument('--mix-aim-offsets',action='store_true',help='Cross speed weights with independent aim weights within each tilt/yaw; omit full grid CSV')
    p.add_argument('--model',default=engine.DEFAULT_MODEL);p.add_argument('--bunt-labels',type=Path)
    a=p.parse_args()
    try:print(json.dumps(run(a.input_dir,a.statcast_csv,a.output_dir,a.mixtures_json,a.tolerances_json,
        a.weight_step,a.minimum_full_speed_weight,a.model,a.bunt_labels,a.mix_aim_offsets),indent=2))
    except (OSError,ValueError,KeyError,TypeError,ZeroDivisionError) as exc:p.exit(1,f'Planned-speed mixture comparison failed: {exc}\n')
