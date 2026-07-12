use crate::domain::shared::game::BattingResult;
use crate::domain::shared::game_history::{ActiveFielderHistory, BattingResultHistory};
use crate::error::AppError;
use crate::repositories::db::FromRow;
use validator::Validate;

impl FromRow for ActiveFielderHistory {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let batting_order_history = ActiveFielderHistory {
            start_count_seq: row.get("start_count_seq")?,
            end_count_seq: row.get("end_count_seq")?,
            team_id: row.get("team_id")?,
            position: row.get("position")?,
            player_id: row.get("player_id")?,
        };

        batting_order_history.validate()?;

        Ok(batting_order_history)
    }
}

impl FromRow for BattingResultHistory {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let batting_result_history = BattingResultHistory {
            count_seq: row.get("count_seq")?,
            pitcher_id: row.get("pitcher_id")?,
            batter_id: row.get("batter_id")?,
            result: row.get::<_, BattingResult>("result")?,
        };

        batting_result_history.validate()?;

        Ok(batting_result_history)
    }
}
