#!/usr/bin/env python3
"""Read-only raw-log/cache and home-run audit. Database comes from SABERBB_DB."""
import argparse
from bisect import bisect_right
from collections import Counter, defaultdict
import json
from math import isfinite
from pathlib import Path
import sqlite3

from diagnostic_db import db_from_env

AB_RESULTS = ('Single', 'Double', 'Triple', 'HomeRun', 'Strikeout', 'Out', 'FieldersChoice', 'DoublePlay')
PA_RESULTS = AB_RESULTS + ('Walk', 'HitByPitch')
AB_SQL = ','.join("'" + value + "'" for value in AB_RESULTS)
PA_SQL = ','.join("'" + value + "'" for value in PA_RESULTS)


def audit(path, season=None, limit=10, extra_base=False, *, batter_id=None, batter_line=None,
          league_batting=False, hard_contact=False, pitcher_matchups=False):
    """Never create, rebuild, or modify the supplied database or its caches."""
    uri = Path(path).expanduser().resolve().as_uri() + '?mode=ro'
    with sqlite3.connect(uri, uri=True, timeout=30) as conn:
        conn.row_factory = sqlite3.Row
        conn.execute('PRAGMA query_only=ON')
        conn.execute('BEGIN')  # Keep all report sections on one consistent read snapshot.
        if hard_contact and not league_batting:
            raise ValueError('--hard-contact requires --league-batting')
        if pitcher_matchups and not league_batting:
            raise ValueError('--pitcher-matchups requires --league-batting')
        if league_batting:
            report = league_batting_audit(conn, season, limit, pitcher_matchups=pitcher_matchups)
            if hard_contact:
                top_ids = {line['player']['id'] for line in report['top_qualified']}
                report['hard_contact'] = hard_contact_audit(conn, report['scope']['season'], top_ids)
            return report
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
NON_HR_HITS = ('Single', 'Double', 'Triple')
IN_PLAY_RESULTS = (*NON_HR_HITS, 'Out', 'FieldersChoice', 'DoublePlay')
FAIR_TRAJECTORIES = ('InField', 'HomeRun', 'GroundRuleDouble')
FAIR_TRAJECTORY_SQL = ','.join(f"'{value}'" for value in FAIR_TRAJECTORIES)
COMPLETED_REGULAR_SQL = "g.game_type='Regular' AND g.actual_date!='1900-01-01'"
VLA_TYPES = ('Grounder', 'Liner', 'Fly', 'PopUp')
VLA_TYPE_EDGES = (10, 25, 50)


def rate(numerator, denominator):
    return numerator / denominator if denominator else None


