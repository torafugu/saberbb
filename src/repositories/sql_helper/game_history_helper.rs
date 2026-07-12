use crate::domain::shared::game::BattingResult;
use crate::domain::shared::game_history::{ActiveFielderView, BattingResultView};
use crate::domain::shared::player::PlayerInfo;
use crate::error::AppError;
use crate::repositories::db::FromRow;
use validator::Validate;

impl FromRow for ActiveFielderView {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let player_info = PlayerInfo {
            id: row.get("player_id")?,
            first_name: row.get("first_name")?,
            last_name: row.get("last_name")?,
            age: row.get("age")?,
            uniform_number: row.get("uniform_number")?,
        };

        let active_fielder_view = ActiveFielderView {
            start_count_seq: row.get("start_count_seq")?,
            end_count_seq: row.get("end_count_seq")?,
            team_id: row.get("team_id")?,
            position: row.get("position")?,
            player: player_info,
        };

        active_fielder_view.validate()?;

        Ok(active_fielder_view)
    }
}

impl FromRow for BattingResultView {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let pitcher_info = PlayerInfo {
            id: row.get("pitcher_id")?,
            first_name: row.get("pitcher_first_name")?,
            last_name: row.get("pitcher_last_name")?,
            age: row.get("pitcher_age")?,
            uniform_number: row.get("pitcher_uniform_number")?,
        };

        let batter_info = PlayerInfo {
            id: row.get("batter_id")?,
            first_name: row.get("batter_first_name")?,
            last_name: row.get("batter_last_name")?,
            age: row.get("batter_age")?,
            uniform_number: row.get("batter_uniform_number")?,
        };

        let batting_result_view = BattingResultView {
            count_seq: row.get("count_seq")?,
            pitcher: pitcher_info,
            batter: batter_info,
            result: row.get::<_, BattingResult>("result")?,
        };

        batting_result_view.validate()?;

        Ok(batting_result_view)
    }
}
