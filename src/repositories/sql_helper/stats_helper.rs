use crate::domain::shared::stat::Standing;
use crate::domain::shared::team::Team;
use crate::error::AppError;
use crate::repositories::db::FromRow;
use validator::Validate;

impl FromRow for Standing {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let standings = Standing {
            team: Team {
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
            r: 0,
            ra: 0,
        };

        standings.validate()?;

        Ok(standings)
    }
}

// impl FromRow for BattingStats {
//     type Error = AppError;

//     fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
//         let first_name: String = row
//             .get("batter_first_name")
//             .map_err(|e| AppError::Database(e))?;
//         let last_name: String = row
//             .get("batter_last_name")
//             .map_err(|e| AppError::Database(e))?;
//         let batting_stats = BattingStats {
//             batter: Player::new(
//                 row.get("player_id").map_err(|e| AppError::Database(e))?,
//                 &first_name,
//                 &last_name,
//             ),
//             ab: row.get("ab").map_err(|e| AppError::Database(e))?,
//             single: row.get("single").map_err(|e| AppError::Database(e))?,
//             double: row.get("double").map_err(|e| AppError::Database(e))?,
//             triple: row.get("triple").map_err(|e| AppError::Database(e))?,
//             homerun: row.get("homerun").map_err(|e| AppError::Database(e))?,
//             ba: row.get("ba").map_err(|e| AppError::Database(e))?,
//             rbi: row.get("rbi").map_err(|e| AppError::Database(e))?,
//         };

//         batting_stats.validate()?;

//         Ok(batting_stats)
//     }
// }
