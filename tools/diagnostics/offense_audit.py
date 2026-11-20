#!/usr/bin/env python3
"""Read-only raw-log/cache and home-run audit. Database comes from SABERBB_DB."""
import argparse
from bisect import bisect_right
from collections import Counter, defaultdict
import json
from pathlib import Path
import sqlite3

from diagnostic_db import db_from_env

AB_RESULTS = ('Single', 'Double', 'Triple', 'HomeRun', 'Strikeout', 'Out', 'FieldersChoice', 'DoublePlay')
PA_RESULTS = AB_RESULTS + ('Walk', 'HitByPitch')
AB_SQL = ','.join("'" + value + "'" for value in AB_RESULTS)
PA_SQL = ','.join("'" + value + "'" for value in PA_RESULTS)


def audit(path, season=None, limit=10, extra_base=False, *, batter_id=None, batter_line=None):
    """Never create, rebuild, or modify the supplied database or its caches."""
    uri = Path(path).expanduser().resolve().as_uri() + '?mode=ro'
    with sqlite3.connect(uri, uri=True, timeout=30) as conn:
        conn.row_factory = sqlite3.Row
        conn.execute('PRAGMA query_only=ON')
        conn.execute('BEGIN')  # Keep all report sections on one consistent read snapshot.
        def rows(sql, args=()):
            return [dict(row) for row in conn.execute(sql, args)]
        args = (season,)
        scope = 'FROM player_game_batting b JOIN game g ON g.id=b.game_id WHERE (?1 IS NULL OR g.season=?1)'
        measures = f'''COUNT(*) AS pitches,
            SUM(b.result IN ({PA_SQL})) AS pa, SUM(b.result IN ({AB_SQL})) AS ab,
            SUM(b.result='HomeRun') AS hr,
            SUM(b.result IN ('Single','Double','Triple','HomeRun')) AS hits'''
        players = rows(f'''SELECT b.batter_id,COUNT(DISTINCT b.game_id) AS games,{measures},
            AVG(CASE WHEN b.launch_speed>0 THEN b.launch_speed END) AS mean_ev_ms,
            AVG(CASE WHEN b.launch_speed>0 THEN b.launch_angle END) AS mean_vla_deg
            {scope} GROUP BY b.batter_id ORDER BY hr DESC,b.batter_id''', args)
        for player in players:
            player['ba'] = player['hits'] / player['ab'] if player['ab'] else None
            player['hr_per_pa'] = player['hr'] / player['pa'] if player['pa'] else None
        if batter_id is not None or batter_line is not None:
            return compare_batters(conn, season, limit, players, batter_id, batter_line)
        report = dict(scope=dict(season=season, all_seasons=season is None),
                      definitions=dict(pitches='All saved batting pitch records; not PA or AB',
                                       pa='Terminal batting results, including BB/HBP',
                                       ab='Terminal batting results excluding BB/HBP; sacrifices are not separately recorded'),
                      seasons=rows(f'SELECT g.season,COUNT(DISTINCT g.id) AS games,{measures} {scope} GROUP BY g.season', args),
                      top_batters=players[:limit],
                      results=rows(f'SELECT b.result,COUNT(*) AS n {scope} GROUP BY b.result ORDER BY n DESC', args),
                      contacts=rows(f'''SELECT b.adjusted_contact_type,COUNT(*) AS contacts,
                          AVG(b.launch_speed) AS mean_ev_ms,AVG(b.launch_angle) AS mean_vla_deg,
                          AVG(b.length_offset_m) AS mean_length_offset_m,AVG(b.thickness_offset_m) AS mean_thickness_offset_m,
                          SUM(b.result='HomeRun') AS hr {scope} AND b.launch_speed>0 GROUP BY b.adjusted_contact_type''', args),
                      home_run_geometry=rows(f'''SELECT COUNT(*) AS hr,
                          SUM(b.first_bounce_time IS NOT NULL) AS hr_with_prior_bounce,
                          SUM(ABS(b.final_hla_deg)>=45) AS hr_with_foul_launch_angle,
                          MIN(b.launch_speed) AS min_ev_ms,AVG(b.launch_speed) AS mean_ev_ms,MAX(b.launch_speed) AS max_ev_ms,
                          MIN(b.launch_angle) AS min_vla_deg,AVG(b.launch_angle) AS mean_vla_deg,MAX(b.launch_angle) AS max_vla_deg,
                          MIN(b.polar_distance) AS min_distance_m,MAX(b.polar_distance) AS max_distance_m
                          {scope} AND b.result='HomeRun' ''', args))
        tables = {row[0] for row in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        if 'batter_info' in tables:
            report['by_batter_type'] = rows(f'''SELECT i.batter_type,{measures},
                AVG(CASE WHEN b.launch_speed>0 THEN b.launch_speed END) AS mean_ev_ms,
                AVG(CASE WHEN b.launch_speed>0 THEN b.launch_angle END) AS mean_vla_deg
                FROM player_game_batting b JOIN game g ON g.id=b.game_id
                JOIN batter_info i ON i.player_id=b.batter_id
                WHERE (?1 IS NULL OR g.season=?1) GROUP BY i.batter_type''', args)
        report['cache_state'] = rows('SELECT * FROM stat_cache_state') if 'stat_cache_state' in tables else None
        report['cache_mismatches'] = None
        if 'game_batting_stat_cache' in tables:
            cached = {r['batter_id']: r for r in rows('''SELECT c.player_id AS batter_id,SUM(c.ab) AS ab,SUM(c.homerun) AS hr
                FROM game_batting_stat_cache c JOIN game g ON g.id=c.game_id
                WHERE (?1 IS NULL OR g.season=?1) GROUP BY c.player_id''', args)}
            raw = {r['batter_id']: r for r in players}
            mismatches = []
            for player_id in sorted(raw.keys() | cached.keys()):
                actual, saved = raw.get(player_id, {}), cached.get(player_id, {})
                if any(actual.get(k, 0) != saved.get(k, 0) for k in ('ab', 'hr')):
                    mismatches.append(dict(batter_id=player_id, raw_ab=actual.get('ab', 0), raw_hr=actual.get('hr', 0),
                                           cached_ab=saved.get('ab', 0), cached_hr=saved.get('hr', 0)))
            report['cache_mismatches'] = mismatches
        if 'batting_stat_totals' in tables:
            # Career display must be compared against every season, even in a season-scoped report.
            report['career_totals_mismatches'] = rows('''WITH c AS (
                SELECT player_id,SUM(ab) AS ab,SUM(homerun) AS hr FROM game_batting_stat_cache GROUP BY player_id),
                ids AS (SELECT player_id FROM c UNION SELECT player_id FROM batting_stat_totals)
                SELECT ids.player_id,c.ab AS per_game_ab,c.hr AS per_game_hr,t.ab AS career_ab,t.homerun AS career_hr
                FROM ids LEFT JOIN c USING(player_id) LEFT JOIN batting_stat_totals t USING(player_id)
                WHERE COALESCE(c.ab,0)!=COALESCE(t.ab,0) OR COALESCE(c.hr,0)!=COALESCE(t.homerun,0)''')
        if players:
            player_id = players[0]['batter_id']
            report['leader_by_game'] = rows(f'''SELECT g.season,b.game_id,g.actual_date,{measures}
                {scope} AND b.batter_id=?2 GROUP BY b.game_id ORDER BY g.season,b.game_id''', (season, player_id))
            if 'batter_info' in tables:
                report['leader_skills'] = rows('SELECT * FROM batter_info WHERE player_id=?', (player_id,))
        if extra_base:
            report['triples'] = rows(f"""SELECT g.season,b.game_id,g.actual_date,b.count_seq,b.batter_id
                {scope} AND b.result='Triple' ORDER BY b.game_id,b.count_seq""", args)
            report['triple_cache_checks'] = {}
            for table in ('game_batting_stat_cache', 'batting_stat_totals'):
                columns = {row[1] for row in conn.execute(f'PRAGMA table_info({table})')}
                report['triple_cache_checks'][table] = (
                    triple_cache_check(rows, table, season) if 'triple' in columns else None
                )
            report['extra_base'] = extra_base_audit(conn, season)
        return report


def triple_cache_check(rows, table, season):
    per_game = table == 'game_batting_stat_cache'
    keys = 'game_id,player_id' if per_game else 'player_id'
    raw_game = 'b.game_id,' if per_game else ''
    cached_game = 'c.game_id,' if per_game else ''
    cached_scope = 'JOIN game g ON g.id=c.game_id WHERE (?1 IS NULL OR g.season=?1)' if per_game else ''
    return rows(f"""WITH raw AS (
          SELECT {raw_game} b.batter_id AS player_id,COUNT(*) AS triples
          FROM player_game_batting b JOIN game g ON g.id=b.game_id
          WHERE b.result='Triple' AND (?1 IS NULL OR g.season=?1) GROUP BY {keys}
        ), cached AS (
          SELECT {cached_game} c.player_id,c.triple AS triples FROM {table} c
          {cached_scope}
        ), ids AS (
          SELECT {keys} FROM raw UNION SELECT {keys} FROM cached WHERE triples != 0
        )
        SELECT {keys},COALESCE(raw.triples,0) AS raw_triples,
               COALESCE(cached.triples,0) AS cached_triples
        FROM ids LEFT JOIN raw USING({keys}) LEFT JOIN cached USING({keys})
        WHERE COALESCE(raw.triples,0)!=COALESCE(cached.triples,0) ORDER BY {keys}
    """, (season if per_game else None,))


EV_EDGES = (20, 30, 35, 40, 45, 50)
VLA_EDGES = (-10, 0, 10, 20, 30, 40, 50)
FAIR_RESULTS = ('Single', 'Double', 'Triple', 'HomeRun', 'Out', 'FieldersChoice', 'DoublePlay')
FAIR_SQL = ','.join(f"'{result}'" for result in FAIR_RESULTS)


def compare_batters(conn, season, limit, players, batter_id, batter_line):
    """Compare the selected batter with active hitters on their current roster."""
    raw = {player['batter_id']: player for player in players}
    cache_table = 'batting_stat_totals' if season is None else 'game_batting_stat_cache'
    cache_columns = {row['name'] for row in conn.execute(f'PRAGMA table_info({cache_table})')}
    cached = {}
    if {'pa', 'ab', 'homerun', 'single', 'double', 'triple'} <= cache_columns:
        cache_source = 'batting_stat_totals c' if season is None else (
            'game_batting_stat_cache c JOIN game g ON g.id=c.game_id WHERE g.season=?'
        )
        cached = {row['player_id']: dict(row) for row in conn.execute(f'''
            SELECT c.player_id,SUM(c.pa) AS pa,SUM(c.ab) AS ab,SUM(c.homerun) AS hr,
                   SUM(c.single+c.double+c.triple+c.homerun) AS hits
            FROM {cache_source} GROUP BY c.player_id
        ''', () if season is None else (season,))}
        for line in cached.values():
            line['ba'] = line['hits'] / line['ab'] if line['ab'] else None
            line['hr_per_pa'] = line['hr'] / line['pa'] if line['pa'] else None
    if batter_line is not None:
        source = cached or raw
        matches = [key for key, row in source.items() if (row['pa'], row['hr']) == tuple(batter_line)]
        if len(matches) != 1:
            pa, hr = batter_line
            if matches:
                raise ValueError(f'{pa} PA / {hr} HR matches batter IDs {matches}; use --batter-id')
            raise ValueError(f'no batter matches {pa} PA / {hr} HR in the selected scope; use --batter-id')
        batter_id = matches[0]
    if batter_id not in raw:
        raise ValueError(f'no batting logs for batter ID {batter_id} in the selected scope')

    info = conn.execute('SELECT team_id FROM player_info WHERE id=?', (batter_id,)).fetchone()
    if info is None:
        raise ValueError(f'no roster information for batter ID {batter_id}')
    roster = {row['id']: dict(row) for row in conn.execute('''
        SELECT id,team_id,first_name,last_name FROM player_info WHERE team_id IS ?
    ''', (info['team_id'],))}
    peers = sorted(
        (pid for pid in roster if pid != batter_id and raw.get(pid, {}).get('pa', 0) > 0),
        key=lambda pid: (-raw[pid]['pa'], pid),
    )[:limit]
    ids = [batter_id, *peers]
    placeholders = ','.join('?' for _ in ids)
    skills = {row['player_id']: dict(row) for row in conn.execute(
        f'SELECT * FROM batter_info WHERE player_id IN ({placeholders})', ids
    )}
    columns = {row['name'] for row in conn.execute('PRAGMA table_info(player_game_batting)')}
    swung = 'b.decision_swung' if 'decision_swung' in columns else 'NULL'
    by_player = defaultdict(list)
    for row in conn.execute(f'''
        SELECT b.batter_id,g.stadium_id,s.name AS stadium_name,b.result,b.outbound_result,
               b.adjusted_contact_type,b.launch_speed,b.launch_angle,b.final_hla_deg,
               b.first_bounce_time,{swung} AS swung
        FROM player_game_batting b JOIN game g ON g.id=b.game_id
        LEFT JOIN stadium s ON s.id=g.stadium_id
        WHERE (? IS NULL OR g.season=?) AND b.batter_id IN ({placeholders})
    ''', (season, season, *ids)):
        by_player[row['batter_id']].append(row)

    def profile(pid):
        line = cached.get(pid)
        raw_line = raw[pid]
        return dict(player=roster[pid], skills=skills.get(pid), raw=raw_line, cached=line,
                    hr_matches_cache=(raw_line['hr'] == line['hr']) if line is not None else None,
                    batting=batting_profile(by_player[pid]))

    return dict(
        scope=dict(season=season, all_seasons=season is None, team_id=info['team_id']),
        definitions=dict(
            teammates='Other hitters on the current roster, ordered by raw PA in this scope.',
            raw_pa='Terminal batting results including BB/HBP. Raw AB/BA may differ from cache because the cache identifies sacrifice flies.',
            swings='Saved swing decision when available. Older logs infer swings from contact type or positive exit speed.',
            contact='A saved swing with positive exit speed, including fouls.',
            fair_contact='A positive-EV contact with a terminal fair batting result and non-foul trajectory.',
            stadium='Uses exposures (raw PA and fair contacts), not just home-run totals.',
            percentiles='Nearest-rank percentiles over contacts. Exit speeds are m/s; angles are degrees.',
        ),
        selected=profile(batter_id), teammates=[profile(pid) for pid in peers],
        teammate_pool=batting_profile(row for pid in peers for row in by_player[pid]),
    )


def batting_profile(records):
    counts = Counter()
    contact_types = defaultdict(Counter)
    parks = defaultdict(Counter)
    power_bins = defaultdict(Counter)
    ev, angle, hr_ev, hr_angle = [], [], [], []
    for row in records:
        result = row['result']
        park = parks[row['stadium_id'], row['stadium_name']]
        park['pa'] += result in PA_RESULTS
        park['hr'] += result == 'HomeRun'
        kind = row['adjusted_contact_type']
        decision = row['swung']
        measured = row['launch_speed'] > 0
        counts['pitches'] += 1
        counts['pa'] += result in PA_RESULTS
        counts['hr'] += result == 'HomeRun'
        counts['strikeouts'] += result == 'Strikeout'
        if result == 'HomeRun':
            counts['hr_with_prior_bounce'] += row['first_bounce_time'] is not None
            counts['hr_with_foul_launch_angle'] += abs(row['final_hla_deg']) >= 45
            counts['hr_with_other_trajectory_result'] += row['outbound_result'] != 'HomeRun'
        if decision is None:
            if measured or kind in {'SolidContact', 'WeakContact', 'MarginalContact', 'SwungAndMiss'}:
                decision = True
            elif kind == 'Take':
                decision = False
            if decision is not None:
                counts['inferred_swing_decisions'] += 1
        if decision is None:
            counts['unknown_swing_decisions'] += 1
        if not decision:
            continue
        counts['swings'] += 1
        if not measured:
            continue
        counts['contacts'] += 1
        contact_types[kind]['contacts'] += 1
        ev.append(row['launch_speed'])
        angle.append(row['launch_angle'])
        if result not in FAIR_RESULTS or row['outbound_result'] == 'Foul':
            continue
        counts['fair_contacts'] += 1
        counts['non_hr_hits'] += result in ('Single', 'Double', 'Triple')
        park['fair_contacts'] += 1
        cell = power_bins[bisect_right(EV_EDGES, row['launch_speed']), bisect_right(VLA_EDGES, row['launch_angle'])]
        cell['fair_contacts'] += 1
        if result == 'HomeRun':
            contact_types[kind]['hr'] += 1
            cell['hr'] += 1
            hr_ev.append(row['launch_speed'])
            hr_angle.append(row['launch_angle'])

    def distribution(values):
        if not values:
            return None
        values.sort()
        n = len(values)
        return dict(n=n, mean=sum(values) / n, p50=values[(50*n - 1)//100],
                    p90=values[(90*n - 1)//100], p95=values[(95*n - 1)//100], max=values[-1])

    def rate(num, denom):
        return num / denom if denom else None

    return dict(
        **{key: counts[key] for key in ('pitches','pa','swings','contacts','fair_contacts','hr','strikeouts',
                                       'inferred_swing_decisions','unknown_swing_decisions','hr_with_prior_bounce',
                                       'hr_with_foul_launch_angle','hr_with_other_trajectory_result')},
        contact_per_swing=rate(counts['contacts'], counts['swings']),
        solid_contact_per_swing=rate(contact_types.get('SolidContact', {}).get('contacts', 0), counts['swings']),
        hr_per_pa=rate(counts['hr'], counts['pa']),
        hr_per_fair_contact=rate(len(hr_ev), counts['fair_contacts']),
        strikeout_per_pa=rate(counts['strikeouts'], counts['pa']),
        non_hr_hit_per_non_hr_fair_contact=rate(counts['non_hr_hits'], counts['fair_contacts'] - len(hr_ev)),
        ev_ms=distribution(ev), launch_angle_deg=distribution(angle),
        home_run_ev_ms=distribution(hr_ev), home_run_launch_angle_deg=distribution(hr_angle),
        contact_types={kind: dict(contacts=c['contacts'], hr=c['hr']) for kind, c in sorted(contact_types.items())},
        by_stadium=[dict(stadium_id=sid, stadium_name=name, pa=c['pa'], hr=c['hr'],
                         fair_contacts=c['fair_contacts'], hr_per_pa=rate(c['hr'], c['pa']),
                         hr_per_fair_contact=rate(c['hr'], c['fair_contacts']))
                    for (sid, name), c in sorted(parks.items(), key=lambda item: (-item[1]['pa'], str(item[0][0])))],
        fair_ev_angle=[dict(ev_ms=bands(EV_EDGES, e), angle_deg=bands(VLA_EDGES, a),
                            fair_contacts=c['fair_contacts'], hr=c['hr'])
                       for (e, a), c in sorted(power_bins.items())],
    )


def bands(edges, index):
    return [edges[index - 1] if index else None, edges[index] if index < len(edges) else None]


def outcomes(counts):
    return {result: counts[result] for result in FAIR_RESULTS}


def extra_base_audit(conn, season):
    """Use first fielding and ordinary batted-ball running rows, one row per play."""
    grid = defaultdict(Counter)
    conversion = defaultdict(Counter)
    timing = defaultdict(list)
    flight = Counter()
    for hit in conn.execute(f"""
        SELECT b.result, b.launch_speed, b.launch_angle, b.outbound_result,
               b.fence_impact_time, b.first_bounce_time, b.first_bounce_distance,
               b.first_bounce_angle, b.fielder_position,
               f.time_to_field, f.catch_type, f.play_type,
               r.defense_time,
               CASE WHEN r.target_runner_id=b.batter_id THEN r.runner_time END AS batter_target_time,
               r.throw_target_base
        FROM player_game_batting b
        JOIN game g ON g.id=b.game_id
        LEFT JOIN player_game_fielding f ON f.game_id=b.game_id AND f.count_seq=b.count_seq AND f.seq=1
        LEFT JOIN player_game_running r ON r.game_id=b.game_id AND r.count_seq=b.count_seq
          AND r.seq=(SELECT MIN(saved.seq) FROM player_game_running saved
                     WHERE saved.game_id=b.game_id AND saved.count_seq=b.count_seq
                       AND saved.event='GrounderPlay')
        WHERE (?1 IS NULL OR g.season=?1) AND b.launch_speed>0
          AND b.result IN ({FAIR_SQL}) AND b.outbound_result IN ('InField','HomeRun','GroundRuleDouble')
    """, (season,)):
        result = hit['result']
        ev, vla = hit['launch_speed'], hit['launch_angle']
        grid[bisect_right(EV_EDGES, ev), bisect_right(VLA_EDGES, vla)][result] += 1

        field_time = hit['time_to_field']
        wall_time = hit['fence_impact_time']
        bounce_time = hit['first_bounce_time']
        wall_before_fielding = wall_time is not None and (field_time is None or wall_time <= field_time)
        deep_gap = (
            hit['first_bounce_distance'] is not None and hit['first_bounce_distance'] >= 75
            and hit['first_bounce_angle'] is not None and 8 <= abs(hit['first_bounce_angle']) < 18
            and bounce_time is not None and (field_time is None or bounce_time <= field_time)
        )
        if ev >= 40 and 20 <= vla < 50:
            flight['fair_contacts'] += 1
            if result == 'HomeRun':
                flight['home_runs'] += 1
            elif wall_before_fielding and (bounce_time is None or wall_time < bounce_time):
                flight['airborne_wall_impacts'] += 1

        if result == 'HomeRun':
            continue
        if hit['outbound_result'] == 'GroundRuleDouble':
            location = 'ground_rule_double'
        elif wall_before_fielding:
            location = 'wall_impact'
        elif deep_gap:
            location = 'deep_gap_first_bounce'
        elif hit['fielder_position'] in ('LF', 'CF', 'RF'):
            location = 'other_outfield'
        else:
            location = 'other_or_unknown'
        fielding = hit['catch_type'] or hit['play_type'] or 'unrecorded'
        conversion[location, fielding][result] += 1
        for metric in ('time_to_field', 'defense_time', 'batter_target_time'):
            value = hit[metric]
            if value is not None and value > 0:
                target = hit['throw_target_base'] if metric == 'batter_target_time' else None
                timing[location, fielding, result, metric, target].append(value)

    return dict(
        definitions=dict(
            population='Positive-EV terminal fair results, including HR. EV is m/s and angles are degrees. Bins are lower-inclusive and upper-exclusive; null is unbounded.',
            power_window='EV >= 40 m/s and 20 <= VLA < 50 degrees; a descriptive slice, not a home-run eligibility rule.',
            wall_impact='Saved fair wall impact at or before first recorded fielding; if fielding time is absent, timing is unverified.',
            deep_gap_first_bounce='First bounce at least 75 m from home and absolute spray angle [8,18) degrees, at or before first recorded fielding when present. Uses nominal gap sectors, not dynamic fielder positions.',
            timing='First fielding time and first GrounderPlay defense time. Batter target time is only recorded when the throw targets the batter; it is grouped by actual target base. Missing observations are omitted. These logs do not record the batter’s unattempted base-arrival times or fence-crossing height.',
        ),
        ev_vla=[
            dict(ev_ms=bands(EV_EDGES, ev), vla_deg=bands(VLA_EDGES, vla), fair_contacts=counts.total(),
                 home_run_rate=counts['HomeRun'] / counts.total(), outcomes=outcomes(counts))
            for (ev, vla), counts in sorted(grid.items())
        ],
        power_window={key: flight[key] for key in ('fair_contacts', 'home_runs', 'airborne_wall_impacts')},
        conversion=[
            dict(location=location, fielding=fielding, balls=counts.total(), outcomes=outcomes(counts))
            for (location, fielding), counts in sorted(conversion.items())
        ],
        timing_seconds=[
            dict(location=location, fielding=fielding, result=result, metric=metric, target_base=target,
                 n=len(values), mean=sum(values) / len(values), min=min(values), max=max(values))
            for (location, fielding, result, metric, target), values in sorted(timing.items())
        ],
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--season', type=int, help='Omit to audit all seasons, matching the career batting display')
    parser.add_argument('--extra-base', action='store_true', help='Include saved fair-contact and extra-base conversion diagnostics')
    parser.add_argument('--top', type=int, default=10)
    target = parser.add_mutually_exclusive_group()
    target.add_argument('--batter-id', type=int, help='Compare this batter with the --top current teammates by PA')
    target.add_argument('--batter-line', type=int, nargs=2, metavar=('PA', 'HR'),
                        help='Select the batter by displayed PA and HR, then compare with teammates')
    options = parser.parse_args()
    if options.top < 1:
        parser.error('--top must be positive')
    if options.extra_base and (options.batter_id is not None or options.batter_line is not None):
        parser.error('--extra-base is only available in the league audit')
    try:
        report = audit(db_from_env(), options.season, options.top, options.extra_base,
                       batter_id=options.batter_id, batter_line=options.batter_line)
    except (ValueError, sqlite3.Error) as exc:
        parser.exit(1, f'offense audit failed: {exc}\n')
    print(json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False))


if __name__ == '__main__':
    main()
