//! Accumulate pitching statistics alongside the batting cache's saved-play replay.
use anyhow::Result;
use rusqlite::{Connection, params};
use std::collections::{BTreeMap, HashMap};

pub(super) const COUNTS: &str = "games,outs,runs,wins,losses,saves,holds,so,bb,starts,complete_games,shutouts,quality_starts,uncertain_starts,relief_wins,hits,home_runs,hbp,wild_pitches,earned_runs,unclassified_runs,at_bats";

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
        wild_ball_four: bool,
        replaced_runner: Option<i64>,
    ) {
        let line = self.lines.entry(pitcher).or_default();
        line.at_bats += u32::from(at_bat);
        line.hits += u32::from(hit);
        line.home_runs += u32::from(result == "HomeRun");
        line.so += u32::from(result == "Strikeout");
        line.bb += u32::from(result == "Walk");
        line.hbp += u32::from(result == "HitByPitch");
        line.wild_pitches += u32::from(result == "WildPitch" || wild_ball_four);

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
