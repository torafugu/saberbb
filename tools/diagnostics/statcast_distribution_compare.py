#!/usr/bin/env python3
"""Exploratory EV/VLA comparison with explicit selection and missing-data audits."""
import argparse
import csv
import hashlib
import json
import math
from bisect import bisect_right
from collections import Counter
from pathlib import Path

from contact_collision_probe import write_csv
from spin_distributions import EV_EDGES, VLA_EDGES, bounds

ID = ['game_pk', 'at_bat_number', 'pitch_number']


def load_csv(path, required):
    with Path(path).open(encoding='utf-8-sig', newline='') as f:
        reader = csv.DictReader(f)
        if not set(required) <= set(reader.fieldnames or []):
            raise ValueError('missing_columns: ' + ','.join(sorted(set(required)-set(reader.fieldnames or []))))
        return list(reader)


def pitch_key(r):
    key = tuple(r[k].strip() for k in ID)
    if not all(key):
        raise ValueError('missing_pitch_key')
    return key


def number(value):
    if not value.strip() or value.strip().lower() in ('null', 'nan', 'na'):
        return None
    try:
        n = float(value)
    except ValueError:
        raise ValueError('invalid_numeric_value: ' + value)
    if not math.isfinite(n):
        raise ValueError('nonfinite_numeric_value')
    return n


def observed(path, bunt_labels=None):
    rows = load_csv(path, ID+['type', 'launch_speed', 'launch_angle'])
    labels = None
    if bunt_labels:
        labels = {}
        for r in load_csv(bunt_labels, ID+['is_bunt']):
            key = pitch_key(r)
            if key in labels or r['is_bunt'] not in ('0', '1'):
                raise ValueError('invalid_or_duplicate_bunt_label')
            labels[key] = r['is_bunt'] == '1'
    audit = Counter(); grid = Counter(); seen = set()
    for r in rows:
        key = pitch_key(r)
        if key in seen:
            raise ValueError('duplicate_statcast_pitch')
        seen.add(key); audit['input_pitches'] += 1
        if r['type'].strip() != 'X':
            audit['not_in_play'] += 1
            continue
        audit['in_play'] += 1
        if labels is not None:
            if key not in labels:
                raise ValueError('missing_bunt_label_for_in_play_pitch: ' + str(key))
            if labels[key]:
                audit['excluded_bunt'] += 1
                continue
        else:
            audit['bunt_unclassified_in_play'] += 1
        audit['selected_in_play'] += 1
        ev, vla = number(r['launch_speed']), number(r['launch_angle'])
        audit['missing_ev'] += ev is None
        audit['missing_vla'] += vla is None
        if ev is None or vla is None:
            audit['excluded_missing_either'] += 1
            continue
        ev *= .44704  # Statcast launch_speed: mph -> SI.
        if ev < 0 or not -90 <= vla <= 90:
            raise ValueError('out_of_range_statcast_ev_vla')
        grid[bisect_right(EV_EDGES, ev), bisect_right(VLA_EDGES, vla)] += 1
        audit['included_complete_pairs'] += 1
    return grid, audit


def simulated(path, scope, variant, cohort, model):
    required = ['scope','variant','cohort','model','side','ev_bin','vla_bin',
                'ev_lower_inclusive','ev_upper_exclusive','vla_lower_inclusive',
                'vla_upper_exclusive','count','paired_contacts','fraction']
    rows = [r for r in load_csv(path, required)
            if (r['scope'],r['variant'],r['cohort'],r['model'],r['side']) ==
               (scope,variant,cohort,model,'input_spin')]
    if not rows:
        raise ValueError('no_simulation_cohort: regenerate finite-barrel diagnostics or select an existing cohort')
    grid = Counter(); denominator = int(rows[0]['paired_contacts'])
    if denominator <= 0:
        raise ValueError('empty_simulation_denominator')
    for r in rows:
        ei, vi = int(r['ev_bin']), int(r['vla_bin'])
        if not 0 <= ei <= len(EV_EDGES) or not 0 <= vi <= len(VLA_EDGES):
            raise ValueError('invalid_bin_index')
        expected = bounds(EV_EDGES, ei)+bounds(VLA_EDGES, vi)
        actual = tuple(number(r[k]) for k in required[7:11])
        if actual != expected:
            raise ValueError('simulation_bin_bounds_mismatch')
        key = ei, vi
        if key in grid:
            raise ValueError('duplicate_simulation_cell')
        count = int(r['count'])
        if count <= 0 or int(r['paired_contacts']) != denominator or not math.isclose(float(r['fraction']),count/denominator,abs_tol=1e-12):
            raise ValueError('invalid_simulation_count_or_denominator')
        grid[key] = count
    if sum(grid.values()) != denominator:
        raise ValueError('simulation_mass_mismatch')
    return grid, denominator


