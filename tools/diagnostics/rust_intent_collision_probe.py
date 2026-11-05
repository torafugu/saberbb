#!/usr/bin/env python3
"""Evaluate frozen finite-barrel responses after auditing Rust intent contacts (SABERBB_DB)."""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import sqlite3

import bat_vibration_profile as profile
from batting_intent import frozen_planner, FROZEN_POLICIES
from contact_3d import ContactGeometry, band, solve_contact
from contact_collision_probe import dot, write_csv
from contact_spin import BALL_RADIUS_M
from diagnostic_db import db_from_env
from effective_mass_probe import direction
from execution_contact_posture import digest
from finite_barrel_collision import solve
from ground_spin_probe import check_metrics
from intent_execution_contract import motion_for, SETTINGS
from pitch_spin_source import InvalidPitchSpin, reconstruct
from rotating_bat_probe import norm, sub
from rust_execution_contact_audit import compare_execution
from contact_state_probe import SOURCE
from swing_execution import SwingPlan

PREFIX = 'rust_intent_collision'
MODEL = 'finite_I0.00015_et0.4'
AXIAL_INERTIA = .00015
MU, ET = .2, .4
SEARCHED = {'contact', 'no_contact', 'unresolved', 'overlap_at_start'}
EVENTS = ('geometry', 'successful', 'fair', 'downward', 'high_vla', 'line_window')


def prepare_contact(state, row):
    """Verification side only: consume an already audited, fixed intent plan."""
    if row['status'] != 'contact':
        return None, 'not_evaluated:' + row['status']
    probe = next(p for p in state['execution_contact']['probes'] if p['intent'] == row['intent'])
    result = probe['search']; g = result['geometry']; truth = state['input']
    if g['normal'] is None:
        return None, 'not_evaluated:missing_normal'
    saved = next(p for p in state['observed_intent_plans']['intent_plans']['plans'] if p['intent'] == row['intent'])
    plan = SwingPlan.from_saved(saved['plan']['forecast_plan'])
    geometry = ContactGeometry(truth['reference_time_sec_since_release'] + result['time_offset_sec'],
        tuple(g['axis']), tuple(g['normal']), g['axial_m'], g['closest_axial_m'], plan.half_length_m, g['contact_region'])
    geometry.validate()
    vin = tuple(truth['ball_velocity_world_ms']); vbat = tuple(g['bat_velocity'])
    if dot(sub(vin, vbat), geometry.normal_world) >= 0:
        return None, 'not_evaluated:non_approaching'
    if geometry.region != 'side':
        return None, 'not_evaluated:unsupported_cap'
    z = profile.REFERENCE_Z_M + geometry.closest_axial_m
    restitution = profile.profile_e(z)
    if restitution is None:
        return None, 'not_evaluated:out_of_curve'
    motion = motion_for(truth, plan)
    delay = plan.nominal_time_sec - truth['reference_time_sec_since_release']
    # Use the sin² pulse velocity AT CONTACT, not its peak or a reference-time sample.
    omega = motion.angular_velocity(result['time_offset_sec'], delay)
    return dict(geometry=geometry, vin=vin, vbat=vbat, omega=omega, source_z_m=z, restitution=restitution), None


def response(prepared, incoming_spin, checks):
    g = prepared['geometry']; mass = profile.MASS_KG
    inertia = mass * profile.GYRATION_M**2
    cm = profile.CM_Z_M - profile.REFERENCE_Z_M
    args = (g, prepared['vin'], prepared['vbat'], incoming_spin, prepared['omega'], mass,
            inertia, AXIAL_INERTIA, cm, SETTINGS['effective_radius_m'] - BALL_RADIUS_M, prepared['restitution'])
    control = solve(*args, mu=0., et=0.)
    check_metrics(checks, 'finite_mu0_', control.get('checks', {}))
    inverse_bat_mass = 1/mass + (g.closest_axial_m - cm)**2/inertia
    normal = solve_contact(g, prepared['vin'], prepared['vbat'], prepared['restitution'], 1/inverse_bat_mass)
    if control['collision_status'] != 'impulse_contact' or normal['collision_status'] != 'impulse_contact':
        raise ValueError('normal_control_unavailable')
    check_metrics(checks, 'control_', dict(outgoing_ms=norm(sub(control['outgoing_world_ms'], normal['outgoing_world_ms'])),
        unchanged_spin_rad_s=norm(sub(control['outgoing_spin_rad_s'], incoming_spin))))
    result = solve(*args, mu=MU, et=ET)
    check_metrics(checks, 'collision_', result.get('checks', {}))
    return result


def flags(row, result):
    successful = result['collision_status'] == 'impulse_contact'
    fair = successful and direction(result) == 'fair_direction'
    return dict(geometry=row['status'] == 'contact', successful=successful, fair=fair,
        downward=fair and result['vla_deg'] < 0, high_vla=fair and result['vla_deg'] >= 50,
        line_window=fair and 10 <= result['vla_deg'] < 30)


