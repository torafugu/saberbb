#!/usr/bin/env python3
"""Mix saved execution states before contact selection; exploratory Statcast comparison."""
import argparse
import json
import hashlib
import math
from bisect import bisect_right
from collections import Counter, defaultdict
from pathlib import Path

from contact_collision_probe import write_csv
from statcast_distribution_compare import load_csv, observed
from spin_distributions import EV_EDGES as OBS_EV, VLA_EDGES as OBS_VLA

EV_EDGES = (0,10,20,25,30,35,40,45,50,55,60)
VLA_EDGES = (-90,-60,-30,0,10,20,30,40,50,60,90)
COHORTS = {'all_successful':'successful_contacts', 'fair_direction':'fair_contacts'}
DEFAULT_VARIANT = 'observed_sigma_0.005_r0.9'
DEFAULT_MODEL = 'finite_I0.00015_et0.4'


def labels(edges):
    return [f'<{edges[0]:g}'] + [f'{a:g}..<{b:g}' for a,b in zip(edges,edges[1:])] + [f'{edges[-1]:g}+']


def unique_object(pairs):
    result = {}
    for key,value in pairs:
        if key in result:
            raise ValueError('duplicate_json_key: '+key)
        result[key] = value
    return result


def read_json(path):
    return json.loads(Path(path).read_text(), object_pairs_hook=unique_object)


def load_states(folder, scope, variant, model, histogram=None):
    folder = Path(folder)
    paths = [folder/'swing_execution_metadata.json', folder/'swing_execution_summary.csv']
    meta = read_json(paths[0])
    if meta['version'] != 2 or meta['variant'] != variant:
        raise ValueError('requires_matching_plan_execution_metadata_v2')
    configs = meta['scenarios']
    if not configs or not math.isfinite(meta['duration_sec']) or meta['duration_sec'] <= 0:
        raise ValueError('invalid_execution_metadata')
    for c in configs.values():
        if (not all(math.isfinite(c[k]) for k in ('planned_effort','execution_ratio','anchor_fraction','start_offset_sec'))
            or not 0 < c['planned_effort'] <= 1 or not 0 < c['execution_ratio'] <= 1
            or not 0 < c['anchor_fraction'] < 1):
            raise ValueError('invalid_execution_metadata')
    fields = ['scenario','scope','variant','model','orientation','stratum','band','trials',
              'evaluable_trials','execution_clock_invalid_trials','evaluable_geometry_contacts',
              'successful_contacts','fair_contacts']
    states = {}
    for r in load_csv(paths[1], fields):
        if tuple(r[k] for k in ('scope','variant','model','orientation','stratum','band')) != (scope,variant,model,'execution','overall','all'):
            continue
        name = r['scenario']
        if name in states or name not in configs:
            raise ValueError('duplicate_or_unknown_summary_scenario')
        counts = {k:int(r[k]) for k in fields[7:]}
        t,n,i,g,s,f = (counts[k] for k in fields[7:])
        if not 0 <= f <= s <= g <= n <= t or t != n+i or n <= 0:
            raise ValueError('invalid_summary_denominators')
        states[name] = dict(config=configs[name], counts=counts, grids={c:Counter() for c in COHORTS})
    if set(states) != set(configs):
        raise ValueError('missing_summary_scenarios')
    histogram = Path(histogram) if histogram else folder/'swing_execution_ev_vla.csv'
    if not histogram.exists():
        histogram = folder/'swing_execution_deviation_ev_vla.csv'
    paths.append(histogram)
    rows = load_csv(histogram,['scenario','scope','variant','model','orientation','cohort',
                               'ev_band_ms','vla_band_deg','contacts','cohort_contacts','fraction'])
    groups = defaultdict(list)
    for r in rows:
        if (r['scope'],r['variant'],r['model']) == (scope,variant,model):
            groups[r['scenario'],r['orientation'],r['cohort']].append(r)
    ev_labels, vla_labels = labels(EV_EDGES), labels(VLA_EDGES)
    used = set()
    for (name,orientation,cohort),cells in groups.items():
        if orientation not in ('execution','executed','planned'):
            continue
        if name not in configs or cohort not in COHORTS:
            raise ValueError('unknown_histogram_scenario_or_cohort')
        target = name
        if orientation == 'planned':
            c = configs[name]
            controls = [k for k,v in configs.items() if v['planned_effort']==c['planned_effort']
                        and v['anchor_fraction']==c['anchor_fraction'] and v['execution_ratio']==1 and v['start_offset_sec']==0]
            if len(controls) != 1:
                raise ValueError('missing_unique_plan_control')
            target = controls[0]
        expected = states[target]['counts'][COHORTS[cohort]]
        grid = Counter()
        for r in cells:
            key = ev_labels.index(r['ev_band_ms']), vla_labels.index(r['vla_band_deg'])
            count = int(r['contacts'])
            fraction = float(r['fraction'])
            if (key in grid or count <= 0 or int(r['cohort_contacts']) != expected
                or not math.isfinite(fraction) or not expected
                or not math.isclose(fraction,count/expected,rel_tol=1e-10,abs_tol=1e-12)):
                raise ValueError('invalid_histogram_cell')
            grid[key] = count
        if sum(grid.values()) != expected:
            raise ValueError('histogram_mass_mismatch')
        if (target,cohort) in used and states[target]['grids'][cohort] != grid:
            raise ValueError('inconsistent_repeated_plan_control')
        states[target]['grids'][cohort] = grid
        used.add((target,cohort))
    for name,state in states.items():
        for cohort,count_key in COHORTS.items():
            if sum(state['grids'][cohort].values()) != state['counts'][count_key]:
                raise ValueError('missing_histogram_mass: '+name+'/'+cohort)
    return states,meta,paths


