"""Outcome-neutral diagnostics for validated, fixed-axis contact state version 1."""
import math
from collections import defaultdict

from contact_collision_probe import EPS, cross, dot, relative_contact_geometry, write_csv


def legacy_group(source):
    """Match batting_resolver order and strict thresholds, without resampling quality."""
    x, z = source['legacy_contact_offset_xz_m']
    theta = math.radians(source['bat_angle_deg'])
    length = x * math.cos(theta) + z * math.sin(theta)
    thickness = -x * math.sin(theta) + z * math.cos(theta)
    group = ('length_miss' if abs(length) > .16 else
             'thickness_miss' if abs(thickness) > .07 else 'contact')
    return group, length, thickness


def timing_band(delay):
    ms = abs(delay) * 1000
    lower = 0
    for upper in (2, 5, 10, 15, 25):
        if ms < upper:
            return f'{lower}..<{upper}ms'
        lower = upper
    return '25ms+'


def relation(pairs):
    """Return Pearson r and OLS y-on-x slope; undefined values stay NULL."""
    if len(pairs) < 2:
        return None, None
    xm = sum(x for x, _ in pairs) / len(pairs)
    ym = sum(y for _, y in pairs) / len(pairs)
    xx = sum((x-xm)**2 for x, _ in pairs)
    yy = sum((y-ym)**2 for _, y in pairs)
    xy = sum((x-xm)*(y-ym) for x, y in pairs)
    return (xy / math.sqrt(xx*yy) if xx > 0 and yy > 0 else None,
            xy / xx if xx > 0 else None)


def diagnose(row, state, relative, entry, reason):
    source = state['input']
    axis = state['bat_axis_world']
    bat = state['bat_velocity_world_ms']
    velocity = source['ball_velocity_world_ms']
    u = tuple(v-b for v, b in zip(velocity, bat))
    transverse = tuple(v-dot(u, axis)*a for v, a in zip(u, axis))
    speed = math.sqrt(dot(transverse, transverse))
    # Compute b and closest time even if the trajectory misses the cylinder.
    if speed > EPS:
        tangent = cross(axis, tuple(-v/speed for v in transverse))
        b = dot(relative, tangent)
        b_zero = dot(source['aim_error_world_m'], tangent)
        db_dt = dot(bat, tangent)
        closest = -dot(relative, transverse)/(speed*speed)
        closest_zero = -dot(source['aim_error_world_m'], transverse)/(speed*speed)
        closest_slope = -dot(bat, transverse)/(speed*speed)
    else:
        b = b_zero = db_dt = closest = closest_zero = closest_slope = None
    zero, zero_reason = relative_contact_geometry(velocity, bat, axis, source['aim_error_world_m'])
    group, length, thickness = legacy_group(source)
    delay = source['timing_offset_sec']
    result = dict(game_id=row['game_id'], count_seq=row['count_seq'],
        adjusted_contact_type=row['adjusted_contact_type'], legacy_group=group,
        legacy_length_m=length, legacy_thickness_m=thickness,
        classification_consistent=(None if row['adjusted_contact_type'] not in
            ('SwungAndMiss','SolidContact','WeakContact','MarginalContact') else
            int((group != 'contact') == (row['adjusted_contact_type'] == 'SwungAndMiss'))),
        timing_band=timing_band(delay), timing_offset_ms=delay*1000,
        cylinder_intersection=int(entry is not None), geometry_status=reason or 'cylinder_intersection',
        b_over_r=b/.07 if b is not None else None,
        zero_timing_b_over_r=b_zero/.07 if b_zero is not None else None,
        delta_b_over_r=(b-b_zero)/.07 if b is not None else None,
        db_dtiming_mm_per_ms=db_dt, # m/s numerically equals mm/ms
        closest_time_offset_ms=closest*1000 if closest is not None else None,
        zero_timing_closest_time_offset_ms=closest_zero*1000 if closest_zero is not None else None,
        closest_time_slope_ms_per_ms=closest_slope,
        zero_timing_intersection=int(zero is not None),
        zero_timing_geometry_status=zero_reason or 'cylinder_intersection',
        first_entry_offset_ms=None, first_entry_since_release_sec=None,
        zero_timing_first_entry_offset_ms=zero['first_contact_time_offset_sec']*1000 if zero else None,
        delta_first_entry_ms=None, axial_at_entry_m=None, within_legacy_axial_window=None,
        b_phase_identity_error_m=abs(b-b_zero-db_dt*delay) if b is not None else None,
        closest_phase_identity_error_sec=abs(closest-closest_zero-closest_slope*delay) if closest is not None else None)
    if entry:
        dt = entry['first_contact_time_offset_sec']
        axial = dot(tuple(r+v*dt for r, v in zip(relative, u)), axis)
        result.update(first_entry_offset_ms=dt*1000,
            first_entry_since_release_sec=source['reference_time_sec_since_release']+dt,
            delta_first_entry_ms=(dt-zero['first_contact_time_offset_sec'])*1000 if zero else None,
            axial_at_entry_m=axial, within_legacy_axial_window=int(abs(axial) <= .16))
    return result