def run(sim_csv, statcast_csv, output, scope='all_swings', variant='observed_sigma_0.005_r0.9',
        cohort='input_fair_common', model='finite_I0.00015_et0.4', bunt_labels=None):
    sim, ns = simulated(sim_csv,scope,variant,cohort,model)
    obs, audit = observed(statcast_csv,bunt_labels)
    no = sum(obs.values())
    if not no:
        raise ValueError('no_complete_statcast_pairs')
    output = Path(output); output.mkdir(parents=True,exist_ok=False)
    rows=[]
    # Dense union grid includes empty cells and both overflow tails.
    for ei in range(len(EV_EDGES)+1):
        for vi in range(len(VLA_EDGES)+1):
            elo,ehi=bounds(EV_EDGES,ei);vlo,vhi=bounds(VLA_EDGES,vi)
            a,b=sim[ei,vi],obs[ei,vi]
            rows.append(dict(ev_bin=ei,vla_bin=vi,ev_lower_inclusive=elo,ev_upper_exclusive=ehi,
                vla_lower_inclusive=vlo,vla_upper_exclusive=vhi,simulation_count=a,statcast_count=b,
                simulation_fraction=a/ns,statcast_fraction=b/no,delta_percentage_points=100*(a/ns-b/no)))
    write_csv(output/'statcast_ev_vla_comparison.csv',list(rows[0]),rows)
    regions={
        'ev_lt25_ms':lambda e,v: e<len(EV_EDGES) and EV_EDGES[e]<=25,
        'vla_lt0_deg':lambda e,v: v<len(VLA_EDGES) and VLA_EDGES[v]<=0,
        'vla_10_to30_deg':lambda e,v: v>0 and VLA_EDGES[v-1]>=10 and v<len(VLA_EDGES) and VLA_EDGES[v]<=30,
    }
    summary=[]
    for label,predicate in regions.items():
        a=sum(c for (e,v),c in sim.items() if predicate(e,v));b=sum(c for (e,v),c in obs.items() if predicate(e,v))
        summary.append(dict(region=label,simulation_n=ns,statcast_n=no,simulation_count=a,statcast_count=b,
            simulation_fraction=a/ns,statcast_fraction=b/no,delta_percentage_points=100*(a/ns-b/no)))
    write_csv(output/'statcast_regions.csv',list(summary[0]),summary)
    for k in ['input_pitches','not_in_play','in_play','excluded_bunt','bunt_unclassified_in_play',
              'selected_in_play','missing_ev','missing_vla','excluded_missing_either','included_complete_pairs']:
        audit[k]+=0
    write_csv(output/'statcast_coverage.csv',['metric','count'],[dict(metric=k,count=v) for k,v in sorted(audit.items())])
    metadata=dict(version=1,status='exploratory_not_calibrated',scope=scope,variant=variant,cohort=cohort,model=model,
        statcast_documentation='https://baseballsavant.mlb.com/csv-docs',
        selected_complete_pair_rate=no/audit['selected_in_play'],
        simulation_n=ns,statcast_n=no,bunt_policy='explicit_complete_labels' if bunt_labels else 'unknown_included',
        ev_conversion='launch_speed mph multiplied by 0.44704 to m/s',
        grid_total_variation=.5*sum(abs(x['simulation_fraction']-x['statcast_fraction']) for x in rows),
        inputs={str(p):hashlib.sha256(Path(p).read_bytes()).hexdigest() for p in [sim_csv,statcast_csv]+([bunt_labels] if bunt_labels else [])},
        limitations=[
            'Simulation fair direction is not a full fair-ball outcome (later flight/ground path is not classified).',
            'Simulation is limited to common successful collisions and source model eligibility; excludes cap/curve failures.',
            'Simulation bunt status is not provided by the histogram. No exact cohort equivalence is asserted.',
            'Statcast launch speed/angle may contain estimates; complete pairs are not guaranteed directly tracked.',
            'Missing EV/VLA are excluded and audited; their distribution is unknown, not filled with zero.',
            'Different player/league/season mix; no statistical significance or parameter fit inferred.',
            'Variants/cohorts/models overlap. Grid proportions are per selected dataset, not per pitch.',
        ])
    (output/'statcast_comparison_metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
    return metadata


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--simulation-csv',type=Path,required=True)
    p.add_argument('--statcast-csv',type=Path,required=True)
    p.add_argument('--output-dir',type=Path,required=True)
    p.add_argument('--bunt-labels',type=Path)
    p.add_argument('--scope',default='all_swings');p.add_argument('--variant',default='observed_sigma_0.005_r0.9')
    p.add_argument('--cohort',choices=['input_fair_common','reference_fair_common','both_fair_common','all_common'],default='input_fair_common')
    p.add_argument('--model',default='finite_I0.00015_et0.4');a=p.parse_args()
    try:
        result=run(a.simulation_csv,a.statcast_csv,a.output_dir,a.scope,a.variant,a.cohort,a.model,a.bunt_labels)
        print(json.dumps(result,indent=2))
    except (OSError,ValueError,KeyError) as exc:p.exit(1,f'Comparison failed: {exc}\n')
