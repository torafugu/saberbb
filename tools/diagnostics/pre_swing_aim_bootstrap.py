#!/usr/bin/env python3
"""Paired whole-pitch bootstrap for two fixed forecast-only aim policies."""
import argparse
from collections import Counter
import json
from pathlib import Path
import random

from bat_axis_origin_audit import check
from contact_collision_probe import write_csv
from execution_contact_posture import digest
from plan_speed_mixture_bootstrap import quantile
from pre_swing_aim import Forecast, read_policies
import pre_swing_aim_probe as policy

PREFIX = 'pre_swing_aim_bootstrap'
REGIONS = ('downward', 'high_vla', 'line_window')
VECTOR_SIZE = 1 + 2 * len(policy.EVENTS) + 1 + len(REGIONS)
PRIMARY_FAMILY = (15., 0., 1.)


def vector(before, after):
    if before['evaluable'] != after['evaluable']:
        raise ValueError('inconsistent_clock_population')
    common = before['fair'] and after['fair']
    return (int(before['evaluable']),
            *(int(before[k]) for k in policy.EVENTS),
            *(int(after[k]) for k in policy.EVENTS), int(common),
            *(int(after[k]) - int(before[k]) if common else 0 for k in REGIONS))


class PitchAudit(policy.PolicyAudit):
    def __init__(self, policies, model, scenarios, reference, candidate):
        super().__init__(policies, model, scenarios)
        self.reference = reference
        self.candidate = candidate
        self.pitches = {}

    def add(self, dataset, records, models, kinematic_fields):
        base = records['saved_tilt']
        key = base['game_id'], base['count_seq']
        if key in self.pitches:
            raise ValueError('duplicate_bootstrap_pitch')
        super().add(dataset, records, models, kinematic_fields)
        # Reuse the same outcome-blind selector as the point-estimate diagnostic.
        forecast = Forecast(base['forecast_plan']['forecast_position_world_m'][2])
        selected = [p for p in self.policies if p.name in (self.reference, self.candidate)]
        families = {}
        for family, cells in policy.family_cells(records, self.scenarios).items():
            choices = policy.select(forecast, selected, cells)
            before = policy.outcome(records[choices[self.reference]], self.model)
            after = policy.outcome(records[choices[self.candidate]], self.model)
            families[family] = vector(before, after)
        self.pitches[key] = families


def metrics(total):
    n = total[0]
    size = len(policy.EVENTS)
    before = dict(zip(policy.EVENTS, total[1:1 + size]))
    after = dict(zip(policy.EVENTS, total[1 + size:1 + 2 * size]))
    result = {}
    for event in policy.EVENTS:
        stem = event + '_probability_per_evaluable_trial'
        result['reference_' + stem] = policy.ratio(before[event], n)
        result['candidate_' + stem] = policy.ratio(after[event], n)
        result[event + '_probability_delta_pp'] = policy.ratio(100 * (after[event] - before[event]), n)
    for event in REGIONS:
        a = policy.ratio(before[event], before['fair'])
        b = policy.ratio(after[event], after['fair'])
        result['reference_' + event + '_fraction_among_fair'] = a
        result['candidate_' + event + '_fraction_among_fair'] = b
        result[event + '_fraction_delta_pp'] = 100 * (b - a) if a is not None and b is not None else None
    common_index = 1 + 2 * size
    for i, event in enumerate(REGIONS):
        result['common_fair_' + event + '_delta_pp'] = policy.ratio(100 * total[common_index + 1 + i], total[common_index])
    return result


def compress(pitches):
    """Group identical joint vectors; each original pitch retains its sampling mass."""
    if not pitches:
        raise ValueError('empty_bootstrap_population')
    families = sorted(next(iter(pitches.values())))
    if not families:
        raise ValueError('empty_bootstrap_families')
    patterns, indices, groups = [], [], {}
    for key, pitch in sorted(pitches.items()):
        if sorted(pitch) != families:
            raise ValueError('inconsistent_bootstrap_families')
        joint = tuple(pitch[f] for f in families)
        if any(len(v) != VECTOR_SIZE for v in joint):
            raise ValueError('invalid_bootstrap_vector')
        if joint not in groups:
            groups[joint] = len(patterns)
            patterns.append(joint)
        indices.append(groups[joint])
    return families, patterns, indices


