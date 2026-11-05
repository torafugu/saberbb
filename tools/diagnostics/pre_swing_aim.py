"""Outcome-blind diagnostic aim rules. This module has no contact/physics imports."""
from bisect import bisect_right
from dataclasses import dataclass
import math


def finite(value):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise ValueError('invalid_policy_number')
    return value


@dataclass(frozen=True)
class Forecast:
    height_m: float

    def __post_init__(self):
        finite(self.height_m)


@dataclass(frozen=True)
class AimPolicy:
    name: str
    intent: str
    height_edges_m: tuple
    aim_z_mm: tuple

    def choose(self, forecast):
        """Return an intended bat-reference offset, never a target outcome correction."""
        return self.aim_z_mm[bisect_right(self.height_edges_m, forecast.height_m)]


def read_policies(config):
    if set(config) != {'version', 'height_edges_m', 'policies'} or config['version'] != 1:
        raise ValueError('invalid_policy_config')
    edges = tuple(config['height_edges_m'])
    for x in edges:
        finite(x)
    if any(a >= b for a, b in zip(edges, edges[1:])):
        raise ValueError('unordered_policy_height_edges')
    if not isinstance(config['policies'], dict) or not config['policies']:
        raise ValueError('empty_policies')
    policies = []
    for name, rule in config['policies'].items():
        if not name.strip() or set(rule) != {'intent', 'aim_z_mm'} or not isinstance(rule['intent'], str) or not rule['intent'].strip():
            raise ValueError('invalid_policy_rule')
        aims = tuple(rule['aim_z_mm'])
        if len(aims) != len(edges) + 1:
            raise ValueError('policy_height_bin_count')
        for x in aims:
            finite(x)
        policies.append(AimPolicy(name, rule['intent'], edges, aims))
    return policies
