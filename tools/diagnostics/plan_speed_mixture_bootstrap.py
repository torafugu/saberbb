#!/usr/bin/env python3
"""Resample whole cached pitches for one fixed planned-speed mixture; no physics or DB."""
import argparse
from bisect import bisect_right
from collections import Counter
import json
import math
from pathlib import Path
import random

from contact_collision_probe import write_csv
from effective_mass_probe import direction
from execution_contact_posture import digest
from plan_tilt_direction_audit import Audit
import plan_speed_mixture_compare as planned

engine=planned.engine
PREFIX='plan_speed_mixture_bootstrap'
REGIONS=dict(ev_lt25=lambda e,v:e<=engine.EV_EDGES.index(25),
             ev_ge45=lambda e,v:e>engine.EV_EDGES.index(45),
             vla_lt0=lambda e,v:v<=engine.VLA_EDGES.index(0),
             vla_ge50=lambda e,v:v>engine.VLA_EDGES.index(50))


class PitchAudit(Audit):
    def __init__(self,model):
        super().__init__();self.model=model;self.pitches={}

    def add(self,dataset,records,models,kinematic_fields):
        super().add(dataset,records,models,kinematic_fields)
        if self.model not in models:raise ValueError('matching_model_required')
        base=records['saved_tilt'];key=(base['game_id'],base['count_seq'])
        if key in self.pitches:raise ValueError('duplicate_bootstrap_pitch')
        conditions={}
        for name,r in records.items():
            if name=='saved_tilt':continue
            valid=r.get('invalid_reason') is None;cell=None
            if valid:
                m=r['responses'][self.model]
                if direction(m)=='fair_direction':
                    cell=(bisect_right(engine.EV_EDGES,m['ev_ms']),bisect_right(engine.VLA_EDGES,m['vla_deg']))
            conditions[name]=(valid,valid and r['search_status']=='contact',cell)
        self.pitches[key]=conditions


def pitch_vectors(pitches,weights):
    """Retain zero-contact and clock-invalid pitches, never draw a strategic condition."""
    result=[]
    for key,conditions in sorted(pitches.items()):
        validity={conditions[n][0] for n in weights}
        if len(validity)!=1:raise ValueError('inconsistent_clock_population')
        valid=validity.pop();geometry=0.;cells=Counter()
        for name,w in weights.items():
            _,contact,cell=conditions[name]
            geometry+=w*contact
            if cell is not None:cells[cell]+=w
        result.append((int(valid),geometry,tuple(cells.items())))
    if not result:raise ValueError('empty_bootstrap_population')
    return result


def aggregate(vectors,multiplicities):
    evaluable=0;geometry=0.;mass=Counter()
    for i,count in multiplicities.items():
        valid,contact,cells=vectors[i]
        evaluable+=valid*count;geometry+=contact*count
        for cell,w in cells:mass[cell]+=count*w
    return evaluable,geometry,mass


def metrics(vectors,multiplicities,observed):
    evaluable,geometry,mass=aggregate(vectors,multiplicities)
    total=math.fsum(mass.values());no=sum(observed.values())
    values=dict(geometry_contact_probability=geometry/evaluable if evaluable else None,
                fair_probability_per_evaluable_trial=total/evaluable if evaluable else None)
    fractions={k:v/total for k,v in mass.items()} if total else {}
    for region,predicate in REGIONS.items():
        target=sum(v for (e,a),v in observed.items() if predicate(e,a))/no
        values[region+'_delta_pp']=100*(sum(v for (e,a),v in fractions.items() if predicate(e,a))-target) if total else None
    values['grid_total_variation']=.5*math.fsum(abs(fractions.get(k,0.)-observed[k]/no) for k in set(fractions)|set(observed)) if total else None
    return values


def limits(criteria):
    result={k+'_delta_pp':(-v,v) for k,v in criteria.get('max_abs_delta_pp',{}).items()}
    if 'max_grid_total_variation' in criteria:result['grid_total_variation']=(0.,criteria['max_grid_total_variation'])
    return result


def within(values,bounds):
    return all(values[k] is not None and lo<=values[k]<=hi for k,(lo,hi) in bounds.items())


