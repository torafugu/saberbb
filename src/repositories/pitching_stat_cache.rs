//! Accumulate pitching statistics alongside the batting cache's saved-play replay.
use crate::domain::shared::{
    game::GameSchedule,
    stats::{SeasonPitchingLine, SeasonPitchingStats},
};
use anyhow::Result;
use rusqlite::{Connection, params};
use std::collections::{BTreeMap, HashMap};

pub(super) const COUNTS: &str = "games,outs,runs,wins,losses,saves,holds,so,bb,starts,complete_games,shutouts,quality_starts,uncertain_starts,relief_wins,hits,home_runs,hbp,wild_pitches,earned_runs,unclassified_runs,at_bats";

pub(super) fn load_season_before_game(
    conn: &Connection,
    game: &GameSchedule,
) -> Result<SeasonPitchingStats> {
    let mut query = conn.prepare_cached(
        "SELECT CASE WHEN p.team_id IN (?1,?2) THEN c.player_id END AS roster_pitcher,
                SUM(c.outs), SUM(c.runs)
         FROM game_pitching_stat_cache c
         JOIN game g ON g.id=c.game_id
         LEFT JOIN player_info p ON p.id=c.player_id
         WHERE g.season=?3 AND g.game_type='Regular' AND g.actual_date!='1900-01-01'
           AND g.id!=?6 AND (g.actual_date,COALESCE(g.seq,0),g.id)<(?4,?5,?6)
         GROUP BY roster_pitcher",
    )?;
    let mut rows = query.query(params![
        game.away_team.id,
        game.home_team.id,
        game.season,
        game.planned_date,
        game.seq,
        game.id,
    ])?;
    let mut stats = SeasonPitchingStats::default();
    while let Some(row) = rows.next()? {
        let line = SeasonPitchingLine {
            outs: row.get(1)?,
            runs: row.get(2)?,
        };
        stats.season_total.outs += line.outs;
        stats.season_total.runs += line.runs;
        if let Some(player_id) = row.get::<_, Option<i64>>(0)? {
            stats.pitchers.insert(player_id, line);
        }
    }
    Ok(stats)
}

#[derive(Default)]
struct Line {
    appeared: bool,
    started: bool,
    outs: u32,
    runs: u32,
    wins: u32,
    losses: u32,
    saves: u32,
    holds: u32,
    so: u32,
    bb: u32,
    hits: u32,
    home_runs: u32,
    hbp: u32,
    wild_pitches: u32,
    unclassified_runs: u32,
    at_bats: u32,
}

#[derive(Default)]
struct Defense {
    first_pitcher: Option<i64>,
    used_reliever: bool,
}

#[derive(Default)]
pub(super) struct GamePitching {
    lines: BTreeMap<i64, Line>,
    defenses: [Defense; 2],
    responsible: HashMap<i64, i64>,
    fielding_error: bool,
    outs: u32,
    current_pitcher: Option<i64>,
}

impl GamePitching {
    pub(super) fn start_half(&mut self) {
        self.responsible.clear();
        self.fielding_error = false;
        self.outs = 0;
        self.current_pitcher = None;
    }

    pub(super) fn record_pitch(
        &mut self,
        pitcher: Option<i64>,
        inning: Option<u32>,
        side: Option<&str>,
        outs: Option<u32>,
        fielding_error: bool,
    ) -> Option<i64> {
        self.fielding_error |= fielding_error;
        let pitcher = pitcher.or(self.current_pitcher)?;
        self.current_pitcher = Some(pitcher);
        let line = self.lines.entry(pitcher).or_default();
        line.appeared = true;
        if let Some(defense) = match side {
            Some("Top") => Some(&mut self.defenses[0]),
            Some("Bottom") => Some(&mut self.defenses[1]),
            _ => None,
        } {
            match defense.first_pitcher {
                None => {
                    defense.first_pitcher = Some(pitcher);
                    line.started |= inning == Some(1);
                }
                Some(first) => defense.used_reliever |= first != pitcher,
            }
        }
        if let Some(outs) = outs {
            let outs = outs.min(3);
            line.outs += outs.saturating_sub(self.outs);
            self.outs = outs;
        }
        Some(pitcher)
    }

    pub(super) fn record_batter(
        &mut self,
        pitcher: i64,
        batter: i64,
        result: &str,
        at_bat: bool,
        hit: bool,
        scored_wild_pitch: bool,
        replaced_runner: Option<i64>,
    ) {
        let line = self.lines.entry(pitcher).or_default();
        line.at_bats += u32::from(at_bat);
        line.hits += u32::from(hit);
        line.home_runs += u32::from(result == "HomeRun");
        line.so += u32::from(result == "Strikeout");
        line.bb += u32::from(result == "Walk");
        line.hbp += u32::from(result == "HitByPitch");
        line.wild_pitches += u32::from(scored_wild_pitch);

        if hit
            || matches!(
                result,
                "Walk" | "HitByPitch" | "FieldersChoice" | "DoublePlay"
            )
        {
            // On a fielder's choice the replacement runner inherits the retired runner's charge.
            let responsible = replaced_runner
                .and_then(|runner| self.responsible.get(&runner).copied())
                .unwrap_or(pitcher);
            self.responsible.insert(batter, responsible);
        }
    }