def validate_weights(weights, states):
    if not isinstance(weights,dict) or not weights or set(weights)-set(states):
        raise ValueError('unknown_or_empty_mixture_states')
    if any(isinstance(w,bool) or not isinstance(w,(int,float)) or not math.isfinite(w) or w < 0 for w in weights.values()):
        raise ValueError('invalid_mixture_weight')
    if not math.isclose(sum(weights.values()),1.,rel_tol=0,abs_tol=1e-12):
        raise ValueError('mixture_weights_must_sum_to_one')
    weights = {k:w for k,w in weights.items() if w > 0}
    # Identical plan and clock parameters imply the same evaluated pitch set; equal counts alone do not.
    signatures = {(s['config']['planned_effort'],s['config']['anchor_fraction'],s['config']['start_offset_sec'],
                   s['counts']['trials'],s['counts']['evaluable_trials']) for k,s in states.items() if k in weights}
    if len(signatures) != 1:
        raise ValueError('mixture_requires_same_plan_and_evaluation_clock')
    return weights


def mix(states, weights, cohort):
    weights = validate_weights(weights,states)
    mass = Counter()
    for name,weight in weights.items():
        state = states[name]
        for key,count in state['grids'][cohort].items():
            mass[key] += weight*count/state['counts']['evaluable_trials']
    probability = sum(mass.values())
    return mass,probability


def rebin_observed(grid):
    result = Counter()
    for (e,v),count in grid.items():
        ei = bisect_right(EV_EDGES,OBS_EV[e-1]) if e else 0
        vi = bisect_right(VLA_EDGES,OBS_VLA[v-1]) if v else 0
        result[ei,vi] += count
    return result


def run(input_dir, statcast_csv, output, mixtures_json=None, scope='all_swings',
        variant=DEFAULT_VARIANT, model=DEFAULT_MODEL, bunt_labels=None, histogram=None):
    states,source,paths = load_states(input_dir,scope,variant,model,histogram)
    return compare(states,source,paths,statcast_csv,output,mixtures_json,scope,variant,model,bunt_labels)


