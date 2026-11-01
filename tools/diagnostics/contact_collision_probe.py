#!/usr/bin/env python3
"""Read-only, deterministic paired collision diagnostics; standard library only."""
import argparse
import csv
import math
import re
import sqlite3
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
KINDS = ('SolidContact', 'WeakContact', 'MarginalContact')
RADIUS = 0.070
BALL_MASS = 0.145  # Explicit trial assumption (kg), not a fitted parameter.
EPS = 1e-12
INPUTS = ('contact_offset_x_m', 'contact_offset_z_m', 'actual_bat_angle_deg',
          'hla_handedness_sign', 'bat_speed', 'pitch_speed', 'launch_speed',
          'launch_angle', 'final_hla_deg', 'approach_variation_deg',
          'pull_bias_deg', 'timing_angle_error')


def dot(a, b):
    return sum(x*y for x, y in zip(a, b))


def angles(velocity):
    """Y is forward, X is right; zero velocity has undefined directions."""
    x, y, z = velocity
    speed = math.sqrt(dot(velocity, velocity))
    horizontal = math.hypot(x, y)
    hla = math.degrees(math.atan2(x, y)) if horizontal > EPS else None
    vla = math.degrees(math.atan2(z, horizontal)) if speed > EPS else None
    return speed, hla, vla


def collide(incoming, bat_velocity, normal, restitution, effective_mass):
    """Frictionless normal impulse with finite bat effective mass.

    normal points from bat to ball; only approaching contacts receive impulse.
    Infinite effective_mass is supported for the stationary reflection control.
    """
    if not 0 <= restitution <= 1 or effective_mass <= 0:
        raise ValueError('Invalid restitution or effective mass')
    if abs(dot(normal, normal)-1) > 1e-9:
        raise ValueError('Contact normal must be unit length')
    relative_normal = dot(tuple(i-b for i, b in zip(incoming, bat_velocity)), normal)
    delta = max(0.0, -(1+restitution)*relative_normal/(1+BALL_MASS/effective_mass))
    outgoing = tuple(v+delta*n for v, n in zip(incoming, normal))
    return outgoing, relative_normal, delta


def prepare(row):
    missing = [k for k in INPUTS if row.get(k) is None]
    if missing:
        return None, 'missing:' + ','.join(missing)
    if not all(isinstance(row[k], (int, float)) and math.isfinite(row[k]) for k in INPUTS):
        return None, 'non_finite_or_non_numeric'
    if row['hla_handedness_sign'] not in (-1, 1):
        return None, 'invalid_handedness'
    if min(row['bat_speed'], row['pitch_speed'], row['launch_speed']) < 0:
        return None, 'negative_speed'
    if row['hla_model_version'] != 2:
        return None, 'unsupported_hla_version'
    theta = math.radians(row['actual_bat_angle_deg'])
    signed_thickness = -row['contact_offset_x_m']*math.sin(theta) + row['contact_offset_z_m']*math.cos(theta)
    radial = signed_thickness/RADIUS
    if abs(radial) > 1+1e-9:
        return None, 'outside_contact_radius'
    radial = max(-1.0, min(1.0, radial))
    # Recover the actual per-contact attack angle from the unchanged VLA formula.
    attack = math.radians(row['launch_angle']) - .60*math.asin(max(-1, min(1, row['contact_offset_z_m']/RADIUS)))
    if not -math.pi/2 <= attack <= math.pi/2:
        return None, 'invalid_reconstructed_attack'
    face = math.radians(.85*(row['approach_variation_deg'] + row['pull_bias_deg'] + row['timing_angle_error']))
    sign = row['hla_handedness_sign']

    def world(v):
        x, y, z = v
        # Rotate local forward toward +X, then mirror X for the actual batting side.
        return (sign*(x*math.cos(face)+y*math.sin(face)),
                y*math.cos(face)-x*math.sin(face), z)

    normal = world((-math.sin(theta)*radial, math.sqrt(max(0, 1-radial*radial)), math.cos(theta)*radial))
    bat = world((0, row['bat_speed']*math.cos(attack), row['bat_speed']*math.sin(attack)))
    stored = row.get('signed_thickness_offset_m')
    mismatch = stored is not None and (not math.isfinite(stored) or abs(stored-signed_thickness) > 1e-9)
    return dict(normal=normal, bat=bat, incoming=(0, -row['pitch_speed'], 0),
                attack_deg=math.degrees(attack), signed_thickness_m=signed_thickness,
                signed_thickness_mismatch=int(mismatch)), None


def direction(hla):
    if hla is None:
        return 'undefined'
    return 'backward' if abs(hla) >= 90 else 'foul_side' if abs(hla) >= 45 else 'fair'


def ev_band(ev):
    return '<25' if ev < 25 else '25..<35' if ev < 35 else '35..<40' if ev < 40 else '40..<45' if ev < 45 else '45+'