    pub(super) fn record_runs(&mut self, pitcher: i64, points: u32, scorers: &[i64]) {
        for index in 0..points as usize {
            let responsible = scorers
                .get(index)
                .and_then(|runner| self.responsible.get(runner).copied())
                .unwrap_or(pitcher);
            let line = self.lines.entry(responsible).or_default();
            line.runs += 1;
            // The log gives the real outcome but cannot replay the inning without an error.
            line.unclassified_runs += u32::from(self.fielding_error);
        }
    }

    pub(super) fn save(mut self, conn: &Connection, game: u32) -> Result<()> {
        let mut decisions = conn.prepare_cached(
            "SELECT pitcher_id,decision FROM player_game_pitching_decision WHERE game_id=?1",
        )?;
        let mut rows = decisions.query([game])?;
        while let Some(row) = rows.next()? {
            let line = self.lines.entry(row.get(0)?).or_default();
            match row.get::<_, String>(1)?.as_str() {
                "Win" => line.wins += 1,
                "Loss" => line.losses += 1,
                "Hold" => line.holds += 1,
                "Save" => line.saves += 1,
                _ => {}
            }
        }
        let completed: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM game WHERE id=?1 AND actual_date IS NOT NULL AND actual_date!='1900-01-01')",
            [game],
            |row| row.get(0),
        )?;
        let mut insert = conn.prepare_cached(&format!(
            "INSERT INTO game_pitching_stat_cache(game_id,player_id,{COUNTS})
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24)"
        ))?;
        for (player, line) in self.lines {
            let complete_game = completed
                && line.started
                && self
                    .defenses
                    .iter()
                    .any(|side| side.first_pitcher == Some(player) && !side.used_reliever);
            let qs_eligible = line.started && line.outs >= 18;
            let earned_runs = line.runs - line.unclassified_runs;
            insert.execute(params![
                game,
                player,
                line.appeared,
                line.outs,
                line.runs,
                line.wins,
                line.losses,
                line.saves,
                line.holds,
                line.so,
                line.bb,
                line.started,
                complete_game,
                complete_game && line.runs == 0,
                qs_eligible && line.runs <= 3,
                qs_eligible && earned_runs <= 3 && line.runs > 3,
                if line.appeared && !line.started {
                    line.wins
                } else {
                    0
                },
                line.hits,
                line.home_runs,
                line.hbp,
                line.wild_pitches,
                earned_runs,
                line.unclassified_runs,
                line.at_bats,
            ])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::{game::GameType, stadium::Stadium, team::Team};

    #[test]
    fn season_pitching_stats_use_only_prior_completed_regular_games() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE game(id INTEGER PRIMARY KEY, season INTEGER, seq INTEGER,
                               actual_date TEXT, game_type TEXT);
             CREATE TABLE player_info(id INTEGER PRIMARY KEY, team_id INTEGER);
             CREATE TABLE game_pitching_stat_cache(game_id INTEGER, player_id INTEGER,
                                                   outs INTEGER, runs INTEGER);
             INSERT INTO player_info VALUES (1,1),(2,2),(3,3);
             INSERT INTO game VALUES
                 (1,2026,2,'2026-04-01','Regular'),
                 (2,2026,2,'2026-04-02','Regular'),
                 (3,2026,1,'2026-04-02','Regular'),
                 (4,2025,2,'2025-04-01','Regular'),
                 (5,2026,2,'1900-01-01','Regular'),
                 (6,2026,2,NULL,'Regular'),
                 (7,2026,2,'2026-04-01','Exhibition'),
                 (8,2026,2,'2026-04-01','Postseason'),
                 (9,2026,3,'2026-04-02','Regular'),
                 (10,2026,2,'2026-04-02','Regular'),
                 (11,2026,2,'2026-04-02','Regular'),
                 (12,2026,1,'2026-04-03','Regular');
             INSERT INTO game_pitching_stat_cache
                 SELECT id,1,18,4 FROM game;
             INSERT INTO game_pitching_stat_cache VALUES (1,2,0,2),(1,3,27,3);",
        )
        .unwrap();
        let game = GameSchedule {
            id: 10,
            season: 2026,
            round_seq: 1,
            seq: 2,
            planned_date: "2026-04-02".parse().unwrap(),
            away_team: Team::min(1, "Away"),
            home_team: Team::min(2, "Home"),
            stadium: Stadium::default(),
            game_type: GameType::Regular,
            max_inning: 9,
            base_four_seam_speed: 40.833,
        };
        let stats = load_season_before_game(&conn, &game).unwrap();
        assert_eq!(stats.pitchers.len(), 2);
        assert_eq!(
            stats.pitchers[&1],
            SeasonPitchingLine { outs: 54, runs: 12 }
        );
        assert_eq!(stats.pitchers[&2], SeasonPitchingLine { outs: 0, runs: 2 });
        assert_eq!(
            stats.season_total,
            SeasonPitchingLine { outs: 81, runs: 17 }
        );
    }
}
