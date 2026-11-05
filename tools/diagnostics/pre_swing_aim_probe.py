#!/usr/bin/env python3
"""Evaluate frozen forecast-only aim policies on matched, validated cached pitches."""
import argparse
from collections import Counter, defaultdict
from itertools import combinations
import json
from pathlib import Path

from contact_3d import band
from contact_collision_probe import write_csv
from effective_mass_probe import direction
from execution_contact_posture import digest
from plan_tilt_direction_audit import Audit, HEIGHT_EDGES
import plan_speed_mixture_compare as planned
from pre_swing_aim import Forecast, read_policies

PREFIX = 'pre_swing_aim'
COUNTS = ('trials', 'evaluable', 'clock_invalid', 'geometry', 'successful', 'fair',
          'downward', 'high_vla', 'line_window', 'no_contact', 'unresolved',
          'overlap_at_start', 'unsupported_cap', 'out_of_curve', 'pivot_below_ground')
EVENTS = ('geometry', 'successful', 'fair', 'downward', 'high_vla', 'line_window')
IDENTITY = ('tilt_deg', 'yaw_batter_deg', 'planned_speed_ratio')


def ratio(a, b):
    return a / b if b else None


def outcome(record, model):
    """Evaluation side only. Called after the policy choices are fixed."""
    valid = record.get('invalid_reason') is None
    status = record['search_status'] if valid else 'clock_invalid'
    m = record['responses'][model] if valid else {}
    successful = m.get('collision_status') == 'impulse_contact'
    fair = valid and direction(m) == 'fair_direction'
    return dict(trials=1, evaluable=valid, clock_invalid=not valid,
        geometry=status == 'contact', successful=successful, fair=fair,
        downward=fair and m['vla_deg'] < 0, high_vla=fair and m['vla_deg'] >= 50,
        line_window=fair and 10 <= m['vla_deg'] < 30,
        no_contact=status == 'no_contact', unresolved=status == 'unresolved',
        overlap_at_start=status == 'overlap_at_start',
        unsupported_cap=m.get('collision_status') == 'not_evaluated:unsupported_cap',
        out_of_curve=m.get('collision_status') == 'not_evaluated:out_of_curve',
        pivot_below_ground=valid and record['kinematics']['pulse_min_pivot_z_m'] < 0,
        ev_ms=m['ev_ms'] if fair else None, vla_deg=m['vla_deg'] if fair else None)


def family_cells(records, scenarios):
    families = defaultdict(dict)
    for name, r in records.items():
        if name == 'saved_tilt':
            continue
        p = scenarios[name]
        # Declarations are independently checked against metadata by the cache loader.
        cell = tuple(p[k] for k in IDENTITY)
        aim = p['aim_z_offset_m'] * 1000
        if aim in families[cell]:
            raise ValueError('duplicate_policy_aim_cell')
        families[cell][aim] = name
    return families


def select(forecast, policies, cells):
    """Only a forecast, frozen rules and scenario identities can reach the selector."""
    choices = {}
    for policy in policies:
        aim = policy.choose(forecast)
        if aim not in cells:
            raise ValueError('missing_policy_aim_cell:' + str(aim))
        choices[policy.name] = cells[aim]
    return choices


def add_pair(g, before, after):
    g['trials'] += 1
    for field in ('evaluable', 'clock_invalid'):
        if before[field] != after[field]:
            raise ValueError('policy_population_mismatch')
        g[field] += before[field]
    for event in EVENTS:
        g['before_' + event] += before[event]
        g['after_' + event] += after[event]
    for event in ('geometry', 'fair'):
        g[event + '_gained'] += not before[event] and after[event]
        g[event + '_lost'] += before[event] and not after[event]
    if before['fair'] and after['fair']:
        g['common_fair'] += 1
        for event in ('downward', 'high_vla', 'line_window'):
            g['common_fair_' + event + '_delta'] += after[event] - before[event]
        g['common_fair_delta_ev_sum'] += after['ev_ms'] - before['ev_ms']
        g['common_fair_delta_vla_sum'] += after['vla_deg'] - before['vla_deg']


