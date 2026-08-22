use crate::domain::resolver::fielding_resolver::PlayType;
use crate::domain::resolver::running_resolver::RunningEvent;
use crate::domain::shared::ball::BattedBall;
use crate::domain::shared::game::{BattingResult, FieldingResult};
use crate::domain::shared::game_state::Ruling;
use crate::domain::shared::game_stats::{
    PlayerGameBatting, PlayerGameBattingView, PlayerGameEntry, PlayerGameEntryView,
    PlayerGameFielding, PlayerGameRunning, PlayerGameRunningView,
};
use crate::domain::shared::player::PlayerInfo;
use crate::domain::shared::stadium::Base;
use crate::domain::util::PolarPosition;
use crate::error::AppError;
use crate::repositories::db::{DbClient, FromRow};
use rusqlite::{
    Transaction, params,
    types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef},
};
use tracing::info;
use validator::Validate;

const INSERT_PLAYER_GAME_ENTRY_SQL: &str = "INSERT INTO player_game_entry (
        game_id, start_count_seq, end_count_seq, position, batting_order, player_id
    ) VALUES (
        ?1, ?2, ?3, ?4, ?5, ?6
    )";

const INSERT_PLAYER_GAME_BATTING_SQL: &str = "INSERT INTO player_game_batting (
        game_id, count_seq, pitcher_id, batter_id, launch_speed, launch_angle, polar_distance, polar_angle,
        total_time, first_bounce_distance, first_bounce_angle, first_bounce_time,
        fence_impact_distance, fence_impact_angle, fence_impact_time, outbound_result,
        fielder_position, result
    ) VALUES (
        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18
    )";

const INSERT_PLAYER_GAME_FIELDING_SQL: &str = "INSERT INTO player_game_fielding (
        game_id, count_seq, seq, catch_fielder_id, catch_fielder_position, cutoff_fielder_id,
        cutoff_fielder_position, final_fielder_id, final_fielder_position, time_to_field, play_type
    ) VALUES (
        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11
    )";

const INSERT_PLAYER_GAME_RUNNING_SQL: &str = "INSERT INTO player_game_running (
        game_id, count_seq, seq, defense_time, runner_time, throw_target_base, event,
        play_type, ruling, runs_scored, target_runner_id, runner_1st_id, runner_2nd_id, runner_3rd_id
    ) VALUES (
        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
    )";

#[tracing::instrument(skip(db_client, tx, player_game_entry), fields(game_id = %game_id, count_seq = %player_game_entry.start_count_seq, player_id = %player_game_entry.player_id), err)]
pub fn insert_player_game_entry(
    db_client: &DbClient,
    tx: &Transaction,
    game_id: u32,
    player_game_entry: &PlayerGameEntry,
) -> Result<usize, AppError> {
    info!("insert_player_game_entry() started");

    let end_count_seq = player_game_entry.end_count_seq.unwrap_or_default();

    db_client.execute_tx(
        tx,
        INSERT_PLAYER_GAME_ENTRY_SQL,
        params![
            game_id,
            player_game_entry.start_count_seq,
            end_count_seq,
            player_game_entry.position,
            player_game_entry.batting_order,
            player_game_entry.player_id
        ],
    )
}

#[tracing::instrument(skip(db_client, tx, player_game_batting), fields(game_id = %game_id, count_seq = %player_game_batting.count_seq), err)]
pub fn insert_player_game_batting(
    db_client: &DbClient,
    tx: &Transaction,
    game_id: u32,
    player_game_batting: &PlayerGameBatting,
) -> Result<usize, AppError> {
    info!("insert_player_game_batting() started");

    let fielder_position_str: Option<&str> = player_game_batting
        .fielder_position
        .as_ref()
        .map(|p| p.as_ref());

    db_client.execute_tx(
        tx,
        INSERT_PLAYER_GAME_BATTING_SQL,
        params![
            game_id,
            player_game_batting.count_seq,
            player_game_batting.pitcher_id,
            player_game_batting.batter_id,
            player_game_batting.ball.launch_speed,
            player_game_batting.ball.launch_angle,
            player_game_batting.ball.final_position.distance,
            player_game_batting.ball.final_position.angle,
            player_game_batting.ball.total_time,
            player_game_batting
                .ball
                .first_bounce_position
                .map(|position| position.distance),
            player_game_batting
                .ball
                .first_bounce_position
                .map(|position| position.angle),
            player_game_batting.ball.first_bounce_time,
            player_game_batting
                .ball
                .fence_impact_position
                .map(|position| position.distance),
            player_game_batting
                .ball
                .fence_impact_position
                .map(|position| position.angle),
            player_game_batting.ball.fence_impact_time,
            player_game_batting.ball.outbound_result,
            fielder_position_str,
            player_game_batting.result
        ],
    )
}

#[tracing::instrument(skip(db_client, tx, player_game_fielding), fields(game_id = %game_id, count_seq = %player_game_fielding.count_seq, seq = %player_game_fielding.seq), err)]
pub fn insert_player_game_fielding(
    db_client: &DbClient,
    tx: &Transaction,
    game_id: u32,
    player_game_fielding: &PlayerGameFielding,
) -> Result<usize, AppError> {
    info!("insert_player_game_fielding() started");

    db_client.execute_tx(
        tx,
        INSERT_PLAYER_GAME_FIELDING_SQL,
        params![
            game_id,
            player_game_fielding.count_seq,
            player_game_fielding.seq,
            player_game_fielding.catch_fielder_id,
            player_game_fielding.catch_fielder_position,
            player_game_fielding.cutoff_fielder_id,
            player_game_fielding.cutoff_fielder_position,
            player_game_fielding.final_fielder_id,
            player_game_fielding.final_fielder_position,
            player_game_fielding.time_to_field,
            player_game_fielding.play_type
        ],
    )
}

