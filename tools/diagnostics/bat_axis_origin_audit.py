#!/usr/bin/env python3
"""Audit saved observation-update poses without collision searches or database access."""
import argparse
import csv
import json
import math
import sqlite3
import tempfile
from collections import Counter, defaultdict
from itertools import zip_longest
from pathlib import Path

from contact_3d import band
from contact_collision_probe import cross, dot, write_csv
from contact_point_speed_probe import digest
from rotating_bat_probe import add, mul, norm, rotate, sub
from swing_prediction_probe import PREDICTION_SOURCE, angle_for_position, plan_swing

VECTORS = ('forecast_position_world_m', 'pivot_world_m', 'pivot_velocity_world_ms',
           'ideal_axis_world', 'actual_axis_world', 'ideal_point_world_m', 'actual_point_world_m',
           'ideal_velocity_world_ms', 'actual_velocity_world_ms', 'ideal_omega_world_rad_s',
           'actual_omega_world_rad_s', 'execution_rotation_world_rad')
METRICS = ('forecast_z_m', 'planned_tilt_deg', 'actual_anchor_tilt_deg', 'attack_deg',
           'execution_error_deg', 'proxy_pivot_z_m', 'barrel_minus_pivot_z_m',
           'axis_velocity_dot_ms', 'entry_tilt_deg', 'phase_rotation_deg',
           'entry_proxy_pivot_z_m', 'entry_barrel_minus_pivot_z_m')
KEYS = ('scope', 'variant', 'stratum', 'band_1', 'band_2')


def finite(x):
    if isinstance(x, bool) or not isinstance(x, (int, float)) or not math.isfinite(x):
        raise ValueError('non_finite_or_non_numeric')
    return x


def number(row, key):
    x = float(row[key])
    return finite(x)


def tilt(a):
    return math.degrees(math.atan2(abs(a[2]), math.hypot(a[0], a[1])))


def check(checks, name, error, tolerance=1e-8):
    checks[name] = max(checks[name], error)
    if error > tolerance:
        raise ValueError('reconstruction_mismatch:' + name)


def inspect(plan, row, checks):
    if plan['plan_version'] != 1 or plan['source'] != PREDICTION_SOURCE:
        raise ValueError('unsupported_plan')
    for key in VECTORS:
        if len(plan[key]) != 3:
            raise ValueError('invalid_vector:' + key)
        for x in plan[key]: finite(x)
    for key in ('lever_m', 'half_length_m', 'nominal_time_sec', 'execution_angle_error_deg'):
        finite(plan[key])
    lever = plan['lever_m']
    if not lever > plan['half_length_m'] > 0 or plan['nominal_time_sec'] <= 0:
        raise ValueError('invalid_plan_parameters')
    # This side is inferred from the saved ideal axis, not independently verified against the DB.
    h = 1. if plan['ideal_axis_world'][0] > 0 else -1.
    theta = angle_for_position(plan['forecast_position_world_m'])
    v = plan['ideal_velocity_world_ms']
    attack = math.degrees(math.atan2(v[2], v[1]))
    if abs(attack) > 90: raise ValueError('invalid_attack')
    expected = plan_swing(dict(forecast=dict(position_world_m=plan['forecast_position_world_m'],
        time_sec=plan['nominal_time_sec']), handedness_sign=h, planned_bat_angle_deg=theta,
        planned_attack_angle_deg=attack, bat_speed_ms=norm(v),
        execution_angle_error_deg=plan['execution_angle_error_deg'], residual_execution_delay_sec=0.),
        lever, plan['half_length_m'])
    for key in VECTORS:
        check(checks, key, norm(sub(plan[key], expected[key])))
    values = dict(forecast_z_m=plan['forecast_position_world_m'][2], planned_tilt_deg=theta,
        actual_anchor_tilt_deg=tilt(plan['actual_axis_world']), attack_deg=attack,
        execution_error_deg=plan['execution_angle_error_deg'], proxy_pivot_z_m=plan['pivot_world_m'][2],
        barrel_minus_pivot_z_m=plan['actual_point_world_m'][2]-plan['pivot_world_m'][2],
        axis_velocity_dot_ms=dot(plan['actual_axis_world'], plan['actual_velocity_world_ms']))
    if row['status'] not in ('contact', 'no_contact', 'overlap_at_start', 'unresolved'):
        raise ValueError('unknown_search_status')
    if row['contact_contract_version'] != '1': raise ValueError('unsupported_contact_contract')
    if row['status'] != 'contact':
        if row['collision_status'] != 'not_evaluated:' + row['status']:
            raise ValueError('unexpected_collision_status')
        return values, h
    phase = (number(row, 'entry_time_offset_ms')-number(row, 'timing_offset_ms'))/1000
    reference_time = number(row, 'entry_since_release_sec')-number(row, 'entry_time_offset_ms')/1000
    check(checks, 'nominal_clock_sec', abs(plan['nominal_time_sec']-reference_time-number(row, 'timing_offset_ms')/1000))
    a = rotate(plan['actual_axis_world'], plan['actual_omega_world_rad_s'], phase)
    saved_axis = tuple(number(row, 'axis_at_entry_'+k) for k in 'xyz')
    check(checks, 'entry_axis', norm(sub(a, saved_axis)))
    omega = tuple(number(row, 'omega_'+k+'_rad_s') for k in 'xyz')
    check(checks, 'saved_omega_rad_s', norm(sub(omega, plan['actual_omega_world_rad_s'])))
    axial = number(row, 'closest_axial_at_entry_m')
    if abs(axial) > plan['half_length_m']+1e-8: raise ValueError('invalid_closest_axial')
    point_velocity = add(plan['pivot_velocity_world_ms'], cross(omega, mul(a, lever+axial)))
    check(checks, 'entry_point_velocity_ms', norm(sub(point_velocity,
        tuple(number(row, 'point_velocity_'+k+'_ms') for k in 'xyz'))))
    rotation = math.degrees(norm(omega)*phase)
    check(checks, 'phase_rotation_deg', abs(rotation-number(row, 'rotation_angle_at_entry_deg')))
    values.update(entry_tilt_deg=tilt(a), phase_rotation_deg=rotation,
        entry_proxy_pivot_z_m=plan['pivot_world_m'][2]+plan['pivot_velocity_world_ms'][2]*phase,
        entry_barrel_minus_pivot_z_m=a[2]*lever)
    return values, h