def quantile(ordered,q):
    if not ordered:return None
    index=(len(ordered)-1)*q;i=math.floor(index);j=math.ceil(index)
    return ordered[i]+(ordered[j]-ordered[i])*(index-i)


def resample(vectors,observed,replicates,seed,bounds):
    rng=random.Random(seed);n=len(vectors);rows=[]
    for i in range(replicates):
        # One count per pitch, shared by every condition. Local RNG never touches game RNG.
        counts=Counter(rng.randrange(n) for _ in range(n))
        values=metrics(vectors,counts,observed)
        rows.append(dict(replicate=i+1,**values,all_within_tolerances=within(values,bounds)))
    return rows


def run(input_dir,statcast_csv,output,mixtures_json,tolerances_json,replicates=2000,bootstrap_seed='0',
        model=engine.DEFAULT_MODEL,bunt_labels=None):
    if type(replicates) is not int or replicates<1:raise ValueError('invalid_bootstrap_replicates')
    criteria=planned.tolerances(tolerances_json)
    if criteria is None:raise ValueError('bootstrap_requires_fixed_tolerances')
    config=engine.read_json(mixtures_json)
    if config.get('version')!=1 or not isinstance(config.get('mixtures'),dict) or len(config['mixtures'])!=1:
        raise ValueError('bootstrap_requires_exactly_one_fixed_mixture')
    label,weights=next(iter(config['mixtures'].items()))
    if not label.strip():raise ValueError('empty_mixture_label')
    audit=PitchAudit(model)
    states,source,paths,checks=planned.load_states(Path(input_dir),model,audit=audit)
    weights=engine.validate_weights(weights,states)
    observed,coverage=engine.observed(statcast_csv,bunt_labels);observed=engine.rebin_observed(observed)
    no=sum(observed.values())
    if not no:raise ValueError('no_complete_statcast_pairs')
    vectors=pitch_vectors(audit.pitches,weights);n=len(vectors)
    counts=Counter({i:1 for i in range(n)});point=metrics(vectors,counts,observed)
    if point['grid_total_variation'] is None:raise ValueError('empty_point_fair_cohort')
    # Recover the existing comparison estimator before resampling any pitches.
    expected_mass,expected_probability=engine.mix(states,weights,'fair_direction')
    valid,geometry,mass=aggregate(vectors,counts)
    errors=dict(checks)
    errors['bootstrap_point_fair_probability']=abs(point['fair_probability_per_evaluable_trial']-expected_probability)
    errors['bootstrap_point_grid_mass']=max(abs(mass[k]/valid-expected_mass[k]) for k in set(mass)|set(expected_mass))
    expected_geometry=sum(w*states[k]['counts']['evaluable_geometry_contacts']/valid for k,w in weights.items())
    errors['bootstrap_point_geometry_probability']=abs(point['geometry_contact_probability']-expected_geometry)
    if max(errors.values(),default=0)>1e-7:raise ValueError('bootstrap_point_reconstruction_mismatch')
    bounds=limits(criteria);rows=resample(vectors,observed,replicates,bootstrap_seed,bounds)
    summary=[]
    for metric,value in point.items():
        values=sorted(r[metric] for r in rows if r[metric] is not None)
        lo,hi=bounds.get(metric,(None,None))
        count=sum(r[metric] is not None and lo<=r[metric]<=hi for r in rows) if lo is not None else None
        region=metric.removesuffix('_delta_pp')
        target=sum(v for (e,a),v in observed.items() if REGIONS[region](e,a))/no if region in REGIONS else None
        summary.append(dict(metric=metric,units='percentage_points' if metric.endswith('_delta_pp') else 'fraction',
            point_estimate=value,simulation_point_fraction=target+value/100 if target is not None else None,
            statcast_reference_fraction=target,percentile_2_5=quantile(values,.025),median=quantile(values,.5),
            percentile_97_5=quantile(values,.975),replicates=replicates,valid_replicates=len(values),
            undefined_replicates=replicates-len(values),tolerance_lower=lo,tolerance_upper=hi,
            point_within_tolerance=lo<=value<=hi if lo is not None else None,
            within_tolerance_replicates=count,within_tolerance_fraction=count/replicates if count is not None else None))
    paths=list(paths)+[Path(statcast_csv),Path(mixtures_json),Path(tolerances_json)]+([Path(bunt_labels)] if bunt_labels else [])
    meta=dict(version=1,status='conditional_pitch_bootstrap_not_calibrated',cohort='fair_direction',model=model,
        source_metadata=source,mixture='mixture:'+label,weights=weights,tolerances=criteria,
        sampled_trials=n,evaluable_trials=valid,clock_invalid_trials=n-valid,statcast_n=no,
        replicates=replicates,bootstrap_seed=bootstrap_seed,resampling_unit='whole_pitch_all_conditions',
        sampling='n draws with replacement from n sampled pitches, including no-contact and clock-invalid pitches',
        percentile_method='linear interpolation at (valid_replicates - 1) * quantile; undefined draws excluded and counted',
        statcast_resampled=False,empty_fair_replicates=sum(r['grid_total_variation'] is None for r in rows),
        point_all_within_tolerances=within(point,bounds),
        all_within_tolerance_replicates=sum(r['all_within_tolerances'] for r in rows),
        all_within_tolerance_fraction=sum(r['all_within_tolerances'] for r in rows)/replicates,
        new_searches=0,db_reads=0,collision_replays=0,bin_edges=dict(ev_ms=engine.EV_EDGES,vla_deg=engine.VLA_EDGES),
        inputs={str(p):digest(p) for p in paths},limitations=[
            'Descriptive conditional percentile ranges, not a revised pass/fail rule or a calibrated equivalence test.',
            'Statcast is held fixed; its sampling uncertainty, missingness, estimates and cohort mismatch are not included.',
            'Pitch resampling treats pitches as exchangeable; player/game dependence and finite-population correction are not modeled.',
            'Mixture weights and criteria remain fixed; no strategic condition is randomly selected or fitted.',
            'Replicate threshold fractions are resampling frequencies, not probabilities that the true model is acceptable.',
            'Grid TV bootstrap percentiles are not bias-corrected and are not a null distribution or a significance test.',
            'All conditions share each resampled pitch count. Clock-invalid pitches are excluded from evaluated denominators, not treated as misses.',
            'Empty fair cohorts have undefined shape metrics and cannot pass configured criteria; they remain in threshold frequency denominators.',
            'Fair direction is not final fair-ball classification; physics eligibility and cohort/bunt limitations of the original comparison remain.',
            'A bootstrap range cannot alone establish that a discrepancy is systematic or explain its physical cause.'
        ])
    output=Path(output);output.mkdir(parents=True,exist_ok=False)
    for suffix,data in [('summary',summary),('replicates',rows),('checks',[dict(metric=k,max_error=v) for k,v in sorted(errors.items())])]:
        write_csv(output/(PREFIX+'_'+suffix+'.csv'),list(data[0]),data)
    write_csv(output/'statcast_coverage.csv',['metric','count'],[dict(metric=k,count=v) for k,v in sorted(coverage.items())])
    (output/(PREFIX+'_metadata.json')).write_text(json.dumps(meta,indent=2)+'\n')
    return {k:meta[k] for k in ('status','sampled_trials','replicates','point_all_within_tolerances','all_within_tolerance_fraction','empty_fair_replicates','new_searches','db_reads')}


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-dir',type=Path,required=True);p.add_argument('--statcast-csv',type=Path,required=True)
    p.add_argument('--output-dir',type=Path,required=True);p.add_argument('--mixtures-json',type=Path,required=True)
    p.add_argument('--tolerances-json',type=Path,required=True);p.add_argument('--replicates',type=int,default=2000)
    p.add_argument('--bootstrap-seed',default='0');p.add_argument('--model',default=engine.DEFAULT_MODEL)
    p.add_argument('--bunt-labels',type=Path)
    a=p.parse_args()
    try:print(json.dumps(run(a.input_dir,a.statcast_csv,a.output_dir,a.mixtures_json,a.tolerances_json,
        a.replicates,a.bootstrap_seed,a.model,a.bunt_labels),indent=2))
    except (OSError,ValueError,KeyError,TypeError,ZeroDivisionError) as exc:p.exit(1,f'Planned-mixture bootstrap failed: {exc}\n')