def league_batting_audit(conn, season, limit, *, pitcher_matchups=False):
    if season is None:
        season = conn.execute(f'SELECT MAX(season) FROM game g WHERE {COMPLETED_REGULAR_SQL}').fetchone()[0]
    games = conn.execute(f'''
        SELECT id, away_team_id, home_team_id FROM game g WHERE season=? AND {COMPLETED_REGULAR_SQL}
    ''', (season,)).fetchall()
    if not games:
        raise ValueError('no completed regular-season games in scope')
    team_games = Counter(team for game in games for team in (game['away_team_id'], game['home_team_id']))
    players = {row['id']: dict(row) for row in conn.execute(
        'SELECT id, team_id, first_name, last_name FROM player_info'
    )}
    batters = defaultdict(Counter)
    batted_balls = defaultdict(Counter)
    grid = defaultdict(Counter)
    power = Counter()
    games_with_logs = set()
    matchups = defaultdict(Counter)
    pitcher_column = 'b.pitcher_id,' if pitcher_matchups else ''
    for play in conn.execute(f'''
        SELECT b.game_id, b.batter_id, {pitcher_column} b.result, b.launch_speed, b.launch_angle, b.outbound_result,
               b.first_bounce_time, b.fence_impact_time, f.time_to_field,
               CASE WHEN b.result='Out' AND (
                   SELECT r.event FROM player_game_running r
                   WHERE r.game_id=b.game_id AND r.count_seq=b.count_seq ORDER BY r.seq DESC LIMIT 1
               )='TagUp' THEN EXISTS (
                   SELECT 1 FROM count c WHERE c.game_id=b.game_id AND c.seq=b.count_seq AND c.point>0
               ) ELSE 0 END AS sacrifice_fly
        FROM game g JOIN player_game_batting b ON b.game_id=g.id
        LEFT JOIN player_game_fielding f ON f.game_id=b.game_id AND f.count_seq=b.count_seq AND f.seq=1
        WHERE g.season=? AND {COMPLETED_REGULAR_SQL} AND b.result IN ({PA_SQL})
    ''', (season,)):
        games_with_logs.add(play['game_id'])
        pid, result = play['batter_id'], play['result']
        batters[pid][result] += 1
        batters[pid]['sf'] += play['sacrifice_fly']
        if pitcher_matchups:
            counts = matchups[pid, play['pitcher_id']]
            counts[result] += 1
            counts['sf'] += play['sacrifice_fly']
        ev, vla = play['launch_speed'], play['launch_angle']
        measured = ev is not None and ev > 0 and vla is not None and play['outbound_result'] in FAIR_TRAJECTORIES
        if result in IN_PLAY_RESULTS:
            kind = VLA_TYPES[bisect_right(VLA_TYPE_EDGES, vla)] if measured else 'Unknown'
            batted_balls[pid][kind, result] += 1
        if result in FAIR_RESULTS and measured:
            grid[bisect_right(EV_EDGES, ev), bisect_right(VLA_EDGES, vla)][result] += 1
            if in_power_window(ev, vla):
                power['fair_contacts'] += 1
                power['home_runs'] += result == 'HomeRun'
                power['trajectory_home_runs'] += play['outbound_result'] == 'HomeRun'
                if result != 'HomeRun' and airborne_wall_impact(play):
                    power['airborne_wall_impacts'] += 1
                    power['airborne_wall_impacts_without_fielding_time'] += play['time_to_field'] is None
    if not games_with_logs:
        raise ValueError(f'no terminal batting logs for completed regular-season games in {season}')

    qualified = []
    qualified_without_ab = 0
    totals, league_balls = Counter(), Counter()
    for pid, counts in batters.items():
        totals.update(counts)
        league_balls.update(batted_balls[pid])
        team = team_games[players.get(pid, {}).get('team_id')]
        minimum = (31 * team + 5) // 10
        line = league_batting_line(counts, batted_balls[pid])
        if not team or line['pa'] < minimum:
            continue
        if not line['ab']:
            qualified_without_ab += 1
            continue
        qualified.append(dict(player=players[pid], team_games=team, minimum_pa=minimum, **line))
    qualified.sort(key=lambda line: (-line['ba'], line['player']['id']))
    top = qualified[:limit]
    top_totals, top_balls = Counter(), Counter()
    for line in top:
        pid = line['player']['id']
        top_totals.update(batters[pid])
        top_balls.update(batted_balls[pid])

    fair_contacts = sum(counts.total() for counts in grid.values())
    report = dict(
        scope=dict(season=season, completed_regular_games=len(games), games_with_terminal_batting_logs=len(games_with_logs)),
        definitions=dict(
            qualification='3.1 PA per completed regular-season game of the current team, rounded to the nearest whole PA (halves up), matching GUI.',
            batting_stats='Terminal batting logs. An Out with a final TagUp event and a run scored is a sacrifice fly, as in the batting cache.',
            babip='(H - HR) / (AB - K - HR + SF). Includes sacrifice flies and balls without recorded launch measurements.',
            launch_angle_types='Measured fair balls in play only: Grounder <10°, Liner [10°,25°), Fly [25°,50°), PopUp >=50°. Uses saved launch angle without spin correction. Unknown counts plays without usable measurements.',
            power_window='Measured fair contact with EV >=40 m/s and VLA [20°,50°). Wall impact must precede first bounce and occur by first fielding when the fielding time is known. Saved logs do not record fence-crossing height.',
        ),
        qualified_batters=len(qualified) + qualified_without_ab,
        qualified_without_at_bats=qualified_without_ab,
        qualified_batting_avg_counts={
            'at_least_500': sum(line['ba'] >= 0.500 for line in qualified),
            '400_to_under_500': sum(0.400 <= line['ba'] < 0.500 for line in qualified),
        },
        max_home_runs=max(counts['HomeRun'] for counts in batters.values()),
        league=league_batting_line(totals, league_balls),
        top_qualified=top,
        top_qualified_pool=league_batting_line(top_totals, top_balls),
        league_power=dict(
            measured_fair_contacts=fair_contacts,
            **{key: power[key] for key in ('fair_contacts', 'home_runs', 'trajectory_home_runs',
                                           'airborne_wall_impacts', 'airborne_wall_impacts_without_fielding_time')},
            window_share_of_measured_fair=rate(power['fair_contacts'], fair_contacts),
            home_runs_per_window_contact=rate(power['home_runs'], power['fair_contacts']),
        ),
        league_ev_vla=ev_vla_grid(grid),
    )

    if pitcher_matchups:
        report['pitcher_matchups'] = pitcher_matchup_report(
            conn, season, len(games), team_games, players, qualified, matchups,
        )
    return report