def ratio(a, b):
    return a/b if b else None


class Aggregate:
    def __init__(self):
        self.groups = defaultdict(Counter); self.pairs = defaultdict(Counter)

    def add(self, rows):
        for row in rows:
            for scope in ['all_plans'] + (['selected_plans'] if row['selected'] else []):
                for stratum, height in [('overall', 'all'), ('forecast_height', row['forecast_height_band_m'])]:
                    key = (scope, row['intent'], row['strategy'], row['preferred_intent'], stratum, height)
                    g = self.groups[key]; g['plans'] += 1
                    g['searched'] += row['status'] in SEARCHED
                    g['clock_invalid'] += row['status'].startswith('execution_starts_') or row['status'] == 'invalid_plan_available_clock'
                    g['search_invalid'] += row['status'] == 'invalid_contact_search_input'
                    for status in ('no_contact','unresolved','overlap_at_start'):
                        g[status] += row['status'] == status
                    for event in EVENTS:
                        g[event] += row[event]
                    for cohort in ('successful', 'fair'):
                        if not row[cohort]:
                            continue
                        for metric in ('ev_ms', 'vla_deg', 'spin_rpm'):
                            if row[metric] is not None:
                                g[cohort+'_n_'+metric] += 1; g[cohort+'_sum_'+metric] += row[metric]
                    g['status:' + row['collision_status']] += 1
        by_intent = {r['intent']: r for r in rows}
        a, b = by_intent['contact_first'], by_intent['line_drive']
        for stratum, height in [('overall', 'all'), ('forecast_height', a['forecast_height_band_m'])]:
            key = (a['strategy'], a['preferred_intent'], stratum, height)
            g = self.pairs[key]; g['pitches'] += 1
            g['common_searched'] += a['status'] in SEARCHED and b['status'] in SEARCHED
            for event in EVENTS:
                g['before_'+event] += a[event]; g['after_'+event] += b[event]
            for event in ('geometry', 'fair'):
                g[event+'_gained'] += not a[event] and b[event]
                g[event+'_lost'] += a[event] and not b[event]
            if a['fair'] and b['fair']:
                g['common_fair'] += 1
                for event in ('downward', 'high_vla', 'line_window'):
                    g['common_fair_'+event+'_delta'] += int(b[event])-int(a[event])
                for metric in ('ev_ms', 'vla_deg', 'spin_rpm'):
                    g['common_fair_delta_'+metric] += b[metric]-a[metric]

    def summaries(self):
        rows = []
        for key, g in sorted(self.groups.items()):
            r = dict(zip(('scope', 'intent', 'strategy', 'preferred_intent', 'stratum', 'forecast_height_band_m'), key))
            r.update({k:g[k] for k in ('plans', 'searched', 'clock_invalid', 'search_invalid', 'no_contact', 'unresolved', 'overlap_at_start', *EVENTS)})
            r['unavailable_geometry_contacts'] = g['geometry']-g['successful']
            for event in EVENTS:
                r[event+'_pct_per_searched'] = ratio(100*g[event], g['searched'])
            for event in ('downward', 'high_vla', 'line_window'):
                r[event+'_pct_among_fair'] = ratio(100*g[event], g['fair'])
            for cohort in ('successful', 'fair'):
                for metric in ('ev_ms', 'vla_deg', 'spin_rpm'):
                    r[cohort+'_n_'+metric] = g[cohort+'_n_'+metric]
                    r['mean_'+cohort+'_'+metric] = ratio(g[cohort+'_sum_'+metric], g[cohort+'_n_'+metric])
            rows.append(r)
        return rows

    def paired_rows(self):
        rows = []
        for key, g in sorted(self.pairs.items()):
            r = dict(zip(('strategy', 'preferred_intent', 'stratum', 'forecast_height_band_m'), key),
                     before_intent='contact_first', after_intent='line_drive')
            r.update({k:g[k] for k in ('pitches', 'common_searched', 'common_fair', 'geometry_gained', 'geometry_lost', 'fair_gained', 'fair_lost')})
            for event in EVENTS:
                r['before_'+event] = g['before_'+event]; r['after_'+event] = g['after_'+event]
                r[event+'_net_events_per_pitch_pp'] = ratio(100*(g['after_'+event]-g['before_'+event]), g['pitches'])
            for event in ('downward', 'high_vla', 'line_window'):
                r['common_fair_'+event+'_delta_pp'] = ratio(100*g['common_fair_'+event+'_delta'], g['common_fair'])
            for metric in ('ev_ms', 'vla_deg', 'spin_rpm'):
                r['common_fair_mean_delta_'+metric] = ratio(g['common_fair_delta_'+metric], g['common_fair'])
            rows.append(r)
        return rows


