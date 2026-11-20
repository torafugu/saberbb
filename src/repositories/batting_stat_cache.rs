//! Replay the saved play log for batting and pitching statistics.
use super::pitching_stat_cache::GamePitching;
use crate::domain::shared::{ball::BallLocation, game::PitchResult, player::RL};
use anyhow::Result;
use rusqlite::{Connection, params};
use std::collections::BTreeMap;

pub(super) const COUNTS: &str = "ab,single,double,triple,homerun,rbi,games,pa,runs,so,bb,hbp,sf,sb,cs,gidp,risp_hits,risp_ab,errors";

#[derive(Default)]
struct Line {
    ab: u32,
    single: u32,
    double: u32,
    triple: u32,
    homerun: u32,
    rbi: u32,
    pa: u32,
    runs: u32,
    so: u32,
    bb: u32,
    hbp: u32,
    sf: u32,
    sb: u32,
    cs: u32,
    gidp: u32,
    risp_hits: u32,
    risp_ab: u32,
    errors: u32,
}

type Bases = [Option<i64>; 3];

pub(super) fn fill(conn: &Connection, game: Option<u32>) -> Result<()> {
    if let Some(game) = game {
        return fill_game(conn, game);
    }
    let games = conn
        .prepare(
            "SELECT DISTINCT game_id FROM player_game_entry
             UNION SELECT DISTINCT game_id FROM player_game_batting
             UNION SELECT DISTINCT game_id FROM player_game_pitching
             UNION SELECT DISTINCT game_id FROM player_game_pitching_decision
             UNION SELECT DISTINCT game_id FROM player_game_running
             UNION SELECT DISTINCT game_id FROM player_game_fielding",
        )?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<u32>>>()?;
    for game in games {
        fill_game(conn, game)?;
    }
    Ok(())
}

