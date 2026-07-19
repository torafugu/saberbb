use crate::domain::resolver::fielding_resolver::PlayType;
use crate::domain::shared::game::{BattingResult, FieldingResult};
use crate::domain::shared::game_state::Ruling;
use crate::domain::shared::game_stats::{
    PlayerGameBattingView, PlayerGameEntryView, PlayerGameRunning,
};
use crate::domain::shared::player::PlayerInfo;
use crate::domain::shared::stadium::Base;
use crate::error::AppError;
use crate::repositories::db::FromRow;
use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use validator::Validate;

impl ToSql for Ruling {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for Ruling {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<Ruling>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl ToSql for Base {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for Base {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<Base>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl ToSql for PlayType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for PlayType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<PlayType>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl ToSql for FieldingResult {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for FieldingResult {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<FieldingResult>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl FromRow for PlayerGameEntryView {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let player_info = PlayerInfo {
            id: row.get("player_id")?,
            first_name: row.get("first_name")?,
            last_name: row.get("last_name")?,
            age: row.get("age")?,
            uniform_number: row.get("uniform_number")?,
        };

        let active_fielder_view = PlayerGameEntryView {
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

impl FromRow for PlayerGameBattingView {
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

        let batting_result_view = PlayerGameBattingView {
            count_seq: row.get("count_seq")?,
            pitcher: pitcher_info,
            batter: batter_info,
            result: row.get::<_, BattingResult>("result")?,
        };

        batting_result_view.validate()?;

        Ok(batting_result_view)
    }
}

impl FromRow for PlayerGameRunning {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let player_game_running = PlayerGameRunning {
            count_seq: row.get("count_seq")?,
            seq: row.get("seq")?,
            defense_time: row.get("defense_time")?,
            runner_time: row.get("runner_time")?,
            throw_target_base: row.get("throw_target_base")?,
            play_type: row.get("play_type")?,
            ruling: row.get("ruling")?,
            runs_scored: row.get("runs_scored")?,
            runner_1st_id: row.get("runner_1st_id")?,
            runner_2nd_id: row.get("runner_2nd_id")?,
            runner_3rd_id: row.get("runner_3rd_id")?,
        };

        player_game_running.validate()?;

        Ok(player_game_running)
    }
}