def aggregate(patterns, counts):
    totals = [[0] * VECTOR_SIZE for _ in patterns[0]]
    for index, count in counts.items():
        for total, v in zip(totals, patterns[index]):
            for j, value in enumerate(v):
                total[j] += count * value
    return totals


def resample(patterns, indices, replicates, seed):
    rng = random.Random(seed)
    n = len(indices)
    for replicate in range(1, replicates + 1):
        # One draw of whole pitches shared by both policies and every speed/geometry.
        counts = Counter(indices[rng.randrange(n)] for _ in range(n))
        yield replicate, [metrics(t) for t in aggregate(patterns, counts)]


def audit_point(audit, families, point):
    summary, pairs, _ = audit.rows()
    for family, values in zip(families, point):
        same_family = lambda row: tuple(row[k] for k in policy.IDENTITY) == family
        a = next(r for r in summary if same_family(r) and r['stratum'] == 'overall' and r['policy'] == audit.reference)
        b = next(r for r in summary if same_family(r) and r['stratum'] == 'overall' and r['policy'] == audit.candidate)
        pair = next(r for r in pairs if same_family(r) and r['stratum'] == 'overall' and
                    {r['before_policy'], r['after_policy']} == {audit.reference, audit.candidate})
        sign = 1 if pair['before_policy'] == audit.reference else -1
        expected = {}
        for event in policy.EVENTS:
            stem = event + '_probability_per_evaluable_trial'
            expected['reference_' + stem] = a[stem]
            expected['candidate_' + stem] = b[stem]
            expected[event + '_probability_delta_pp'] = pair[event + '_probability_delta_pp']
            if expected[event + '_probability_delta_pp'] is not None:
                expected[event + '_probability_delta_pp'] *= sign
        for event in REGIONS:
            x, y = a[event + '_fraction_among_fair'], b[event + '_fraction_among_fair']
            expected['reference_' + event + '_fraction_among_fair'] = x
            expected['candidate_' + event + '_fraction_among_fair'] = y
            expected[event + '_fraction_delta_pp'] = 100 * (y - x) if x is not None and y is not None else None
            value = pair['common_fair_' + event + '_delta_pp']
            expected['common_fair_' + event + '_delta_pp'] = sign * value if value is not None else None
        for metric, value in values.items():
            other = expected[metric]
            if (value is None) != (other is None):
                raise ValueError('bootstrap_point_undefined_mismatch:' + metric)
            if value is not None:
                check(audit.checks, 'bootstrap_point_' + metric, abs(value - other), 1e-7)