class PolicyAudit(Audit):
    def __init__(self, policies, model, scenarios):
        super().__init__()
        self.policies = policies
        self.model = model
        self.scenarios = scenarios
        self.policy_counts = defaultdict(Counter)
        self.pair_counts = defaultdict(Counter)
        self.choices = Counter()
        self.decisions = []

    def add(self, dataset, records, models, kinematic_fields):
        if self.model not in models:
            raise ValueError('matching_model_required')
        base = records['saved_tilt']
        forecast = Forecast(base['forecast_plan']['forecast_position_world_m'][2])
        families = family_cells(records, self.scenarios)
        # Require all declared rule branches even if this sample never visits one.
        for cells in families.values():
            if any(aim not in cells for p in self.policies for aim in p.aim_z_mm):
                raise ValueError('missing_policy_aim_cell')
        decisions = {family: select(forecast, self.policies, cells) for family, cells in families.items()}
        super().add(dataset, records, models, kinematic_fields)
        height = band(forecast.height_m, HEIGHT_EDGES)
        for family, choices in decisions.items():
            evaluated = {name: outcome(records[scenario], self.model) for name, scenario in choices.items()}
            for name, scenario in choices.items():
                f = evaluated[name]
                aim = records[scenario]['forecast_plan']['planned_aim_offset_world_m'][2] * 1000
                self.choices[family, name, height, aim, scenario] += 1
                self.decisions.append(dict(game_id=base['game_id'], count_seq=base['count_seq'],
                    **dict(zip(IDENTITY, family)), policy=name, forecast_height_m=forecast.height_m,
                    aim_z_mm=aim, selected_scenario=scenario))
                for stage, h in (('overall', 'all'), ('forecast_height', height)):
                    g = self.policy_counts[family, name, stage, h]
                    for field in COUNTS:
                        g[field] += f[field]
                    if f['fair']:
                        g['ev_sum'] += f['ev_ms']
                        g['vla_sum'] += f['vla_deg']
            for before, after in combinations(choices, 2):
                for stage, h in (('overall', 'all'), ('forecast_height', height)):
                    add_pair(self.pair_counts[family, before, after, stage, h], evaluated[before], evaluated[after])

    def rows(self):
        summary, pairs, selections = [], [], []
        for (family, policy, stage, height), g in sorted(self.policy_counts.items()):
            row = dict(zip(IDENTITY, family), policy=policy, stratum=stage, forecast_height_band_m=height,
                       **{field: g[field] for field in COUNTS})
            for event in EVENTS:
                row[event + '_probability_per_evaluable_trial'] = ratio(g[event], g['evaluable'])
            for event in ('downward', 'high_vla', 'line_window'):
                row[event + '_fraction_among_fair'] = ratio(g[event], g['fair'])
            row.update(mean_fair_ev_ms=ratio(g['ev_sum'], g['fair']), mean_fair_vla_deg=ratio(g['vla_sum'], g['fair']))
            summary.append(row)
        for (family, before, after, stage, height), g in sorted(self.pair_counts.items()):
            row = dict(zip(IDENTITY, family), before_policy=before, after_policy=after,
                       stratum=stage, forecast_height_band_m=height,
                       **{field: g[field] for field in ('trials', 'evaluable', 'clock_invalid', 'common_fair',
                           'geometry_gained', 'geometry_lost', 'fair_gained', 'fair_lost')})
            for event in EVENTS:
                row['before_' + event] = g['before_' + event]
                row['after_' + event] = g['after_' + event]
                row[event + '_probability_delta_pp'] = ratio(100 * (g['after_' + event] - g['before_' + event]), g['evaluable'])
            for event in ('downward', 'high_vla', 'line_window'):
                row['common_fair_' + event + '_delta_pp'] = ratio(100 * g['common_fair_' + event + '_delta'], g['common_fair'])
            row['common_fair_mean_delta_ev_ms'] = ratio(g['common_fair_delta_ev_sum'], g['common_fair'])
            row['common_fair_mean_delta_vla_deg'] = ratio(g['common_fair_delta_vla_sum'], g['common_fair'])
            pairs.append(row)
        for (family, policy, height, aim, scenario), count in sorted(self.choices.items()):
            selections.append(dict(zip(IDENTITY, family), policy=policy, forecast_height_band_m=height,
                                   aim_z_mm=aim, selected_scenario=scenario, trials=count))
        return summary, pairs, selections


