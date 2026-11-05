#!/usr/bin/env python3
"""Build explicitly requested intent plans and verify them against evaluated cache trajectories."""
import argparse
import json
from pathlib import Path

from batting_intent import BattedBallIntent, FROZEN_POLICIES, IntentPlanner, PlanningInput, POLICY_NAMES
from contact_collision_probe import write_csv
from execution_contact_posture import digest, same
import pre_swing_aim_probe as policy
from swing_execution import SwingPlan

PREFIX = 'batting_intent'


class IntentPlanAudit(policy.PolicyAudit):
    def __init__(self, planner, intents, model, source):
        super().__init__(planner.diagnostic_policies(intents), model, source['scenarios'])
        self.planner = planner
        self.intents = intents
        self.source = source
        self.plans = []

    def add(self, dataset, records, models, kinematic_fields):
        base = records['saved_tilt']
        inputs = PlanningInput.from_forecast_plan(base['forecast_plan'])
        for family, cells in policy.family_cells(records, self.scenarios).items():
            for intent in self.intents:
                # Build the plan before consulting any contact/response fields.
                built = self.planner.plan(inputs, intent, *family)
                aim_mm = built.aim_z_offset_m * 1000
                if aim_mm not in cells:
                    raise ValueError('missing_intent_aim_cell')
                name = cells[aim_mm]
                cached = records[name]
                same(built.forecast_plan, cached['forecast_plan'], self.checks, 'intent_forecast_plan')
                execution_plan = None
                if cached.get('invalid_reason') is None:
                    execution_plan = SwingPlan.from_saved(built.forecast_plan, 1.,
                        self.source['duration_sec'], self.source['anchor_fraction']).record()
                    same(execution_plan, cached['plan'], self.checks, 'intent_execution_plan')
                self.plans.append(dict(game_id=base['game_id'], count_seq=base['count_seq'],
                    **dict(zip(policy.IDENTITY, family)), selected_scenario=name,
                    **built.record(), execution_plan=execution_plan))
        super().add(dataset, records, models, kinematic_fields)


def run(input_dir, output, intents=('contact_first', 'line_drive'), model=policy.planned.engine.DEFAULT_MODEL):
    if not intents or len(set(intents)) != len(intents):
        raise ValueError('empty_or_duplicate_batting_intents')
    intents = tuple(BattedBallIntent(x) for x in intents)
    config = json.loads(FROZEN_POLICIES.read_text())
    planner = IntentPlanner(config)
    source = json.loads((Path(input_dir) / 'plan_tilt_metadata.json').read_text())
    if source.get('version') != 4:
        raise ValueError('requires_planned_speed_v4_and_matching_model')
    audit = IntentPlanAudit(planner, intents, model, source)
    _, source, paths, checks = policy.planned.load_states(Path(input_dir), model, audit=audit)
    summary, pairs, selections = audit.rows()
    for rows in (summary, selections):
        for row in rows:
            row['intent'] = row.pop('policy')
    for row in pairs:
        row['before_intent'] = row.pop('before_policy')
        row['after_intent'] = row.pop('after_policy')
    metadata = dict(version=1, status='explicit_intent_plan_cache_equivalence', model=model,
        intents=[i.value for i in intents], intent_policies={i.value: POLICY_NAMES[i] for i in intents},
        frozen_policies=config, source_metadata=source, sampled_trials=source['sampled_trials'],
        generated_plans=len(audit.plans), new_searches=0, db_reads=0, collision_replays=0,
        inputs={str(p): digest(p) for p in list(paths) + [FROZEN_POLICIES]},
        limitations=[
            'Intent is explicitly supplied by the caller; no count/runner/personality policy or stochastic intent mixture.',
            'Forecast-only aim rules and the existing execution-angle error are used to build plans before contact evaluation.',
            'Selected cached plan fields and evaluable pulse start poses are checked against freshly generated plans.',
            'Cached outcomes are reused only after plan equivalence checks; this is a wiring check, not another independent holdout.',
            'Clock-invalid cases retain forecast plans but have no evaluated execution plan; they are not misses.',
            'The diagnostic planned-swing pipeline remains separate from the Rust live batting resolver.',
            'Only contact_first and line_drive are exposed; a validated fly-ball intent is not yet defined.',
            'The same fair-direction, physics-eligibility, absolute-height and player/game-dependence limitations remain.'
        ])
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False)
    for suffix, rows in [('summary', summary), ('selections', selections),
                         ('checks', [dict(metric=k, max_error=v) for k, v in sorted(checks.items())])]:
        write_csv(output / (PREFIX + '_' + suffix + '.csv'), list(rows[0]), rows)
    pair_fields = list(pairs[0]) if pairs else [*policy.IDENTITY, 'stratum', 'forecast_height_band_m', 'before_intent', 'after_intent']
    write_csv(output / (PREFIX + '_pairs.csv'), pair_fields, pairs)
    with (output / (PREFIX + '_plans.jsonl')).open('w') as f:
        for row in audit.plans:
            f.write(json.dumps(row, allow_nan=False) + '\n')
    (output / (PREFIX + '_metadata.json')).write_text(json.dumps(metadata, indent=2) + '\n')
    return {k: metadata[k] for k in ('status', 'intents', 'sampled_trials', 'generated_plans', 'new_searches', 'db_reads')}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input-dir', type=Path, required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    parser.add_argument('--intents', choices=[i.value for i in BattedBallIntent], nargs='+', default=['contact_first', 'line_drive'])
    parser.add_argument('--model', default=policy.planned.engine.DEFAULT_MODEL)
    args = parser.parse_args()
    try:
        print(json.dumps(run(args.input_dir, args.output_dir, args.intents, args.model), indent=2))
    except (OSError, ValueError, KeyError, TypeError, ZeroDivisionError) as exc:
        parser.exit(1, f'Batting-intent plan probe failed: {exc}\n')