DETAIL_FIELDS = ['game_id','count_seq','adjusted_contact_type','legacy_group','legacy_length_m',
    'legacy_thickness_m','classification_consistent','timing_band','timing_offset_ms',
    'cylinder_intersection','geometry_status','b_over_r','zero_timing_b_over_r','delta_b_over_r',
    'db_dtiming_mm_per_ms','closest_time_offset_ms','zero_timing_closest_time_offset_ms',
    'closest_time_slope_ms_per_ms','zero_timing_intersection','zero_timing_geometry_status',
    'first_entry_offset_ms','first_entry_since_release_sec','zero_timing_first_entry_offset_ms',
    'delta_first_entry_ms','axial_at_entry_m','within_legacy_axial_window',
    'b_phase_identity_error_m','closest_phase_identity_error_sec']


def write_reports(output, records):
    write_csv(output/'state_miss_contacts.csv', DETAIL_FIELDS, records)
    keys = ['adjusted_contact_type','legacy_group','timing_band']
    groups = defaultdict(list)
    for r in records:
        groups[tuple(r[k] for k in keys)].append(r)
        groups[r['adjusted_contact_type'],r['legacy_group'],'all'].append(r)
    summaries = []
    for key, rows in sorted(groups.items()):
        hits = [r for r in rows if r['cylinder_intersection']]
        paired = [r for r in hits if r['zero_timing_intersection']]
        geometry = [r for r in rows if r['b_over_r'] is not None]
        def avg(field, subset=rows, absolute=False):
            values = [r[field] for r in subset if r[field] is not None]
            return sum(abs(v) if absolute else v for v in values)/len(values) if values else None
        time_r,time_slope = relation([(r['timing_offset_ms'],r['first_entry_offset_ms']) for r in hits])
        b_r,b_slope = relation([(r['timing_offset_ms'],r['b_over_r']) for r in geometry])
        summaries.append(dict(zip(keys,key), valid_states=len(rows), cylinder_intersections=len(hits),
            intersection_pct=100*len(hits)/len(rows),
            zero_timing_intersections=sum(r['zero_timing_intersection'] for r in rows),
            gained_with_timing=sum(r['cylinder_intersection'] and not r['zero_timing_intersection'] for r in rows),
            lost_with_timing=sum(not r['cylinder_intersection'] and r['zero_timing_intersection'] for r in rows),
            classification_comparable=sum(r['classification_consistent'] is not None for r in rows),
            classification_mismatches=sum(r['classification_consistent']==0 for r in rows),
            geometry_states=len(geometry), paired_intersections=len(paired),
            avg_abs_timing_ms=avg('timing_offset_ms',absolute=True),
            avg_abs_b_over_r=avg('b_over_r',absolute=True),
            avg_abs_delta_b_over_r=avg('delta_b_over_r',absolute=True),
            avg_first_entry_offset_ms=avg('first_entry_offset_ms',hits),
            avg_abs_first_entry_offset_ms=avg('first_entry_offset_ms',hits,True),
            avg_delta_first_entry_ms=avg('delta_first_entry_ms',paired),
            avg_abs_axial_at_entry_m=avg('axial_at_entry_m',hits,True),
            within_legacy_axial_window_pct=100*sum(r['within_legacy_axial_window'] for r in hits)/len(hits) if hits else None,
            timing_entry_pearson_r=time_r,timing_entry_slope_ms_per_ms=time_slope,
            timing_b_pearson_r=b_r,timing_b_slope_per_ms=b_slope,
            avg_closest_time_slope_ms_per_ms=avg('closest_time_slope_ms_per_ms'),
            max_b_phase_identity_error_m=max((r['b_phase_identity_error_m'] for r in geometry),default=None),
            max_closest_phase_identity_error_sec=max((r['closest_phase_identity_error_sec'] for r in geometry),default=None)))
    fields=keys+['valid_states','cylinder_intersections','intersection_pct','zero_timing_intersections',
        'gained_with_timing','lost_with_timing','classification_comparable','classification_mismatches',
        'geometry_states','paired_intersections','avg_abs_timing_ms','avg_abs_b_over_r','avg_abs_delta_b_over_r',
        'avg_first_entry_offset_ms','avg_abs_first_entry_offset_ms','avg_delta_first_entry_ms',
        'avg_abs_axial_at_entry_m','within_legacy_axial_window_pct','timing_entry_pearson_r',
        'timing_entry_slope_ms_per_ms','timing_b_pearson_r','timing_b_slope_per_ms',
        'avg_closest_time_slope_ms_per_ms','max_b_phase_identity_error_m','max_closest_phase_identity_error_sec']
    write_csv(output/'state_miss_summary.csv',fields,summaries)