def vla_band(vla):
    if vla is None:
        return 'undefined'
    return '<0' if vla < 0 else '0..<10' if vla < 10 else '10..<25' if vla < 25 else '25..<40' if vla < 40 else '40+'


def write_csv(path, fields, rows):
    with path.open('x', newline='', encoding='utf-8') as stream:
        writer = csv.DictWriter(stream, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)


def run(db_path, first, last, output, restitutions=(.3, .5, .7), masses=(.3, .6)):
    if first > last:
        raise ValueError('first_game_id must be <= last_game_id')
    if not restitutions or not masses or not all(math.isfinite(e) and 0 <= e <= 1 for e in restitutions) or not all(math.isfinite(m) and m > 0 for m in masses):
        raise ValueError('Use finite restitution in [0,1] and positive effective mass')
    variants = [(f'moving_e{e:g}_m{m:g}', e, m) for e in restitutions for m in masses]
    if len({v[0] for v in variants}) != len(variants):
        raise ValueError('Duplicate or indistinguishable parameter variants')
    # Resolve/encode file URI so spaces, # and ? in paths cannot alter URI options.
    db = sqlite3.connect(Path(db_path).resolve().as_uri() + '?mode=ro', uri=True)
    db.row_factory = sqlite3.Row
    try:
        db.execute('PRAGMA query_only=ON')
        db.execute('BEGIN')  # All outputs use one database snapshot.
        params = dict(first_game_id=first, last_game_id=last)
        pitch_info = dict(db.execute('''SELECT COUNT(*) AS pitches, MIN(game_id) AS observed_first_game_id,
            MAX(game_id) AS observed_last_game_id FROM player_game_batting
            WHERE game_id BETWEEN :first_game_id AND :last_game_id''', params).fetchone())
        # These existing queries preserve missing fielding rows and deduplicate throws.
        sql = (ROOT/'sql/diagnostics/foul_direction_outcomes.sql').read_text()
        queries = [q for q in re.sub(r'^--.*$', '', sql, flags=re.M).split(';') if q.strip()]
        reports = []
        for query in queries[:3]:
            cursor = db.execute(query, params)
            reports.append(([c[0] for c in cursor.description], [dict(r) for r in cursor]))
        cursor = db.execute('''SELECT b.*, p.speed AS pitch_speed FROM player_game_batting b
            LEFT JOIN player_game_pitching p ON p.game_id=b.game_id AND p.count_seq=b.count_seq
            WHERE b.game_id BETWEEN :first_game_id AND :last_game_id
              AND b.adjusted_contact_type IN ('SolidContact','WeakContact','MarginalContact')
            ORDER BY b.game_id,b.count_seq''', params)
        output = Path(output)
        output.mkdir(parents=True, exist_ok=False)  # Never overwrite a prior run.
        for name, (fields, rows) in zip(('direction_coverage.csv', 'direction_transitions.csv', 'direction_gaps.csv'), reports):
            write_csv(output/name, fields, rows)
        counts, reasons = Counter(), Counter()
        grouped = defaultdict(lambda: dict(n=0, ev=0.0, hla=0.0, hla_n=0, vla=0.0, vla_n=0,
                                           foul=0, backward=0, separating=0, current_fouls=0))
        distribution = Counter()
        fields = ['game_id','count_seq','adjusted_contact_type','hla_handedness_sign','variant',
                  'restitution','effective_mass_kg','pitch_speed_proxy_ms','bat_speed_ms',
                  'reconstructed_attack_deg','signed_thickness_m','signed_thickness_mismatch',
                  'nx','ny','nz','bat_vx_ms','bat_vy_ms','bat_vz_ms',
                  'relative_normal_ms','normal_delta_v_ms','out_vx_ms','out_vy_ms','out_vz_ms',
                  'ev_ms','hla_deg','vla_deg','launch_direction','current_ev_ms','current_hla_deg',
                  'current_vla_deg','current_result']
        with (output/'collision_contacts.csv').open('x', newline='', encoding='utf-8') as stream:
            writer = csv.DictWriter(stream, fieldnames=fields)
            writer.writeheader()
            for raw in cursor:
                row = dict(raw)
                counts['contacts'] += 1
                geometry, reason = prepare(row)
                if reason:
                    reasons[reason] += 1
                    continue
                counts['comparable_contacts'] += 1
                counts['signed_thickness_mismatches'] += geometry['signed_thickness_mismatch']
                options = [('current', None, None)] + variants
                for variant, e, mass in options:
                    outgoing = relative = delta = None
                    if variant == 'current':
                        ev, hla, vla = row['launch_speed'], row['final_hla_deg'], row['launch_angle']
                    else:
                        outgoing, relative, delta = collide(geometry['incoming'], geometry['bat'], geometry['normal'], e, mass)
                        ev, hla, vla = angles(outgoing)
                    key = (row['adjusted_contact_type'], variant)
                    group = grouped[key]
                    group['n'] += 1
                    group['ev'] += ev
                    if hla is not None:
                        group['hla'] += abs(hla)
                        group['hla_n'] += 1
                        group['foul'] += abs(hla) >= 45
                        group['backward'] += abs(hla) >= 90
                    if vla is not None:
                        group['vla'] += vla
                        group['vla_n'] += 1
                    group['separating'] += relative is not None and relative >= 0
                    group['current_fouls'] += row['result'] == 'Foul'
                    distribution[(key[0],variant,ev_band(ev),vla_band(vla),direction(hla))] += 1
                    record = dict(game_id=row['game_id'],count_seq=row['count_seq'],
                        adjusted_contact_type=key[0],hla_handedness_sign=row['hla_handedness_sign'],variant=variant,
                        restitution=e,effective_mass_kg=mass,pitch_speed_proxy_ms=row['pitch_speed'],
                        bat_speed_ms=row['bat_speed'],reconstructed_attack_deg=geometry['attack_deg'],
                        signed_thickness_m=geometry['signed_thickness_m'],
                        signed_thickness_mismatch=geometry['signed_thickness_mismatch'],
                        nx=geometry['normal'][0],ny=geometry['normal'][1],nz=geometry['normal'][2],
                        bat_vx_ms=geometry['bat'][0],bat_vy_ms=geometry['bat'][1],bat_vz_ms=geometry['bat'][2],
                        relative_normal_ms=relative,normal_delta_v_ms=delta,
                        out_vx_ms=outgoing[0] if outgoing else None,
                        out_vy_ms=outgoing[1] if outgoing else None,out_vz_ms=outgoing[2] if outgoing else None,
                        ev_ms=ev,hla_deg=hla,vla_deg=vla,launch_direction=direction(hla),
                        current_ev_ms=row['launch_speed'],current_hla_deg=row['final_hla_deg'],
                        current_vla_deg=row['launch_angle'],current_result=row['result'])
                    writer.writerow(record)
        coverage = dict(first_game_id=first,last_game_id=last,**pitch_info,
                        contacts=counts['contacts'],comparable_contacts=counts['comparable_contacts'],
                        excluded_contacts=counts['contacts']-counts['comparable_contacts'],
                        signed_thickness_mismatches=counts['signed_thickness_mismatches'],trial_variants=len(variants))
        write_csv(output/'coverage.csv',list(coverage),[coverage])
        write_csv(output/'excluded_inputs.csv',['reason','contacts'],
                  [dict(reason=k,contacts=v) for k,v in sorted(reasons.items())])
        summaries = []
        for (kind,variant), g in sorted(grouped.items()):
            summaries.append(dict(adjusted_contact_type=kind,variant=variant,contacts=g['n'],
                avg_ev_ms=g['ev']/g['n'],avg_abs_hla_deg=g['hla']/g['hla_n'] if g['hla_n'] else None,
                avg_vla_deg=g['vla']/g['vla_n'] if g['vla_n'] else None,
                defined_hla_contacts=g['hla_n'],defined_vla_contacts=g['vla_n'],
                launch_foul_direction_pct=100*g['foul']/g['hla_n'] if g['hla_n'] else None,
                launch_backward_direction_pct=100*g['backward']/g['hla_n'] if g['hla_n'] else None,
                separating_contacts=g['separating'],current_result_fouls=g['current_fouls']))
        summary_fields = ['adjusted_contact_type','variant','contacts','avg_ev_ms','avg_abs_hla_deg',
                          'avg_vla_deg','defined_hla_contacts','defined_vla_contacts',
                          'launch_foul_direction_pct','launch_backward_direction_pct',
                          'separating_contacts','current_result_fouls']
        write_csv(output/'collision_summary.csv',summary_fields,summaries)
        write_csv(output/'collision_distribution.csv',
                  ['adjusted_contact_type','variant','ev_band','vla_band','launch_direction','contacts'],
                  [dict(adjusted_contact_type=k[0],variant=k[1],ev_band=k[2],vla_band=k[3],
                        launch_direction=k[4],contacts=v) for k,v in sorted(distribution.items())])
        return coverage
    finally:
        db.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--db', required=True, type=Path)
    parser.add_argument('--first-game-id', required=True, type=int)
    parser.add_argument('--last-game-id', required=True, type=int)
    parser.add_argument('--output-dir', required=True, type=Path, help='New directory; must not already exist')
    parser.add_argument('--restitution', nargs='+', type=float, default=[.3,.5,.7])
    parser.add_argument('--effective-mass-kg', nargs='+', type=float, default=[.3,.6])
    args = parser.parse_args()
    try:
        coverage = run(args.db,args.first_game_id,args.last_game_id,args.output_dir,
                       args.restitution,args.effective_mass_kg)
    except (ValueError, OSError, sqlite3.Error) as exc:
        parser.exit(1, f'Diagnostic failed: {exc}\n')
    print(', '.join(f'{k}={v}' for k,v in coverage.items()))
    print(f'CSV reports: {args.output_dir}')


if __name__ == '__main__':
    main()