def run(input_dir, output, variants=None):
    root = Path(input_dir); output = Path(output)
    names = ('observation_update_inputs.jsonl', 'observation_update_contacts.csv',
             'observation_update_metadata.json', 'observation_update_coverage.csv')
    paths = {n: root/n for n in names}; hashes = {n: digest(p) for n,p in paths.items()}
    meta = json.loads(paths[names[2]].read_text())
    if meta.get('version') != 1: raise ValueError('unsupported_metadata')
    conditions = {'prior_only', 'exact_observation_control'} | {f'observed_sigma_{s:g}' for s in meta['sensor_sigma_m']}
    allowed = {f'{c}_r{l:g}' for c in conditions for l in meta['levers_m']}
    selected = set(variants) if variants is not None else allowed
    if not selected or not selected <= allowed: raise ValueError('invalid_variants')
    with paths[names[3]].open(newline='') as f: coverage = list(csv.DictReader(f))
    if len(coverage) != 1: raise ValueError('invalid_coverage')
    histories = int(coverage[0]['valid_histories']); expected = int(coverage[0]['trial_rows'])
    if expected != histories*len(allowed): raise ValueError('invalid_source_count')
    output.mkdir(parents=True, exist_ok=False)
    groups = defaultdict(Counter); checks = defaultdict(float); counts = Counter(); examples = []
    with tempfile.TemporaryDirectory(prefix='bat_axis_origin_') as tmp, \
         sqlite3.connect(str(Path(tmp)/'keys.db')) as db, \
         paths[names[0]].open() as pf, paths[names[1]].open(newline='') as cf:
        db.execute('CREATE TABLE keys(game INTEGER, seq INTEGER, variant TEXT, PRIMARY KEY(game,seq,variant))')
        reader = csv.DictReader(cf)
        if not reader.fieldnames or len(reader.fieldnames) != len(set(reader.fieldnames)):
            raise ValueError('missing_or_duplicate_csv_columns')
        for line, (raw, row) in enumerate(zip_longest(pf, reader), 1):
            try:
                if raw is None or row is None: raise ValueError('plan_contact_length_mismatch')
                record = json.loads(raw); plan = record['plan']
                game, seq = int(row['game_id']), int(row['count_seq'])
                variant = f"{record['condition']}_r{plan['lever_m']:g}"
                if (game < 0 or seq < 0 or record['game_id'] != game or record['count_seq'] != seq
                    or row['variant'] != variant or row['forecast_condition'] != record['condition']
                    or variant not in allowed or plan['lever_m'] != number(row,'lever_m')
                    or plan['half_length_m'] != meta['half_length_m']
                    or plan['half_length_m'] != number(row,'half_length_m')):
                    raise ValueError('plan_contact_key_or_geometry_mismatch')
                db.execute('INSERT INTO keys VALUES(?,?,?)',(game,seq,variant))
                values, h = inspect(plan, row, checks)  # Audit even unselected variants.
                counts['source_rows'] += 1
                if variant not in selected: continue
                counts['selected_rows'] += 1
                strata = [('overall','all','all'), ('inferred_side',str(int(h)),'all'),
                    ('planned_tilt',band(values['planned_tilt_deg'],(10,20,30,40,50,60,75,90)),'all'),
                    ('pivot_height',band(values['proxy_pivot_z_m'],(0,.25,.5,.75,1,1.5)),'all')]
                if 'entry_tilt_deg' in values:
                    strata.append(('planned_entry_tilt',band(values['planned_tilt_deg'],(10,20,30,40,50,60,75,90)),
                        band(values['entry_tilt_deg'],(10,20,30,40,50,60,75,90))))
                for scope in ('all_swings','recorded_batting_only'):
                    if scope == 'recorded_batting_only' and row['adjusted_contact_type']=='UnrecordedBatting': continue
                    for s,a,b in strata:
                        g = groups[scope,variant,s,a,b];g['rows']+=1;g[row['status']]+=1
                        g['proxy_pivot_below_ground_rows'] += values['proxy_pivot_z_m'] < 0
                        for k,v in values.items(): g[k]+=v;g['n_'+k]+=1
                if values['proxy_pivot_z_m'] < 0 and len(examples)<30:
                    examples.append(dict(game_id=game,count_seq=seq,variant=variant,**values))
            except (ValueError, KeyError, TypeError, OverflowError, sqlite3.IntegrityError) as exc:
                raise ValueError(f'input row {line}: {exc}') from exc
        if counts['source_rows'] != expected: raise ValueError('source_coverage_mismatch')
        if any(n != len(allowed) for (n,) in db.execute('SELECT COUNT(*) FROM keys GROUP BY game,seq')):
            raise ValueError('incomplete_source_keys')
    rows = []
    for key,g in sorted(groups.items()):
        r = dict(zip(KEYS,key));r.update({k:g[k] for k in ('rows','contact','no_contact','overlap_at_start','unresolved','proxy_pivot_below_ground_rows')})
        for k in METRICS: r['n_'+k]=g['n_'+k];r['avg_'+k]=g[k]/g['n_'+k] if g['n_'+k] else None
        rows.append(r)
    fields=list(KEYS)+['rows','contact','no_contact','overlap_at_start','unresolved','proxy_pivot_below_ground_rows']+[p+k for k in METRICS for p in ('n_','avg_')]
    write_csv(output/'bat_axis_origin_summary.csv',fields,rows)
    write_csv(output/'bat_axis_origin_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
    write_csv(output/'bat_axis_origin_coverage.csv',['source_rows','selected_rows'],[dict(source_rows=counts['source_rows'],selected_rows=counts['selected_rows'])])
    write_csv(output/'bat_axis_origin_examples.csv',['game_id','count_seq','variant']+list(METRICS),examples)
    (output/'bat_axis_origin_metadata.json').write_text(json.dumps(dict(version=1,inputs=hashes,variants=sorted(selected),
        note='Kinematic audit only. No search, response replay, RNG or DB access. Side inferred from saved ideal axis; pivot is a model proxy, not observed hands. Examples are first 30 below-ground proxy pivots, not a representative sample.'),indent=2)+'\n')
    return dict(source_rows=counts['source_rows'],selected_rows=counts['selected_rows'])


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input-dir',type=Path,required=True)
    parser.add_argument('--output-dir',type=Path,required=True)
    parser.add_argument('--variants',nargs='+')
    args=parser.parse_args()
    try: print(run(args.input_dir,args.output_dir,args.variants))
    except (OSError,ValueError,KeyError,TypeError,sqlite3.Error) as exc: parser.exit(1,f'Audit failed: {exc}\n')