def run(db_path, first, last, output, gyro_sign=0, invalid_spin_policy='error'):
    if first > last or gyro_sign not in (-1, 0, 1) or invalid_spin_policy not in ('error', 'exclude'):
        raise ValueError('invalid_probe_parameters')
    output = Path(output)
    if output.exists():
        raise FileExistsError(output)
    planner = frozen_planner()
    coverage = Counter({k:0 for k in ('states','matched_states','matched_plans','replayed_searches',
        'response_events','solver_calls','spin_lookup_pitches','invalid_spin_pitches')})
    checks = defaultdict(float)
    aggregate = Aggregate(); details = []; statuses = Counter(); invalid_spins = []
    state_hash = hashlib.sha256(); spin_hash = hashlib.sha256(); seen = set()
    db = sqlite3.connect(Path(db_path).resolve().as_uri()+'?mode=ro', uri=True)
    try:
        db.execute('PRAGMA query_only=ON'); db.execute('BEGIN')
        source = db.execute('SELECT game_id,count_seq,state_version,state_source,invalid_reason,state_json '
            'FROM player_game_contact_state WHERE game_id BETWEEN ? AND ? ORDER BY game_id,count_seq', (first,last))
        for game, seq, version, origin, invalid, payload in source:
            key = (game,seq)
            if key in seen:
                raise ValueError('duplicate_intent_collision_pitch')
            seen.add(key); coverage['states'] += 1
            state_hash.update((json.dumps([game,seq,version,origin,invalid,payload], separators=(',',':'))+'\n').encode())
            if version != 1 or origin != SOURCE:
                coverage['unsupported_states'] += 1; continue
            if invalid is not None or payload is None:
                coverage['outer_invalid_states'] += 1; continue
            state = json.loads(payload)
            if state.get('observed_intent_plans') is None:
                coverage['missing_observed_intent_states'] += 1; continue
            rows, error = compare_execution(state, game, seq, planner, checks)
            if error:
                coverage['not_comparable:'+error] += 1; continue
            coverage['matched_states'] += 1
            prepared = [prepare_contact(state,r) for r in rows]
            incoming = None; spin_error = None
            if any(p is not None for p,_ in prepared):
                saved_spin = db.execute('SELECT spin_rate,spin_angle,spin_efficiency FROM player_game_pitching '
                    'WHERE game_id=? AND count_seq=?', key).fetchall()
                if len(saved_spin) != 1:
                    raise ValueError('pitch_spin_key_missing_or_duplicate:'+str(key))
                raw = saved_spin[0]
                spin_hash.update((json.dumps([game,seq,*raw], separators=(',',':'))+'\n').encode())
                coverage['spin_lookup_pitches'] += 1
                try:
                    try:
                        scalars = tuple(float(x) for x in raw)
                    except (ValueError,TypeError) as exc:
                        raise InvalidPitchSpin('non_numeric_pitch_spin_scalar') from exc
                    incoming = reconstruct(*scalars, state['input']['ball_velocity_world_ms'], gyro_sign)
                except InvalidPitchSpin as exc:
                    if invalid_spin_policy == 'error':
                        raise ValueError(f'invalid_pitch_spin:{key}:{exc.reason}') from exc
                    spin_error = 'not_evaluated:invalid_pitch_spin'
                    invalid_spins.append(dict(game_id=game,count_seq=seq,reason=exc.reason))
                    coverage['invalid_spin_pitches'] += 1
            evaluated = []
            for row, (p, reason) in zip(rows,prepared):
                coverage['matched_plans'] += 1
                coverage['search_status:'+row['status']] += 1
                coverage['replayed_searches'] += row['status'] in SEARCHED
                if p is not None and spin_error is None:
                    result = response(p,incoming,checks)
                    coverage['response_events'] += 1
                    coverage['solver_calls'] += 2  # Frozen model and independent mu=0 control.
                else:
                    result = dict(collision_status=reason or spin_error)
                coverage['collision_status:'+result['collision_status']] += 1
                metrics = dict(row, game_id=game,count_seq=seq,collision_status=result['collision_status'],
                    forecast_height_band_m=band(row['forecast_height_m'], (.6,.9,1.2)),
                    source_z_m=p['source_z_m'] if p else None, applied_e=p['restitution'] if p else None,
                    **flags(row,result), **{k:result.get(k) for k in ('ev_ms','hla_deg','vla_deg','spin_rpm')})
                statuses[row['intent'],row['selected'],result['collision_status']] += 1
                evaluated.append(metrics); details.append(metrics)
            aggregate.add(evaluated)
    finally:
        db.close()
    if not coverage['states']:
        raise ValueError('no_contact_state_rows_in_range')
    module_names = ('finite_barrel_collision.py','bat_vibration_profile.py','contact_3d.py','contact_spin.py',
        'contact_collision_probe.py','pitch_spin_source.py','rust_execution_contact_audit.py','intent_execution_contract.py',
        'rust_observed_intent_audit.py','rust_intent_plan_audit.py','observation_update_probe.py','batting_intent.py',
        'swing_execution.py','rotating_bat_probe.py','rust_intent_collision_probe.py')
    metadata = dict(version=1, status='evaluated_audited_intent_contacts' if coverage['response_events'] else 'no_comparable_collision_responses',
        db_path=str(db_path), first_game_id=first,last_game_id=last,coverage=dict(coverage),model=MODEL,
        model_parameters=dict(axial_inertia_kg_m2=AXIAL_INERTIA,mu=MU,et=ET,bat=profile.metadata()),
        definitions=dict(forecast_height_edges_m=[.6,.9,1.2],fair='abs(HLA) < 45 deg',downward='fair-direction VLA < 0 deg',high_vla='fair-direction VLA >= 50 deg',
            line_window='fair-direction 10 <= VLA < 30 deg; diagnostic window, not an observed line-drive label'),
        incoming_spin=dict(source='saved_pitch_scalars_reconstructed_at_contact_velocity',gyro_sign=gyro_sign,invalid_policy=invalid_spin_policy,
            assumptions='Transverse rpm = rate * efficiency. gyro_sign=0 omits unknown gyro; +/-1 sensitivity signs along velocity. No release-to-contact spin decay.'),
        db_access='single_read_only_transaction_for_states_and_spin',state_rows_sha256=state_hash.hexdigest(),spin_rows_sha256=spin_hash.hexdigest(),
        inputs={str(p):digest(p) for p in [FROZEN_POLICIES]+[Path(__file__).with_name(n) for n in module_names]},
        new_searches=coverage['replayed_searches'],collision_replays=coverage['solver_calls'],
        limitations=[
            'Python-only response diagnostic on audited Rust geometry; no Rust collision-response parity claim or live-game change.',
            'Intent, aim and pulse start pose stay frozen. Actual contact normals and response outcomes never feed selection.',
            'The existing full-speed positional restitution template and finite_I0.00015_et0.4 model are reused without fitting.',
            'Side contacts within the template only. Cap, out-of-curve, missing/invalid spin, non-approach and unavailable solver targets remain explicit.',
            'Search statuses remain separate from collision eligibility. Unavailable responses are not misses or zero EV.',
            'Summary reports all and selected plans plus forecast-height strata. Scopes/strata overlap; alternative intents share pitches.',
            'Rates per searched plan and rates among successful fair responses use different denominators. Paired EV/VLA uses common fair responses only.',
            'Pair net events per pitch includes all comparable pitch pairs, including invalid clocks. Common-searched count is reported separately.',
            'No statistical confidence intervals, game/player clustering, Statcast mixture fitting, flight/ground/fielder simulation or real-world calibration.'
        ])
    output.mkdir(parents=True,exist_ok=False)
    for suffix, rows, fields in [
        ('summary',aggregate.summaries(),['scope','intent','plans']),
        ('pairs',aggregate.paired_rows(),['before_intent','after_intent','pitches']),
        ('details',details,['game_id','count_seq','intent','status']),
        ('coverage',[dict(metric=k,count=v) for k,v in sorted(coverage.items())],['metric','count']),
        ('checks',[dict(metric=k,max_error=v) for k,v in sorted(checks.items())],['metric','max_error']),
        ('status',[dict(intent=k[0],selected=k[1],collision_status=k[2],plans=n) for k,n in sorted(statuses.items())],['intent','selected','collision_status','plans']),
        ('invalid_spin',invalid_spins,['game_id','count_seq','reason'])]:
        write_csv(output/(PREFIX+'_'+suffix+'.csv'),list(rows[0]) if rows else fields,rows)
    (output/(PREFIX+'_metadata.json')).write_text(json.dumps(metadata,indent=2,allow_nan=False)+'\n')
    return dict(status=metadata['status'],**coverage)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--first-game-id',type=int,required=True)
    parser.add_argument('--last-game-id',type=int,required=True)
    parser.add_argument('--output-dir',type=Path,required=True)
    parser.add_argument('--gyro-sign',type=int,choices=(-1,0,1),default=0)
    parser.add_argument('--invalid-spin-policy',choices=('error','exclude'),default='error')
    args = parser.parse_args()
    try:
        print(json.dumps(run(db_from_env(),args.first_game_id,args.last_game_id,args.output_dir,args.gyro_sign,args.invalid_spin_policy),indent=2))
    except (OSError,ValueError,KeyError,TypeError,sqlite3.Error) as exc:
        parser.exit(1,f'Rust intent collision diagnostic failed: {exc}\n')
