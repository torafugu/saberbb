"""Streaming paired-spin distributions; no samples or unavailable values imputed."""
import bisect
import math
from collections import Counter, defaultdict

from contact_collision_probe import write_csv
from effective_mass_probe import direction

KEYS = ['scope', 'variant', 'cohort', 'model']
DELTA_EDGES = {
    'ev_ms': (-2., -.5, -.1, 0., .1, .5, 2.),
    'vla_deg': (-10., -5., -2., -1., 0., 1., 2., 5., 10.),
    'spin_rpm': (-2000., -1000., -500., -100., 0., 100., 500., 1000., 2000.),
}
EV_EDGES = tuple(float(x) for x in range(0, 71, 5))
VLA_EDGES = tuple(float(x) for x in range(-90, 91, 10))


def bounds(edges, index):
    return (edges[index-1] if index else None,
            edges[index] if index < len(edges) else None)


class SpinDistributions:
    def __init__(self):
        self.stats = defaultdict(Counter)
        self.hist = Counter()
        self.joint = Counter()

    def add(self, r, zero, spun):
        scopes = ['all_swings']
        if r['adjusted_contact_type'] != 'UnrecordedBatting':
            scopes.append('recorded_batting_only')
        reference_fair = direction(zero['normal_profile']) == 'fair_direction'
        for model, left in zero.items():
            right = spun[model]
            if any(m['collision_status'] != 'impulse_contact' for m in (left, right)):
                continue
            for m in (left, right):
                if any(not math.isfinite(m[k]) for k in DELTA_EDGES):
                    raise ValueError('nonfinite_spin_distribution_metric')
            cohorts = ['all_common']
            if direction(right) == 'fair_direction':
                cohorts.append('input_fair_common')
            if reference_fair:
                cohorts.append('reference_fair_common')
            if direction(left) == direction(right) == 'fair_direction':
                cohorts.append('both_fair_common')
            for scope in scopes:
                for cohort in cohorts:
                    key = (scope, r['variant'], cohort, model)
                    stat = self.stats[key]
                    stat['contacts'] += 1
                    for metric, edges in DELTA_EDGES.items():
                        delta = right[metric] - left[metric]
                        stat['sum_' + metric] += delta
                        stat['abs_' + metric] += abs(delta)
                        stat['sq_' + metric] += delta * delta
                        stat[('positive_' if delta > 0 else 'negative_' if delta < 0 else 'zero_') + metric] += 1
                        self.hist[key + (metric, bisect.bisect_right(edges, delta))] += 1
                    for label, m in [('zero', left), ('input_spin', right)]:
                        self.joint[key + (label, bisect.bisect_right(EV_EDGES, m['ev_ms']),
                                          bisect.bisect_right(VLA_EDGES, m['vla_deg']))] += 1

    def write(self, output):
        stats, hist, joint = [], [], []
        for key, s in sorted(self.stats.items()):
            n = s['contacts']
            for metric in DELTA_EDGES:
                stats.append(dict(zip(KEYS, key), metric=metric, paired_contacts=n,
                    mean_delta=s['sum_'+metric]/n, mean_abs_delta=s['abs_'+metric]/n,
                    rms_delta=math.sqrt(s['sq_'+metric]/n),
                    positive=s['positive_'+metric], negative=s['negative_'+metric], zero=s['zero_'+metric]))
        for key, count in sorted(self.hist.items()):
            group, metric, index = key[:4], key[4], key[5]
            lo, hi = bounds(DELTA_EDGES[metric], index)
            n = self.stats[group]['contacts']
            hist.append(dict(zip(KEYS, group), metric=metric, bin_index=index,
                lower_inclusive=lo, upper_exclusive=hi, count=count, paired_contacts=n, fraction=count/n))
        for key, count in sorted(self.joint.items()):
            group, side, ei, vi = key[:4], key[4], key[5], key[6]
            elo, ehi = bounds(EV_EDGES, ei)
            vlo, vhi = bounds(VLA_EDGES, vi)
            n = self.stats[group]['contacts']
            joint.append(dict(zip(KEYS, group), side=side, ev_bin=ei, vla_bin=vi,
                ev_lower_inclusive=elo, ev_upper_exclusive=ehi,
                vla_lower_inclusive=vlo, vla_upper_exclusive=vhi,
                count=count, paired_contacts=n, fraction=count/n))
        write_csv(output/'finite_barrel_spin_delta_stats.csv', KEYS+[
            'metric','paired_contacts','mean_delta','mean_abs_delta','rms_delta','positive','negative','zero'], stats)
        write_csv(output/'finite_barrel_spin_delta_hist.csv', KEYS+[
            'metric','bin_index','lower_inclusive','upper_exclusive','count','paired_contacts','fraction'], hist)
        write_csv(output/'finite_barrel_spin_ev_vla.csv', KEYS+[
            'side','ev_bin','vla_bin','ev_lower_inclusive','ev_upper_exclusive',
            'vla_lower_inclusive','vla_upper_exclusive','count','paired_contacts','fraction'], joint)
