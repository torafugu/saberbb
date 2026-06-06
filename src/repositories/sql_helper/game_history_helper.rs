use crate::domain::shared::game_history::BattingOrderHistory;
use crate::domain::shared::player::Player;
use crate::error::AppError;
use crate::repositories::db::FromRow;
use validator::Validate;

impl FromRow for BattingOrderHistory {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let player = Player::new(
            row.get("player_id")?,
            &row.get::<_, String>("player_first_name")?,
            &row.get::<_, String>("player_last_name")?,
            row.get("player_age")?,
            row.get("player_throw")?,
            row.get("player_bat")?,
            row.get("player_mod_ba")?,
            row.get("player_mod_slg")?,
        );
        let batting_order_history = BattingOrderHistory {
            start_inning_seq: row.get("start_inning_seq")?,
            start_inning_tb: row.get("start_inning_tb")?,
            start_count_seq: row.get("start_count_seq")?,
            end_inning_seq: row.get("end_inning_seq")?,
            end_inning_tb: row.get("end_inning_tb")?,
            end_count_seq: row.get("end_count_seq")?,
            team_id: row.get("team_id")?,
            index: row.get("index_num")?,
            position: row.get("position")?,
            player: player,
        };

        batting_order_history.validate()?;

        Ok(batting_order_history)
    }
}