PITCHER_COHORTS = ('qualified_era_under_2', 'qualified_era_2_to_under_4', 'qualified_era_at_least_4',
                   'unqualified', 'unknown')


def pitcher_matchup_report(conn, season, completed_games, team_games, players, qualified, matchups):
    cache_games = conn.execute(f"""
        SELECT COUNT(DISTINCT c.game_id)
        FROM game_pitching_stat_cache c JOIN game g ON g.id=c.game_id
        WHERE g.season=? AND {COMPLETED_REGULAR_SQL}
    """, (season,)).fetchone()[0]
    if not cache_games:
        raise ValueError(f'no cached pitching lines for completed regular-season games in {season}')
    pitcher_groups = {}
    pitching = {group: Counter() for group in (*PITCHER_COHORTS, 'league', 'qualified')}
    qualified_pitchers = []
    for row in conn.execute(f"""
        SELECT c.player_id, SUM(c.outs) AS outs, SUM(c.runs) AS runs,
               SUM(c.earned_runs) AS earned_runs, SUM(c.unclassified_runs) AS unclassified_runs
        FROM game_pitching_stat_cache c JOIN game g ON g.id=c.game_id
        WHERE g.season=? AND {COMPLETED_REGULAR_SQL}
        GROUP BY c.player_id
    """, (season,)):
        pid = row['player_id']
        team = team_games[players.get(pid, {}).get('team_id')]
        line = Counter({key: row[key] for key in ('outs', 'runs', 'earned_runs', 'unclassified_runs')})
        line['pitchers'] = 1
        if not team:
            group = 'unknown'
        elif line['outs'] < 3 * team:
            group = 'unqualified'
        else:
            era_runs = 27 * line['earned_runs']
            if era_runs < 2 * line['outs']:
                group = 'qualified_era_under_2'
            elif era_runs < 4 * line['outs']:
                group = 'qualified_era_2_to_under_4'
            else:
                group = 'qualified_era_at_least_4'
            pitching['qualified'].update(line)
            qualified_pitchers.append(dict(player=players[pid], team_games=team, **pitching_matchup_line(line)))
        pitcher_groups[pid] = group
        pitching[group].update(line)
        pitching['league'].update(line)

    by_batter = defaultdict(Counter)
    league_batting = defaultdict(Counter)
    for (pid, pitcher_id), counts in matchups.items():
        group = pitcher_groups.get(pitcher_id, 'unknown')
        by_batter[pid, group].update(counts)
        league_batting[group].update(counts)
    league_outs = pitching['league']['outs']
    qualified_pitchers.sort(key=lambda row: (row['era'], row['player']['id']))
    return dict(
        definitions=dict(
            pitcher_qualification='At least one inning (three outs) per completed regular-season game of the current team, matching the GUI.',
            pitcher_cohorts='Use the season pitching cache and unrounded ERA for qualified tiers. Unknown means the current team has no games in scope or the opposing pitcher has no season pitching line.',
            interpretation='Observed terminal batting results against the recorded pitcher. Season ERA includes these same matchups. Neither player selection nor game context is controlled.',
            innings='Innings are reported as integer outs. Cohort RA9 uses all charged runs; ERA uses cached earned runs.',
            coverage='Partial pitching cache coverage can change qualification and ERA. Batting splits include all terminal batting logs, with unmatched opponents in the unknown cohort.',
        ),
        completed_games_with_pitching_cache=cache_games,
        pitching_cache_covers_all_completed_games=cache_games == completed_games,
        pitching_by_cohort={
            group: dict(**pitching_matchup_line(line), share_of_league_outs=rate(line['outs'], league_outs))
            for group, line in pitching.items()
        },
        qualified_pitchers=qualified_pitchers,
        league_batting_by_pitcher_cohort={
            group: matchup_batting_line(league_batting[group]) for group in PITCHER_COHORTS
        },
        qualified_batter_matchups=[
            dict(player=line['player'], pa=line['pa'], ba=line['ba'], strikeouts_per_ab=line['strikeouts_per_ab'],
                 babip=line['babip'], hr=line['hr'],
                 by_pitcher_cohort={
                     group: matchup_batting_line(by_batter[line['player']['id'], group])
                     for group in PITCHER_COHORTS
                 })
            for line in qualified
        ],
        qualified_batting_avg_distribution={
            'below_200': sum(line['ba'] < .200 for line in qualified),
            '200_to_under_250': sum(.200 <= line['ba'] < .250 for line in qualified),
            '250_through_300': sum(.250 <= line['ba'] <= .300 for line in qualified),
            'over_300_to_under_400': sum(.300 < line['ba'] < .400 for line in qualified),
            '400_to_under_500': sum(.400 <= line['ba'] < .500 for line in qualified),
            'at_least_500': sum(line['ba'] >= .500 for line in qualified),
        },
    )