#[tracing::instrument(skip(db_client, tx, player_game_running), fields(game_id = %game_id, count_seq = %player_game_running.count_seq, seq = %player_game_running.seq), err)]
pub fn insert_player_game_running(
    db_client: &DbClient,
    tx: &Transaction,
    game_id: u32,
    player_game_running: &PlayerGameRunning,
) -> Result<usize, AppError> {
    info!("insert_player_game_running() started");

    db_client.execute_tx(
        tx,
        INSERT_PLAYER_GAME_RUNNING_SQL,
        params![
            game_id,
            player_game_running.count_seq,
            player_game_running.seq,
            player_game_running.defense_time,
            player_game_running.runner_time,
            player_game_running.throw_target_base,
            player_game_running.event.as_ref(),
            player_game_running.play_type,
            player_game_running.ruling,
            player_game_running.runs_scored,
            player_game_running.target_runner_id,
            player_game_running.runner_1st_id,
            player_game_running.runner_2nd_id,
            player_game_running.runner_3rd_id
        ],
    )
}

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

impl ToSql for RunningEvent {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for RunningEvent {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<RunningEvent>().map_err(|e| {
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
            batting_order: row.get("batting_order")?,
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

        let first_bounce_distance = row.get::<_, Option<f64>>("first_bounce_distance")?;
        let first_bounce_angle = row.get::<_, Option<f64>>("first_bounce_angle")?;
        let first_bounce_position = first_bounce_distance
            .zip(first_bounce_angle)
            .map(|(distance, angle)| PolarPosition::new(distance, angle));

        let fence_impact_distance = row.get::<_, Option<f64>>("fence_impact_distance")?;
        let fence_impact_angle = row.get::<_, Option<f64>>("fence_impact_angle")?;
        let fence_impact_position = fence_impact_distance
            .zip(fence_impact_angle)
            .map(|(distance, angle)| PolarPosition::new(distance, angle));

        let batting_result_view = PlayerGameBattingView {
            count_seq: row.get("count_seq")?,
            pitcher: pitcher_info,
            batter: batter_info,
            ball: BattedBall::new(
                row.get("launch_speed")?,
                row.get("launch_angle")?,
                row.get("polar_distance")?,
                row.get("polar_angle")?,
                row.get("total_time")?,
                first_bounce_position,
                row.get("first_bounce_time")?,
                fence_impact_position,
                row.get("fence_impact_time")?,
                row.get("outbound_result")?,
            ),
            fielder_position: row.get("fielder_position")?,
            result: row.get::<_, BattingResult>("result")?,
        };

        batting_result_view.validate()?;

        Ok(batting_result_view)
    }
}

impl FromRow for PlayerGameRunningView {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let runner_1st: Option<PlayerInfo> = row
            .get::<_, Option<i64>>("runner_1st_id")?
            .map(|_| -> Result<PlayerInfo, AppError> {
                Ok(PlayerInfo {
                    id: row.get("runner_1st_id")?,
                    first_name: row.get("runner_1st_first_name")?,
                    last_name: row.get("runner_1st_last_name")?,
                    age: row.get("runner_1st_age")?,
                    uniform_number: row.get("runner_1st_uniform_number")?,
                })
            })
            .transpose()?;

        let runner_2nd: Option<PlayerInfo> = row
            .get::<_, Option<i64>>("runner_2nd_id")?
            .map(|_| -> Result<PlayerInfo, AppError> {
                Ok(PlayerInfo {
                    id: row.get("runner_2nd_id")?,
                    first_name: row.get("runner_2nd_first_name")?,
                    last_name: row.get("runner_2nd_last_name")?,
                    age: row.get("runner_2nd_age")?,
                    uniform_number: row.get("runner_2nd_uniform_number")?,
                })
            })
            .transpose()?;

        let runner_3rd: Option<PlayerInfo> = row
            .get::<_, Option<i64>>("runner_3rd_id")?
            .map(|_| -> Result<PlayerInfo, AppError> {
                Ok(PlayerInfo {
                    id: row.get("runner_3rd_id")?,
                    first_name: row.get("runner_3rd_first_name")?,
                    last_name: row.get("runner_3rd_last_name")?,
                    age: row.get("runner_3rd_age")?,
                    uniform_number: row.get("runner_3rd_uniform_number")?,
                })
            })
            .transpose()?;

        let target_runner: Option<PlayerInfo> = row
            .get::<_, Option<i64>>("target_runner_id")?
            .map(|_| -> Result<PlayerInfo, AppError> {
                Ok(PlayerInfo {
                    id: row.get("target_runner_id")?,
                    first_name: row.get("target_runner_first_name")?,
                    last_name: row.get("target_runner_last_name")?,
                    age: row.get("target_runner_age")?,
                    uniform_number: row.get("target_runner_uniform_number")?,
                })
            })
            .transpose()?;

        let player_game_running_view = PlayerGameRunningView {
            count_seq: row.get("count_seq")?,
            seq: row.get("seq")?,
            defense_time: row.get("defense_time")?,
            runner_time: row.get("runner_time")?,
            throw_target_base: row.get("throw_target_base")?,
            target_runner,
            event: row.get("event")?,
            play_type: row.get("play_type")?,
            ruling: row.get("ruling")?,
            runs_scored: row.get("runs_scored")?,
            runner_1st,
            runner_2nd,
            runner_3rd,
        };

        player_game_running_view.validate()?;

        Ok(player_game_running_view)
    }
}