def run(input_dir, output, policies_json=None, model=planned.engine.DEFAULT_MODEL):
    policies_json = Path(policies_json) if policies_json else Path(__file__).with_name('pre_swing_aim_policies.json')
    config = json.loads(policies_json.read_text())
    policies = read_policies(config)
    source_meta = json.loads((Path(input_dir) / 'plan_tilt_metadata.json').read_text())
    if source_meta.get('version') != 4:
        raise ValueError('requires_planned_speed_v4_and_matching_model')
    audit = PolicyAudit(policies, model, source_meta['scenarios'])
    states, source, paths, checks = planned.load_states(Path(input_dir), model, audit=audit)
    summary, pairs, selections = audit.rows()
    metadata = dict(version=1, status='forecast_only_policy_sensitivity_not_calibrated', model=model,
        policies=config, source_metadata=source,
        inputs={str(p): digest(p) for p in list(paths) + [policies_json]},
        selector_inputs=['forecast_height_world_m', 'frozen_policy', 'scenario_identity'],
        new_searches=0, db_reads=0, collision_replays=0,
        family_count=len({tuple(s['config'][k] for k in IDENTITY) for s in states.values()}),
        definitions=dict(downward='fair-direction VLA < 0 deg', high_vla='fair-direction VLA >= 50 deg',
                         line_window='fair-direction 10 <= VLA < 30 deg; diagnostic window, not a measured line-drive label'),
        limitations=[
            'Policies are explicit hypotheses, not fitted MLB frequencies or established optimal strategies.',
            'World-height cutoffs are not normalized strike-zone coordinates or a calibrated perceptual model.',
            'Intent labels do not guarantee achieved VLA. No inverse collision model or post-contact correction.',
            'Speeds, tilts and yaws are evaluated separately. No mixture weights, ranking or Statcast fitting.',
            'Clock-invalid trials are excluded from probability denominators. Unresolved/overlap cases are counted separately, not declared misses.',
            'Fair-direction is not final fair-ball adjudication. Unsupported contact physics and ground-pivot diagnostics remain visible.',
            'Conditional fair fractions change with the selected cohort; read paired trial rates and common-fair effects together.',
            'Reusing an inspected sample is exploratory. Freeze rules before applying to an untouched sample; no independent real-world validation is claimed.'
        ])
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False)
    for suffix, rows in [('summary', summary), ('pairs', pairs), ('selections', selections),
                         ('checks', [dict(metric=k, max_error=v) for k, v in sorted(checks.items())])]:
        write_csv(output / (PREFIX + '_' + suffix + '.csv'), list(rows[0]), rows)
    with (output / (PREFIX + '_decisions.jsonl')).open('w') as f:
        for row in audit.decisions:
            f.write(json.dumps(row) + '\n')
    (output / (PREFIX + '_metadata.json')).write_text(json.dumps(metadata, indent=2) + '\n')
    return dict(status=metadata['status'], sampled_trials=source['sampled_trials'], policies=len(policies),
                families=metadata['family_count'], new_searches=0, db_reads=0)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input-dir', type=Path, required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    parser.add_argument('--policies-json', type=Path)
    parser.add_argument('--model', default=planned.engine.DEFAULT_MODEL)
    args = parser.parse_args()
    try:
        print(json.dumps(run(args.input_dir, args.output_dir, args.policies_json, args.model), indent=2))
    except (OSError, ValueError, KeyError, TypeError, ZeroDivisionError) as exc:
        parser.exit(1, f'Pre-swing aim probe failed: {exc}\n')