def pitching_matchup_line(counts):
    return dict(
        **{key: counts[key] for key in ('pitchers', 'outs', 'runs', 'earned_runs', 'unclassified_runs')},
        era=rate(27 * counts['earned_runs'], counts['outs']),
        ra9=rate(27 * counts['runs'], counts['outs']),
    )


def batting_outcome_rates(counts):
    ab = sum(counts[result] for result in AB_RESULTS) - counts['sf']
    non_hr_hits = sum(counts[result] for result in NON_HR_HITS)
    hr, k = counts['HomeRun'], counts['Strikeout']
    hits = non_hr_hits + hr
    bip = ab - k - hr + counts['sf']
    return dict(
        pa=sum(counts[result] for result in PA_RESULTS),
        ab=ab, hits=hits, ba=rate(hits, ab), strikeouts=k, strikeouts_per_ab=rate(k, ab),
        singles=counts['Single'], doubles=counts['Double'], triples=counts['Triple'], hr=hr,
        sacrifice_flies=counts['sf'], balls_in_play=bip, babip=rate(non_hr_hits, bip),
    )


def matchup_batting_line(counts):
    line = batting_outcome_rates(counts)
    return dict(**line, walks=counts['Walk'], walks_per_pa=rate(counts['Walk'], line['pa']),
                hr_per_pa=rate(line['hr'], line['pa']))


def league_batting_line(counts, batted_balls):
    by_type = {}
    for kind in (*VLA_TYPES, 'Unknown'):
        results = {result: batted_balls[kind, result] for result in IN_PLAY_RESULTS}
        balls = sum(results.values())
        type_hits = sum(results[result] for result in NON_HR_HITS)
        by_type[kind] = dict(balls=balls, hits=type_hits, hit_rate=rate(type_hits, balls), outcomes=results)
    return dict(**batting_outcome_rates(counts), bip_by_launch_angle=by_type)


HARD_EV_EDGES = (45, 47.5, 50)
LOW_VLA_EDGES = (0, 2.5, 5, 7.5, 10)
HARD_VLA_EDGES = (-10, 0, 10, 20, 30, 40, 50)
HLA_EDGES = (-30, -20, -10, 0, 10, 20, 30)
BOUNCE_DISTANCE_EDGES = (10, 20, 30, 40, 50, 60)
CONTACT_Z_CM_EDGES = (-4, -3, -2, -1, -0.5, 0, 0.5, 1, 2, 3, 4)
HARD_CONTACT_COHORTS = ('top_qualified', 'other_batters')
VERTICAL_INPUTS = ('contact_offset_z_m', 'adapted_vertical_offset_m', 'swing_error_z_m')


def observed_number(value):
    return value is not None and isfinite(value)


def measured_band(edges, value):
    return bisect_right(edges, value) if observed_number(value) else -1


def labeled_band(edges, band):
    return bands(edges, band) if band >= 0 else None