fn fill_game(conn: &Connection, game: u32) -> Result<()> {
    let mut lines = BTreeMap::<i64, Line>::new();
    let mut pitching = GamePitching::default();
    let mut entries =
        conn.prepare_cached("SELECT DISTINCT player_id FROM player_game_entry WHERE game_id=?1")?;
    for player in entries.query_map([game], |row| row.get(0))? {
        lines.entry(player?).or_default();
    }

    // The fielding record's catch_fielder_id can be the throw recipient.
    // Resolve the recorded interception position against the defense on that pitch.
    let mut errors = conn.prepare_cached(
        "SELECT (
            SELECT e.player_id FROM player_game_entry e
            JOIN player_info p ON p.id=e.player_id
            WHERE e.game_id=f.game_id
              AND e.position=f.interception_fielder_position
              AND e.start_count_seq<=f.count_seq
              AND (e.end_count_seq=0 OR e.end_count_seq>=f.count_seq)
              AND p.team_id=CASE c.inning_tb WHEN 'Top' THEN g.home_team_id WHEN 'Bottom' THEN g.away_team_id END
            ORDER BY e.start_count_seq DESC LIMIT 1
         ) FROM player_game_fielding f
         JOIN count c ON c.game_id=f.game_id AND c.seq=f.count_seq
         JOIN game g ON g.id=f.game_id
         WHERE f.game_id=?1 AND f.error_type IS NOT NULL AND f.error_type NOT IN ('','None')",
    )?;
    for fielder in errors.query_map([game], |row| row.get::<_, Option<i64>>(0))? {
        if let Some(fielder) = fielder? {
            lines.entry(fielder).or_default().errors += 1;
        }
    }

    let mut plays = conn.prepare_cached(
        "WITH counts AS MATERIALIZED (
            SELECT seq,inning_seq,inning_tb,point,out FROM count WHERE game_id=?1
         ), pitch AS (
            SELECT seq FROM counts
            UNION SELECT count_seq FROM player_game_batting WHERE game_id=?1
            UNION SELECT count_seq FROM player_game_running WHERE game_id=?1
            UNION SELECT count_seq FROM player_game_pitching WHERE game_id=?1
         ), running AS (
            SELECT *,
                ROW_NUMBER() OVER(PARTITION BY count_seq ORDER BY seq DESC) AS last
            FROM player_game_running WHERE game_id=?1
         )
         SELECT c.inning_seq,c.inning_tb,COALESCE(c.point,0),b.batter_id,b.result,
                r.event,r.runner_1st_id,r.runner_2nd_id,r.runner_3rd_id,r.target_runner_id,r.ruling,
                s.target_runner_id,s.ruling,s.runner_1st_id,s.runner_2nd_id,s.runner_3rd_id,
                first.target_runner_id,first.ruling,p.actual_location_x,p.actual_location_y,
                COALESCE(p.pitcher_id,b.pitcher_id),c.out,
                EXISTS(SELECT 1 FROM player_game_fielding f WHERE f.game_id=?1 AND f.count_seq=k.seq
                       AND f.error_type IS NOT NULL AND f.error_type NOT IN ('','None'))
         FROM pitch k
         LEFT JOIN counts c ON c.seq=k.seq
         LEFT JOIN player_game_batting b ON b.game_id=?1 AND b.count_seq=k.seq
         LEFT JOIN player_game_pitching p ON p.game_id=?1 AND p.count_seq=k.seq
         LEFT JOIN running r ON r.count_seq=k.seq AND r.last=1
         LEFT JOIN running s ON s.count_seq=k.seq AND s.event='BaseSteal'
         LEFT JOIN running first ON first.count_seq=k.seq AND first.event='GrounderPlay' AND r.event='DoublePlay'
         ORDER BY k.seq",
    )?;
    let mut pitches = plays.query([game])?;
    let mut half = (None, None);
    let mut bases: Bases = [None; 3];
    while let Some(row) = pitches.next()? {
        let current_half = (
            row.get::<_, Option<u32>>(0)?,
            row.get::<_, Option<String>>(1)?,
        );
        if current_half != half {
            bases = [None; 3];
            pitching.start_half();
            half = current_half;
        }
        let pitcher = pitching.record_pitch(
            row.get(20)?,
            half.0,
            half.1.as_deref(),
            row.get(21)?,
            row.get(22)?,
        );
        let points: u32 = row.get(2)?;
        let batter: Option<i64> = row.get(3)?;
        let result: Option<String> = row.get(4)?;
        let event: Option<String> = row.get(5)?;
        let recorded_bases = [row.get(6)?, row.get(7)?, row.get(8)?];
        let target: Option<i64> = row.get(9)?;
        let ruling: Option<String> = row.get(10)?;

        if let Some(stealer) = row.get::<_, Option<i64>>(11)? {
            let line = lines.entry(stealer).or_default();
            match row.get::<_, Option<String>>(12)?.as_deref() {
                Some("Safe") => {
                    line.sb += 1;
                    bases = [row.get(13)?, row.get(14)?, row.get(15)?];
                }
                // The simulator leaves the runner in place on a failed steal.
                Some("Out") => line.cs += 1,
                _ => {}
            }
        }

        let result = result.as_deref().unwrap_or_default();
        let event = event.as_deref().unwrap_or_default();
        let before = bases;
        let sacrifice_fly = result == "Out" && event == "TagUp" && points > 0;
        let wild_ball_four = result == "Walk"
            && matches!(
                (row.get::<_, Option<f64>>(18)?, row.get::<_, Option<f64>>(19)?),
                (Some(x), Some(y)) if matches!((BallLocation { x, y }).call(RL::Right), PitchResult::WildPitch)
            );
        let mut scorers = Vec::new();
        match (result, event) {
            ("HomeRun", _) => {
                scorers.extend(before.into_iter().flatten());
                scorers.extend(batter);
                bases = [None; 3];
            }
            ("Walk" | "HitByPitch", _) if !wild_ball_four => {
                if before.iter().all(Option::is_some) {
                    scorers.extend(before[2]);
                }
                bases = [
                    batter,
                    before[0].or(before[1]),
                    if before[0].is_some() && before[1].is_some() {
                        before[1]
                    } else {
                        before[2]
                    },
                ];
            }
            ("WildPitch" | "Walk", _) => {
                scorers.extend(before[2]);
                bases = [
                    if wild_ball_four { batter } else { None },
                    before[0],
                    before[1],
                ];
            }
            (_, "GrounderPlay" | "DoublePlay") => {
                let retired = (ruling.as_deref() == Some("Out"))
                    .then_some(target)
                    .flatten();
                let first_retired = if row.get::<_, Option<String>>(17)?.as_deref() == Some("Out") {
                    row.get::<_, Option<i64>>(16)?
                } else {
                    None
                };
                scorers.extend(before.into_iter().rev().flatten().filter(|id| {
                    !recorded_bases.contains(&Some(*id))
                        && Some(*id) != retired
                        && Some(*id) != first_retired
                }));
                bases = recorded_bases;
            }
            ("Double", "") => {
                // A ground-rule double has no separate running record.
                scorers.extend(before[1..].iter().copied().flatten());
                bases = [None, batter, before[0]];
            }
            (_, "TagUp") => {
                // Scoring is saved, but the current simulator does not commit the tag-up snapshot.
                scorers.extend(before[2]);
            }
            _ => {}
        }
        for &scorer in scorers.iter().take(points as usize) {
            lines.entry(scorer).or_default().runs += 1;
        }

        if let Some(batter) = batter {
            let line = lines.entry(batter).or_default();
            let hit = matches!(result, "Single" | "Double" | "Triple" | "HomeRun");
            let at_bat = !sacrifice_fly
                && (hit
                    || matches!(
                        result,
                        "Strikeout" | "Out" | "FieldersChoice" | "DoublePlay"
                    ));
            if let Some(pitcher) = pitcher {
                let replaced_runner =
                    if result == "FieldersChoice" && ruling.as_deref() == Some("Out") {
                        target
                    } else {
                        None
                    };
                pitching.record_batter(
                    pitcher,
                    batter,
                    result,
                    at_bat,
                    hit,
                    wild_ball_four,
                    replaced_runner,
                );
            }
            line.ab += u32::from(at_bat);
            line.pa +=
                u32::from(at_bat || sacrifice_fly || matches!(result, "Walk" | "HitByPitch"));
            line.single += u32::from(result == "Single");
            line.double += u32::from(result == "Double");
            line.triple += u32::from(result == "Triple");
            line.homerun += u32::from(result == "HomeRun");
            line.so += u32::from(result == "Strikeout");
            line.bb += u32::from(result == "Walk");
            line.hbp += u32::from(result == "HitByPitch");
            line.sf += u32::from(sacrifice_fly);
            line.gidp += u32::from(result == "DoublePlay");
            if at_bat && (before[1].is_some() || before[2].is_some()) {
                line.risp_ab += 1;
                line.risp_hits += u32::from(hit);
            }
            if result != "DoublePlay"
                && result != "WildPitch"
                && (!wild_ball_four || before.iter().all(Option::is_some))
            {
                line.rbi += points;
            }
        }
        if points > 0
            && let Some(pitcher) = pitcher
        {
            pitching.record_runs(pitcher, points, &scorers);
        }
    }

    let mut insert = conn.prepare_cached(&format!(
        "INSERT INTO game_batting_stat_cache(game_id,player_id,{COUNTS})
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,1,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)"
    ))?;
    for (player, s) in lines {
        insert.execute(params![
            game,
            player,
            s.ab,
            s.single,
            s.double,
            s.triple,
            s.homerun,
            s.rbi,
            s.pa,
            s.runs,
            s.so,
            s.bb,
            s.hbp,
            s.sf,
            s.sb,
            s.cs,
            s.gidp,
            s.risp_hits,
            s.risp_ab,
            s.errors,
        ])?;
    }
    pitching.save(conn, game)
}
