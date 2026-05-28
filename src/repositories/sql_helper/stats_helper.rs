use crate::domain::shared::player::Player;
use crate::domain::shared::stat::{BattingStats, Standing};
use crate::domain::shared::team::Team;
use crate::error::AppError;
use crate::repositories::db::FromRow;
use validator::Validate;

impl FromRow for Standing {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let standings = Standing {
            team: Team {
                id: row.get("team_id")?,
                name: row.get("team_name")?,
                players: Vec::new(),
            },
            games: row.get("games")?,
            wins: row.get("wins")?,
            losses: row.get("losses")?,
            draws: row.get("draws")?,
            pct: row.get("pct")?,
            gb: 0.0,
            r: 0,
            ra: 0,
        };

        standings.validate()?;

        Ok(standings)
    }
}

impl FromRow for BattingStats {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let first_name: String = row.get("batter_first_name")?;
        let last_name: String = row.get("batter_last_name")?;
        let batting_stats = BattingStats {
            batter: Player::min(row.get("batter_id")?, &first_name, &last_name),
            ab: row.get("ab")?,
            single: row.get("single")?,
            double: row.get("double")?,
            triple: row.get("triple")?,
            homerun: row.get("homerun")?,
            ba: row.get("ba")?,
            rbi: row.get("rbi")?,
        };

        batting_stats.validate()?;

        Ok(batting_stats)
    }
}
