use crate::domain::shared::player::{Player, PlayerInfo};
use crate::domain::shared::stats::{BattingStats, PitchingStats, Standing};
use crate::domain::shared::team::Team;
use crate::error::AppError;
use crate::repositories::db::FromRow;
use validator::Validate;

impl FromRow for Standing {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let standings = Standing {
            team: Team {
                manager:None,
                id: row.get("team_id").map_err(|e| AppError::Database(e))?,
                name: row.get("team_name").map_err(|e| AppError::Database(e))?,
                players: Vec::new(),
            },
            games: row.get("games").map_err(|e| AppError::Database(e))?,
            wins: row.get("wins").map_err(|e| AppError::Database(e))?,
            losses: row.get("losses").map_err(|e| AppError::Database(e))?,
            draws: row.get("draws").map_err(|e| AppError::Database(e))?,
            pct: row.get("pct").map_err(|e| AppError::Database(e))?,
            gb: 0.0,
            r: row.get("runs")?,
            ra: row.get("runs_allowed")?,
            remaining: row.get("remaining")?,
            homeruns: row.get("homeruns")?,
            steals: row.get("steals")?,
            ba: row.get("ba")?,
            era: row.get("era")?,
            errors: row.get("errors")?,
        };

        standings.validate()?;

        Ok(standings)
    }
}

impl FromRow for BattingStats {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let first_name: String = row
            .get("batter_first_name")
            .map_err(|e| AppError::Database(e))?;
        let last_name: String = row
            .get("batter_last_name")
            .map_err(|e| AppError::Database(e))?;
        let batting_stats = BattingStats {
            batter: Player::from_player_info(PlayerInfo::new_min(
                row.get("player_id").map_err(|e| AppError::Database(e))?,
                first_name,
                last_name,
            )),
            ab: row.get("ab").map_err(|e| AppError::Database(e))?,
            single: row.get("single").map_err(|e| AppError::Database(e))?,
            double: row.get("double").map_err(|e| AppError::Database(e))?,
            triple: row.get("triple").map_err(|e| AppError::Database(e))?,
            homerun: row.get("homerun").map_err(|e| AppError::Database(e))?,
            ba: row.get("ba").map_err(|e| AppError::Database(e))?,
            rbi: row.get("rbi")?,
            games: row.get("games")?,
            pa: row.get("pa")?,
            runs: row.get("runs")?,
            so: row.get("so")?,
            bb: row.get("bb")?,
            hbp: row.get("hbp")?,
            sf: row.get("sf")?,
            sb: row.get("sb")?,
            cs: row.get("cs")?,
            gidp: row.get("gidp")?,
            risp_hits: row.get("risp_hits")?,
            risp_ab: row.get("risp_ab")?,
            errors: row.get("errors")?,
        };

        batting_stats.validate()?;

        Ok(batting_stats)
    }
}

impl FromRow for PitchingStats {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let first_name: String = row
            .get("pitcher_first_name")
            .map_err(|e| AppError::Database(e))?;
        let last_name: String = row
            .get("pitcher_last_name")
            .map_err(|e| AppError::Database(e))?;
        let pitching_stats = PitchingStats {
            batter: Player::from_player_info(PlayerInfo::new_min(
                row.get("player_id").map_err(|e| AppError::Database(e))?,
                first_name,
                last_name,
            )),
            games: row.get("games")?,
            outs: row.get("outs")?,
            wins: row.get("wins")?,
            losses: row.get("losses")?,
            saves: row.get("saves")?,
            holds: row.get("holds")?,
            so: row.get("so")?,
            bb: row.get("bb")?,
            starts: row.get("starts")?,
            complete_games: row.get("complete_games")?,
            shutouts: row.get("shutouts")?,
            quality_starts: if row.get::<_, u32>("uncertain_starts")? == 0 {
                Some(row.get("quality_starts")?)
            } else {
                None
            },
            relief_wins: row.get("relief_wins")?,
            hits: row.get("hits")?,
            home_runs: row.get("home_runs")?,
            hbp: row.get("hbp")?,
            wild_pitches: row.get("wild_pitches")?,
            runs: row.get("runs")?,
            earned_runs: if row.get::<_, u32>("unclassified_runs")? == 0 {
                Some(row.get("earned_runs")?)
            } else {
                None
            },
            at_bats: row.get("at_bats")?,
        };

        pitching_stats.validate()?;

        Ok(pitching_stats)
    }
}
