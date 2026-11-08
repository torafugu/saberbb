#!/usr/bin/env python3
"""Read-only raw-log/cache and home-run audit. Database comes from SABERBB_DB."""
import argparse
import json
from pathlib import Path
import sqlite3

from diagnostic_db import db_from_env

AB_RESULTS = ('Single', 'Double', 'Triple', 'HomeRun', 'Strikeout', 'Out', 'FieldersChoice', 'DoublePlay')
PA_RESULTS = AB_RESULTS + ('Walk', 'HitByPitch')
AB_SQL = ','.join("'" + value + "'" for value in AB_RESULTS)
PA_SQL = ','.join("'" + value + "'" for value in PA_RESULTS)


def audit(path, season=None, limit=10):
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
        return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--season', type=int, help='Omit to audit all seasons, matching the career batting display')
    parser.add_argument('--top', type=int, default=10)
    options = parser.parse_args()
    if options.top < 1:
        parser.error('--top must be positive')
    try:
        report = audit(db_from_env(), options.season, options.top)
    except (ValueError, sqlite3.Error) as exc:
        parser.exit(1, f'offense audit failed: {exc}\n')
    print(json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False))


if __name__ == '__main__':
    main()