def run(input_dir, output, policies_json=None, reference='contact_center', candidate='line_drive_low_center',
        replicates=2000, bootstrap_seed='0', model=policy.planned.engine.DEFAULT_MODEL):
    if type(replicates) is not int or replicates < 1:
        raise ValueError('invalid_bootstrap_replicates')
    policies_json = Path(policies_json) if policies_json else Path(__file__).with_name('pre_swing_aim_seed2_frozen.json')
    config = json.loads(policies_json.read_text())
    policies = read_policies(config)
    if reference == candidate or not {reference, candidate} <= {p.name for p in policies}:
        raise ValueError('requires_two_distinct_declared_policies')
    source_meta = json.loads((Path(input_dir) / 'plan_tilt_metadata.json').read_text())
    if source_meta.get('version') != 4:
        raise ValueError('requires_planned_speed_v4_and_matching_model')
    audit = PitchAudit(policies, model, source_meta['scenarios'], reference, candidate)
    _, source, paths, checks = policy.planned.load_states(Path(input_dir), model, audit=audit)
    families, patterns, indices = compress(audit.pitches)
    totals = aggregate(patterns, Counter(indices))
    point = [metrics(t) for t in totals]
    audit_point(audit, families, point)
    rows = []
    distributions = [{metric: [] for metric in values} for values in point]
    for replicate, samples in resample(patterns, indices, replicates, bootstrap_seed):
        for family, values, distribution in zip(families, samples, distributions):
            rows.append(dict(zip(policy.IDENTITY, family), replicate=replicate, **values))
            for metric, value in values.items():
                if value is not None:
                    distribution[metric].append(value)
    summary = []
    for family, values, distribution in zip(families, point, distributions):
        for metric, value in values.items():
            samples = sorted(distribution[metric])
            summary.append(dict(zip(policy.IDENTITY, family), reference_policy=reference, candidate_policy=candidate,
                metric=metric, role='primary' if family == PRIMARY_FAMILY and metric == 'line_window_probability_delta_pp' else 'supporting',
                units='percentage_points' if metric.endswith('_delta_pp') else 'fraction', point_estimate=value,
                percentile_2_5=quantile(samples, .025), median=quantile(samples, .5), percentile_97_5=quantile(samples, .975),
                replicates=replicates, valid_replicates=len(samples), undefined_replicates=replicates - len(samples)))
    metadata = dict(version=1, status='fixed_policy_paired_pitch_bootstrap', model=model, policies=config,
        reference_policy=reference, candidate_policy=candidate, difference='candidate minus reference',
        source_metadata=source, sampled_trials=len(indices),
        populations=[dict(zip(policy.IDENTITY, family), evaluable_trials=t[0], clock_invalid_trials=len(indices) - t[0],
                         common_fair=t[1 + 2 * len(policy.EVENTS)]) for family, t in zip(families, totals)],
        replicates=replicates, bootstrap_seed=bootstrap_seed, resampling_unit='whole_pitch_all_policies_and_families',
        unique_joint_patterns=len(patterns), sampling='n draws with replacement from n sampled pitches including no-contact and clock-invalid pitches',
        primary=dict(tilt_deg=15., yaw_batter_deg=0., planned_speed_ratio=1., metric='line_window_probability_delta_pp',
                     present=PRIMARY_FAMILY in families),
        percentile_method='linear interpolation at (valid_replicates - 1) * quantile; undefined draws excluded and counted per metric',
        new_searches=0, db_reads=0, collision_replays=0,
        inputs={str(p): digest(p) for p in list(paths) + [policies_json]},
        limitations=[
            'Conditional descriptive percentile intervals with policies and physical coefficients held fixed; no fitting or new pass/fail rule.',
            'Whole pitches are treated as exchangeable; player/game dependence and finite-population correction are not modeled.',
            'All speeds and geometries share each pitch multiplicity; their results are correlated, not independent replications.',
            'Clock-invalid pitches remain in sampling but not evaluable denominators. Unresolved/overlap cases retain the source estimator definition.',
            'Fair-conditioned fractions are recomputed within each policy and draw; empty denominators are undefined, never zero-filled.',
            'Common-fair effects condition on both policies producing fair-direction contacts; they do not describe all pitches.',
            'Fair-direction and the 10-to-30-degree line window are diagnostic labels, not final fair-ball adjudication or observed line-drive labels.',
            'Intervals do not include policy-selection uncertainty, sensor/model uncertainty, or external real-world validation.',
            'Only the declared primary contrast is primary; supporting metrics are not simultaneous confidence intervals or a search for the best policy.'
        ])
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False)
    for suffix, data in [('summary', summary), ('replicates', rows),
                         ('checks', [dict(metric=k, max_error=v) for k, v in sorted(checks.items())])]:
        write_csv(output / (PREFIX + '_' + suffix + '.csv'), list(data[0]), data)
    (output / (PREFIX + '_metadata.json')).write_text(json.dumps(metadata, indent=2) + '\n')
    return dict(status=metadata['status'], sampled_trials=len(indices), families=len(families), replicates=replicates,
                reference_policy=reference, candidate_policy=candidate, new_searches=0, db_reads=0)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input-dir', type=Path, required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    parser.add_argument('--policies-json', type=Path)
    parser.add_argument('--reference', default='contact_center')
    parser.add_argument('--candidate', default='line_drive_low_center')
    parser.add_argument('--replicates', type=int, default=2000)
    parser.add_argument('--bootstrap-seed', default='0')
    parser.add_argument('--model', default=policy.planned.engine.DEFAULT_MODEL)
    args = parser.parse_args()
    try:
        print(json.dumps(run(args.input_dir, args.output_dir, args.policies_json, args.reference, args.candidate,
                             args.replicates, args.bootstrap_seed, args.model), indent=2))
    except (OSError, ValueError, KeyError, TypeError, ZeroDivisionError) as exc:
        parser.exit(1, f'Paired aim bootstrap failed: {exc}\n')
