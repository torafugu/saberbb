#!/usr/bin/env python3
"""Evaluate fixed coach/player intent decisions from audited paired caches; no database access."""
import argparse
from collections import Counter, defaultdict
from dataclasses import dataclass
import json
import math
from pathlib import Path
import random

from contact_collision_probe import write_csv
from execution_contact_posture import digest, same
from plan_speed_mixture_bootstrap import quantile
import rust_intent_game_bootstrap as cache
from rust_observed_intent_audit import decide

PREFIX = 'intent_selection_policy'
FIELDS = cache.SIDE_FIELDS
VIEWS = ('contact_first', 'line_drive', 'recorded')


@dataclass(frozen=True)
class Scenario:
    name: str
    strategy: str | None
    preference: str | None


# Declared independently of response outcomes. No fitting, scoring, or best-case selection.
SCENARIOS = (Scenario('recorded', None, None),) + tuple(
    Scenario(strategy + '__' + preference, strategy, preference)
    for strategy in ('let_batter_decide', 'put_ball_in_play', 'seek_line_drive')
    for preference in ('contact_first', 'line_drive'))


def selection(scenario, source_strategy, source_preference):
    """Only team objective and pre-existing player preference enter this function."""
    return decide(scenario.strategy or source_strategy, scenario.preference or source_preference)


def view(scenario):
    return 'recorded' if scenario.strategy is None else selection(scenario, '', '')['selected_intent']


def game_views(aggregate, stratum='overall', height='all'):
    result = {v: Counter() for v in VIEWS}
    for key, counts in aggregate.groups.items():
        scope, intent, strategy, preference, row_stratum, row_height = key
        if (scope, row_stratum, row_height) != ('all_plans', stratum, height):
            continue
        result[intent].update(counts)
        # Select first using the two declared inputs; results only enter accumulation.
        chosen = selection(SCENARIOS[0], strategy, preference)['selected_intent']
        if chosen == intent:
            result['recorded'].update(counts)
    return result


def metrics(counts):
    def ratio(n, d, scale=1.):
        return scale * n / d if d else None
    values = {event + '_pct_per_pitch': ratio(counts[event], counts['plans'], 100.)
              for event in cache.collision.EVENTS}
    values['searched_pct_per_pitch'] = ratio(counts['searched'], counts['plans'], 100.)
    for region in cache.REGIONS:
        values[region + '_pct_among_fair'] = ratio(counts[region], counts['fair'], 100.)
    for metric in cache.METRICS:
        values['mean_fair_' + metric] = ratio(counts['fair_sum_' + metric], counts['fair_n_' + metric])
    return values


def difference(candidate, reference):
    return {k: candidate[k] - value if candidate[k] is not None and value is not None else None
            for k, value in reference.items()}


def vectors_for(games):
    vectors = []
    for game in sorted(games):
        counts = game_views(games[game])
        vectors.append(tuple(counts[v][k] for v in VIEWS for k in FIELDS))
    return vectors


def total_vectors(vectors, weights):
    # Shared game multiplicities preserve pairing. Ratios are computed after summing.
    sums = [0] * len(vectors[0])
    for index, count in weights.items():
        for column, value in enumerate(vectors[index]):
            sums[column] += count * value
    n = len(FIELDS)
    return {view_name: Counter(dict(zip(FIELDS, sums[i*n:(i+1)*n])))
            for i, view_name in enumerate(VIEWS)}


def interval(samples, replicates, point, confidence):
    valid = len(samples)
    available = point is not None and valid == replicates
    alpha = (1. - confidence) / 2
    ordered = sorted(samples)
    return dict(ci_low=quantile(ordered, alpha) if available else None,
                ci_high=quantile(ordered, 1.-alpha) if available else None,
                valid_replicates=valid, undefined_replicates=replicates-valid,
                interval_status='point_undefined' if point is None else
                    'interval_available' if available else 'undefined_replicates')