def summary(values):
    values = sorted(value for value in values if observed_number(value))
    n = len(values)
    return dict(n=n, mean=rate(sum(values), n), p50=values[(n - 1) // 2] if n else None)


def hard_contact_rows(conn, season):
    optional = []
    for alias, table, names in (
        ('b', 'player_game_batting', VERTICAL_INPUTS),
        ('f', 'player_game_fielding', ('interception_fielder_position', 'selection_path', 'catch_type')),
    ):
        columns = {row['name'] for row in conn.execute(f'PRAGMA table_info({table})')}
        optional.extend(f'{alias}.{name}' if name in columns else f'NULL AS {name}' for name in names)
    return conn.execute(f'''
        SELECT b.batter_id, b.result, b.launch_speed, b.launch_angle, b.final_hla_deg,
               b.first_bounce_time, b.first_bounce_distance, b.first_bounce_angle,
               f.seq IS NOT NULL AS has_fielding, f.catch_fielder_position, f.time_to_field,
               r.seq IS NOT NULL AS has_primary_play, r.throw_target_base, r.target_runner_id,
               r.defense_time, r.runner_time, r.ruling, {', '.join(optional)}
        FROM game g JOIN player_game_batting b ON b.game_id=g.id
        LEFT JOIN player_game_fielding f ON f.game_id=b.game_id AND f.count_seq=b.count_seq AND f.seq=1
        LEFT JOIN player_game_running r ON r.game_id=b.game_id AND r.count_seq=b.count_seq
          AND r.seq=(SELECT MIN(saved.seq) FROM player_game_running saved
                     WHERE saved.game_id=b.game_id AND saved.count_seq=b.count_seq AND saved.event='GrounderPlay')
        WHERE g.season=? AND {COMPLETED_REGULAR_SQL}
          AND b.launch_speed>=45 AND b.launch_angle IS NOT NULL
          AND b.result IN ({FAIR_SQL}) AND b.outbound_result IN ({FAIR_TRAJECTORY_SQL})
    ''', (season,))


def first_base_phase(play, field):
    if not play['has_primary_play']:
        return 'no_primary_grounder_play', {}
    if not play['throw_target_base']:
        return 'base_unrecorded', {}
    if play['throw_target_base'] != 'First':
        return 'other_base', {}
    if play['target_runner_id'] != play['batter_id']:
        return 'first_base_target_unverified', {}
    runner, defense = play['runner_time'], play['defense_time']
    if not (observed_number(runner) and runner > 0 and observed_number(defense) and defense >= 0):
        return 'first_base_times_missing', {}
    margin = defense - runner
    if (field is not None and defense < field) or play['ruling'] != ('Safe' if margin >= 0 else 'Out'):
        return 'first_base_times_inconsistent', {}
    times = dict(batter_first_sec=runner, defense_first_sec=defense, defense_minus_batter_sec=margin)
    if field is not None:
        times['defense_after_field_sec'] = defense - field
    if margin < 0:
        return 'defense_before_batter', times
    if field is None:
        return 'safe_field_time_missing', times
    if field >= runner:
        return 'fielding_completed_at_or_after_batter', times
    return 'delivery_completed_at_or_after_batter', times


def bounce_checkpoint(play, field):
    bounce = play['first_bounce_time']
    if not observed_number(bounce) or bounce < 0:
        return 'no_saved_bounce', -1, -1
    if field is not None and bounce > field:
        return 'projected_after_fielding', -1, -1
    status = 'by_fielding' if field is not None else 'field_time_unrecorded'
    return (status, measured_band(BOUNCE_DISTANCE_EDGES, play['first_bounce_distance']),
            measured_band(HLA_EDGES, play['first_bounce_angle']))


def low_contact_line(counts):
    n = counts.total()
    return dict(balls=n, singles=counts['Single'], single_rate=rate(counts['Single'], n),
                outcomes={result: counts[result] for result in IN_PLAY_RESULTS})


def shared_geometry_rates(geometry):
    top, others = (geometry[cohort] for cohort in HARD_CONTACT_COHORTS)
    shared = sorted(key for key in top.keys() & others.keys() if key[2] >= 0)
    shared_balls = sum(top[key].total() + others[key].total() for key in shared)
    return dict(cells=len(shared), **{
        cohort: dict(
            shared_balls=sum(groups[key].total() for key in shared),
            total_balls=sum(cell.total() for cell in groups.values()),
            standardized_single_rate=rate(sum(
                (top[key].total() + others[key].total()) * groups[key]['Single'] / groups[key].total()
                for key in shared
            ), shared_balls),
        )
        for cohort, groups in geometry.items()
    })


def hard_contact_audit(conn, season, top_ids):
    geometry = {cohort: defaultdict(Counter) for cohort in HARD_CONTACT_COHORTS}
    bounces = defaultdict(Counter)
    fielding = defaultdict(list)
    vertical_angles = defaultdict(list)
    vertical_offsets = defaultdict(list)
    for play in hard_contact_rows(conn, season):
        ev, angle = play['launch_speed'], play['launch_angle']
        result = play['result']
        cohort = 'top_qualified' if play['batter_id'] in top_ids else 'other_batters'
        ev_band = measured_band(HARD_EV_EDGES, ev)
        z = play['contact_offset_z_m']
        z_band = measured_band(CONTACT_Z_CM_EDGES, 100 * z if observed_number(z) else None)
        for cohort_key in ('league', cohort):
            vertical_angles[cohort_key, measured_band(HARD_VLA_EDGES, angle)].append(play)
            vertical_offsets[cohort_key, ev_band, z_band].append(play)
        if result not in IN_PLAY_RESULTS or not 0 <= angle < 10:
            continue
        angle_band = measured_band(LOW_VLA_EDGES, angle)
        hla_band = measured_band(HLA_EDGES, play['final_hla_deg'])
        geometry[cohort][ev_band, angle_band, hla_band][result] += 1
        field = play['time_to_field']
        field = field if observed_number(field) and field >= 0 else None
        bounces[(cohort, *bounce_checkpoint(play, field))][result] += 1
        phase, times = first_base_phase(play, field)
        position = play['interception_fielder_position'] or play['catch_fielder_position']
        key = (cohort, result, bool(play['has_fielding']), position or 'Unknown',
               play['selection_path'] or 'Unknown', play['catch_type'] or 'Unknown', phase)
        fielding[key].append(dict(field_complete_sec=field, **times))

    return dict(
        definitions=dict(
            low_angle='Non-HR terminal fair balls in play with EV >=45 m/s and saved VLA [0°,10°).',
            geometry='Signed HLA is field-relative. Shared cells require known HLA and at least one ball per cohort. Both standardized rates use pooled cell weights. Counts show shared coverage.',
            first_bounce='Saved first bounce by the first fielding completion. A projected bounce after fielding carries no location in this report. When fielding time is absent, bounce timing is unverified.',
            first_base='First saved GrounderPlay only. An observed first-base play must target the batter. Positive defense-minus-batter time means the batter was earlier; a tie is safe. Time after fielding includes transfer, throw, receipt, or carry. Missing plays have no inferred timing.',
            vertical_contact='Terminal measured fair contact with EV >=45 m/s, including HR. Signed contact Z in centimeters is positive toward higher VLA in the live launch calculation. Null bands identify missing observations.',
        ),
        low_angle_by_geometry=[
            dict(cohort=cohort, ev_ms=bands(HARD_EV_EDGES, ev), vla_deg=bands(LOW_VLA_EDGES, vla),
                 hla_deg=labeled_band(HLA_EDGES, hla), **low_contact_line(counts))
            for cohort, cells in geometry.items() for (ev, vla, hla), counts in sorted(cells.items())
        ],
        low_angle_shared_geometry=shared_geometry_rates(geometry),
        low_angle_by_first_bounce=[
            dict(cohort=cohort, phase=phase, distance_m=labeled_band(BOUNCE_DISTANCE_EDGES, distance),
                 angle_deg=labeled_band(HLA_EDGES, angle), **low_contact_line(counts))
            for (cohort, phase, distance, angle), counts in sorted(bounces.items())
        ],
        low_angle_by_fielding_and_first_base=[
            dict(cohort=cohort, result=result, fielding_recorded=recorded, fielder_position=position,
                 selection_path=path, catch_type=catch, first_base_phase=phase, balls=len(records),
                 timing_seconds={metric: summary(row.get(metric) for row in records)
                                 for metric in ('field_complete_sec', 'batter_first_sec', 'defense_first_sec',
                                                'defense_minus_batter_sec', 'defense_after_field_sec')})
            for (cohort, result, recorded, position, path, catch, phase), records in sorted(fielding.items())
        ],
        hard_by_launch_angle=[
            dict(cohort=cohort, vla_deg=bands(HARD_VLA_EDGES, vla), balls=len(plays),
                 home_runs=sum(row['result'] == 'HomeRun' for row in plays),
                 vertical_cm={name.removesuffix('_m'): summary(
                     100 * row[name] for row in plays if observed_number(row[name])
                 ) for name in VERTICAL_INPUTS})
            for (cohort, vla), plays in sorted(vertical_angles.items())
        ],
        hard_by_vertical_offset=[
            dict(cohort=cohort, ev_ms=bands(HARD_EV_EDGES, ev), contact_offset_z_cm=labeled_band(CONTACT_Z_CM_EDGES, z),
                 balls=len(plays), launch_angle_deg=summary(row['launch_angle'] for row in plays),
                 vla_at_least_20=sum(row['launch_angle'] >= 20 for row in plays),
                 vla_at_least_30=sum(row['launch_angle'] >= 30 for row in plays),
                 home_runs=sum(row['result'] == 'HomeRun' for row in plays))
            for (cohort, ev, z), plays in sorted(vertical_offsets.items())
        ],
    )


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


def ev_vla_grid(grid):
    return [
        dict(ev_ms=bands(EV_EDGES, ev), vla_deg=bands(VLA_EDGES, vla), fair_contacts=counts.total(),
             home_run_rate=counts['HomeRun'] / counts.total(), outcomes=outcomes(counts))
        for (ev, vla), counts in sorted(grid.items())
    ]


def in_power_window(ev, vla):
    return ev >= 40 and 20 <= vla < 50


def wall_before_fielding(hit):
    wall_time, field_time = hit['fence_impact_time'], hit['time_to_field']
    return wall_time is not None and (field_time is None or wall_time <= field_time)


def airborne_wall_impact(hit):
    bounce_time = hit['first_bounce_time']
    return wall_before_fielding(hit) and (bounce_time is None or hit['fence_impact_time'] < bounce_time)


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
        bounce_time = hit['first_bounce_time']
        deep_gap = (
            hit['first_bounce_distance'] is not None and hit['first_bounce_distance'] >= 75
            and hit['first_bounce_angle'] is not None and 8 <= abs(hit['first_bounce_angle']) < 18
            and bounce_time is not None and (field_time is None or bounce_time <= field_time)
        )
        if in_power_window(ev, vla):
            flight['fair_contacts'] += 1
            if result == 'HomeRun':
                flight['home_runs'] += 1
            elif airborne_wall_impact(hit):
                flight['airborne_wall_impacts'] += 1

        if result == 'HomeRun':
            continue
        if hit['outbound_result'] == 'GroundRuleDouble':
            location = 'ground_rule_double'
        elif wall_before_fielding(hit):
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
        ev_vla=ev_vla_grid(grid),
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
    parser.add_argument('--season', type=int, help='Defaults to the latest completed regular season with --league-batting, otherwise all seasons')
    parser.add_argument('--extra-base', action='store_true', help='Include saved fair-contact and extra-base conversion diagnostics')
    parser.add_argument('--hard-contact', action='store_true',
                        help='With --league-batting, diagnose low-angle fielding and vertical contact at EV >=45 m/s')
    parser.add_argument('--pitcher-matchups', action='store_true',
                        help='With --league-batting, split batting by opposing pitcher cohort and report league pitching workload')
    parser.add_argument('--top', type=int, default=10)
    target = parser.add_mutually_exclusive_group()
    target.add_argument('--batter-id', type=int, help='Compare this batter with the --top current teammates by PA')
    target.add_argument('--batter-line', type=int, nargs=2, metavar=('PA', 'HR'),
                        help='Select the batter by displayed PA and HR, then compare with teammates')
    target.add_argument('--league-batting', action='store_true',
                        help='Compare the --top qualified batting-average leaders with the league')
    options = parser.parse_args()
    if options.top < 1:
        parser.error('--top must be positive')
    if options.extra_base and (options.batter_id is not None or options.batter_line is not None):
        parser.error('--extra-base is only available in the league audit')
    if options.extra_base and options.league_batting:
        parser.error('--extra-base and --league-batting are independent reports; choose one')
    if options.hard_contact and not options.league_batting:
        parser.error('--hard-contact requires --league-batting')
    if options.pitcher_matchups and not options.league_batting:
        parser.error('--pitcher-matchups requires --league-batting')
    try:
        report = audit(db_from_env(), options.season, options.top, options.extra_base,
                       batter_id=options.batter_id, batter_line=options.batter_line,
                       league_batting=options.league_batting, hard_contact=options.hard_contact,
                       pitcher_matchups=options.pitcher_matchups)
    except (ValueError, sqlite3.Error) as exc:
        parser.exit(1, f'offense audit failed: {exc}\n')
    print(json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False))


if __name__ == '__main__':
    main()