def compare(states,source,paths,statcast_csv,output,mixtures_json=None,scope='all_swings',
            variant=DEFAULT_VARIANT,model=DEFAULT_MODEL,bunt_labels=None,default_mixtures=None,
            prefix='execution_mixture',comparison_kind='execution',emit_grid=True):
    paths=list(paths)
    mixtures = {'state:'+name:{name:1.} for name in states}
    for label,weights in (default_mixtures or {}).items():
        mixtures['mixture:'+label]=validate_weights(weights,states)
    if mixtures_json:
        config = read_json(mixtures_json)
        if config['version'] != 1 or not isinstance(config['mixtures'],dict) or not config['mixtures']:
            raise ValueError('invalid_mixtures_config')
        for label,weights in config['mixtures'].items():
            if not label.strip():
                raise ValueError('empty_mixture_label')
            mixtures['mixture:'+label] = validate_weights(weights,states)
        paths.append(Path(mixtures_json))
    obs,audit = observed(statcast_csv,bunt_labels)
    obs = rebin_observed(obs)
    no = sum(obs.values())
    if not no:
        raise ValueError('no_complete_statcast_pairs')
    paths += [Path(statcast_csv)] + ([Path(bunt_labels)] if bunt_labels else [])
    regions = dict(ev_lt25=lambda e,v:e<=EV_EDGES.index(25),
                   ev_ge45=lambda e,v:e>EV_EDGES.index(45),
                   vla_lt0=lambda e,v:v<=VLA_EDGES.index(0),
                   vla_ge50=lambda e,v:v>VLA_EDGES.index(50))
    grids,summary,components = [],[],[]
    ev_labels,vla_labels = labels(EV_EDGES),labels(VLA_EDGES)
    for name,weights in mixtures.items():
        for cohort in COHORTS:
            mass,probability = mix(states,weights,cohort)
            comparable = cohort == 'fair_direction' and probability > 0
            fractions = {key:value/probability for key,value in mass.items()} if probability else {}
            for ei,ev in enumerate(ev_labels if emit_grid else []):
                for vi,vla in enumerate(vla_labels):
                    a = fractions.get((ei,vi),0.) if probability else None
                    b = obs[ei,vi]/no if cohort == 'fair_direction' else None
                    grids.append(dict(mixture=name,cohort=cohort,ev_band_ms=ev,vla_band_deg=vla,
                        probability_per_evaluable_trial=mass[ei,vi],simulation_fraction=a,statcast_fraction=b,
                        delta_percentage_points=100*(a-b) if comparable else None))
            def region(predicate):
                return sum(p for (e,v),p in fractions.items() if predicate(e,v)) if probability else None
            counts = next(states[k]['counts'] for k,w in weights.items() if w > 0)
            contact = sum(w*states[k]['counts']['evaluable_geometry_contacts']/states[k]['counts']['evaluable_trials'] for k,w in weights.items())
            summary.append(dict(mixture=name,cohort=cohort,source_trials=counts['trials'],evaluable_trials=counts['evaluable_trials'],
                clock_invalid_trials=counts['execution_clock_invalid_trials'],geometry_contact_probability=contact,
                cohort_probability_per_evaluable_trial=probability,statcast_n=no if cohort=='fair_direction' else None,
                grid_total_variation=.5*sum(abs(fractions.get(key,0.)-obs[key]/no) for key in set(fractions)|set(obs)) if comparable else None,
                ev_lt25_fraction=region(lambda e,v:e<=EV_EDGES.index(25)),
                ev_ge45_fraction=region(lambda e,v:e>EV_EDGES.index(45)),
                vla_lt0_fraction=region(lambda e,v:v<=VLA_EDGES.index(0)),
                vla_ge50_fraction=region(lambda e,v:v>VLA_EDGES.index(50)),
                **{'statcast_'+label+'_fraction':sum(c for (e,v),c in obs.items() if predicate(e,v))/no
                   if cohort=='fair_direction' else None for label,predicate in regions.items()}))
            for state,weight in weights.items():
                if not weight:continue
                s=states[state];n=s['counts']['evaluable_trials'];c=s['counts'][COHORTS[cohort]]
                components.append(dict(mixture=name,cohort=cohort,scenario=state,trial_weight=weight,
                    evaluable_trials=n,cohort_contacts=c,cohort_probability=c/n,
                    selected_cohort_weight=weight*c/n/probability if probability else None))
    metadata = dict(version=1,status='exploratory_not_calibrated',scope=scope,variant=variant,model=model,
        source_metadata=source,mixtures=mixtures,statcast_n=no,comparison_kind=comparison_kind,
        statcast_complete_pair_rate=no/audit['selected_in_play'],
        bunt_policy='explicit_complete_labels' if bunt_labels else 'unknown_included',
        ev_conversion='launch_speed mph multiplied by 0.44704 to m/s',
        bin_edges=dict(ev_ms=EV_EDGES,vla_deg=VLA_EDGES),
        inputs={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths},
        limitations=[
            'Weights are user-specified trial-state probabilities, independent of pitch/player/context; no frequencies fitted or inferred.',
            'Conditional contact distributions use sum(weight * cell_count / evaluable_trials), then normalize after selection.',
            'Clock-invalid trials are excluded, not misses. Execution mixtures require identical plan/clock parameters; planned-speed mixtures require a shared validated evaluation set.',
            'Fair direction is not a full fair-ball classification; cap/curve and collision failures are not EV/VLA observations.',
            'Only fair_direction is compared with Statcast in-play complete pairs; all_successful is a separate diagnostic.',
            'Simulation bunt labels and equivalent player/league/season cohorts are not established.',
            'Statcast missing EV/VLA are excluded and audited; recorded values may include estimates.',
            'Mixtures and states share pitches. Weighted masses are expectations, not sample counts or independent samples.',
            'No significance, parameter fit, automatic winner selection, or production probability change is implied.',
        ])
    output=Path(output);output.mkdir(parents=True,exist_ok=False)
    for filename,rows in [(prefix+'_summary.csv',summary),(prefix+'_ev_vla.csv',grids),
                          (prefix+'_components.csv',components)]:
        if filename==prefix+'_ev_vla.csv' and not emit_grid:continue
        write_csv(output/filename,list(rows[0]),rows)
    write_csv(output/'statcast_coverage.csv',['metric','count'],[dict(metric=k,count=v) for k,v in sorted(audit.items())])
    (output/(prefix+'_metadata.json')).write_text(json.dumps(metadata,indent=2)+'\n')
    return dict(status=metadata['status'],states=len(states),custom_mixtures=len(mixtures)-len(states),statcast_n=no)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-dir',type=Path,required=True)
    p.add_argument('--statcast-csv',type=Path,required=True)
    p.add_argument('--output-dir',type=Path,required=True)
    p.add_argument('--mixtures-json',type=Path)
    p.add_argument('--histogram-csv',type=Path,help='Override normal histogram; also accepts deviation histogram with repeated planned controls')
    p.add_argument('--scope',choices=('all_swings','recorded_batting_only'),default='all_swings')
    p.add_argument('--variant',default=DEFAULT_VARIANT);p.add_argument('--model',default=DEFAULT_MODEL)
    p.add_argument('--bunt-labels',type=Path)
    a=p.parse_args()
    try:print(json.dumps(run(a.input_dir,a.statcast_csv,a.output_dir,a.mixtures_json,a.scope,a.variant,a.model,a.bunt_labels,a.histogram_csv),indent=2))
    except (OSError,ValueError,KeyError,TypeError) as exc:p.exit(1,f'Execution mixture comparison failed: {exc}\n')