def run(folder, output, replicates=2000, seed='0', confidence=.95):
    if type(replicates) is not int or replicates < 1 or not math.isfinite(confidence) or not 0 < confidence < 1:
        raise ValueError('invalid_bootstrap_parameters')
    output = Path(output)
    if output.exists():
        raise FileExistsError(output)
    meta, overall, games, checks, paths = cache.load_cache(folder)
    if len(games) < 2:
        raise ValueError('requires_at_least_two_game_clusters')
    vectors = vectors_for(games)
    point_counts = total_vectors(vectors, Counter({i: 1 for i in range(len(games))}))
    expected = game_views(overall)
    for v in VIEWS:
        same({k: expected[v][k] for k in FIELDS}, dict(point_counts[v]), checks, 'policy_game_sum.' + v)
    point = {v: metrics(point_counts[v]) for v in VIEWS}
    delta = {v: difference(point[v], point['contact_first']) for v in VIEWS}
    distributions = {v: defaultdict(list) for v in VIEWS}
    delta_distributions = {v: defaultdict(list) for v in VIEWS}
    rng = random.Random(str(seed))
    for _ in range(replicates):
        weights = Counter(rng.randrange(len(games)) for _ in games)
        totals = total_vectors(vectors, weights)
        sampled = {v: metrics(totals[v]) for v in VIEWS}
        for v in VIEWS:
            differences = difference(sampled[v], sampled['contact_first'])
            for k, value in sampled[v].items():
                if value is not None:
                    distributions[v][k].append(value)
                if differences[k] is not None:
                    delta_distributions[v][k].append(differences[k])
    summary = []
    for scenario in SCENARIOS:
        v = view(scenario)
        for metric, value in point[v].items():
            ci = interval(distributions[v][metric], replicates, value, confidence)
            delta_ci = interval(delta_distributions[v][metric], replicates, delta[v][metric], confidence)
            summary.append(dict(scenario=scenario.name, strategy_override=scenario.strategy,
                preferred_intent_override=scenario.preference, metric=metric, estimate=value, **ci,
                delta_vs_contact_first=delta[v][metric],
                **{'delta_' + k: value for k, value in delta_ci.items()},
                delta_unit='percentage_points' if '_pct_' in metric else metric.removeprefix('mean_fair_'),
                confidence=confidence, games=len(games), pitches=point_counts[v]['plans'],
                searched=point_counts[v]['searched'], fair=point_counts[v]['fair']))
    decisions = []
    for scenario in SCENARIOS:
        for key, counts in sorted(overall.groups.items()):
            scope, intent, strategy, preference, stratum, height = key
            if (scope, intent, stratum, height) != ('all_plans', 'contact_first', 'overall', 'all'):
                continue
            decision = selection(scenario, strategy, preference)
            decisions.append(dict(scenario=scenario.name, source_strategy=strategy,
                source_preferred_intent=preference, effective_strategy=scenario.strategy or strategy,
                effective_preferred_intent=scenario.preference or preference,
                selected_intent=decision['selected_intent'], decision_reason=decision['reason'], pitches=counts['plans']))
    for scenario in SCENARIOS:
        same(point_counts[view(scenario)]['plans'], sum(r['pitches'] for r in decisions if r['scenario']==scenario.name),
             checks, 'decision_population.' + scenario.name)
    heights = []
    for height in sorted({key[-1] for key in overall.groups if key[-2] == 'forecast_height'}):
        counts = game_views(overall, 'forecast_height', height)
        values = {v: metrics(counts[v]) for v in VIEWS}
        for scenario in SCENARIOS:
            v = view(scenario)
            diffs = difference(values[v], values['contact_first'])
            for metric, value in values[v].items():
                heights.append(dict(scenario=scenario.name, forecast_height_band_m=height, metric=metric,
                    estimate=value, delta_vs_contact_first=diffs[metric], pitches=counts[v]['plans'],
                    searched=counts[v]['searched'], fair=counts[v]['fair'], interval_status='descriptive_only'))
    coverage = dict(observed_games=len(games), contributing_games=sum(bool(g.pairs) for g in games.values()),
        pitches=meta['coverage']['matched_states'], scenarios=len(SCENARIOS), replicates=replicates,
        interval_available=sum(r['interval_status']=='interval_available' for r in summary),
        intervals_unavailable=sum(r['interval_status']!='interval_available' for r in summary))
    metadata = dict(version=1, status='fixed_coach_player_policy_evaluation', coverage=coverage,
        scenarios=[dict(name=s.name, strategy_override=s.strategy, preferred_intent_override=s.preference,
            effective_view=view(s)) for s in SCENARIOS], source_metadata=meta,
        selection_inputs=['team_strategy', 'player_preferred_intent'], reference='always_contact_first',
        policy_fitting=False, seed=str(seed), replicates=replicates, confidence=confidence,
        resampling='whole games with replacement; shared draws across every scenario; ratios of pooled counts',
        denominator='per_pitch includes every matched candidate pair, including invalid or unavailable responses; among_fair is conditional',
        height_intervals='descriptive only; no confidence intervals or automatic height-based selection',
        intent_aims='contact_first: center; line_drive: center below world height 0.6m, otherwise 15mm below forecast center',
        inputs={str(p): digest(p) for p in paths},
        code_sha256={p.name: digest(p) for p in (Path(__file__), Path(cache.__file__), Path(__file__).with_name('rust_observed_intent_audit.py'))},
        db_reads=0, new_searches=0, collision_replays=0,
        limitations=[
            'Six coach/preference combinations are uniform counterfactual settings; recorded preserves existing mixed decisions. No player IDs or new per-player traits are inferred from cached outcomes.',
            'Existing deterministic coach override rules yield only two distinct uniform intent policies; repeated equivalent scenarios are not independent evidence.',
            'PutBallInPlay currently selects ContactFirst by rule; that name does not establish optimal fair-ball probability. No outcome-based or fitted selector is introduced.',
            'Paired one-pitch diagnostic outcomes keep source pitch and execution inputs fixed. No live batting intervention, changed pitch sequence, hit/run/win effect, or Statcast calibration is evaluated.',
            'Per-pitch unavailable outcomes stay in the denominator; unavailable is not proof of a physical miss. Inspect geometry, successful response, searched and fair counts together.',
            'Reusing an evaluated holdout characterizes existing rules; changed thresholds, objectives or weights need a newly frozen policy and unused holdout.',
            'Games are resampled, not players/teams; intervals are marginal, not simultaneous. Undefined replicates suppress intervals instead of being zero-filled.',
            'Only supplied cache data and recorded zero-contribution games are used; no historical seed reconstruction or full replay is performed.'
        ])
    output.mkdir(parents=True, exist_ok=False)
    for suffix, rows in [('summary', summary), ('decisions', decisions), ('height', heights),
                         ('coverage', [dict(metric=k, count=v) for k, v in sorted(coverage.items())]),
                         ('checks', [dict(metric=k, max_error=v) for k, v in sorted(checks.items())])]:
        write_csv(output / (PREFIX + '_' + suffix + '.csv'), list(rows[0]), rows)
    (output / (PREFIX + '_metadata.json')).write_text(json.dumps(metadata, indent=2, allow_nan=False) + '\n')
    return dict(status=metadata['status'], **coverage)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input-dir', type=Path, required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    parser.add_argument('--replicates', type=int, default=2000)
    parser.add_argument('--bootstrap-seed', default='0')
    args = parser.parse_args()
    try:
        print(json.dumps(run(args.input_dir, args.output_dir, args.replicates, args.bootstrap_seed), indent=2))
    except (OSError, ValueError, KeyError, TypeError) as exc:
        parser.exit(1, f'Intent selection policy failed: {exc}\n')
