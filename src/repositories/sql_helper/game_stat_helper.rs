use crate::domain::resolver::fielding_resolver::PlayType;
use crate::domain::resolver::pitching_resolver::{LocationBias, PitchDisplacement};
use crate::domain::resolver::running_resolver::RunningEvent;
use crate::domain::shared::ball::{BallLocation, BallMovement, BattedBall, PitchedBall};
use crate::domain::shared::game::{BattingResult, FieldingResult};
use crate::domain::shared::game_state::Ruling;
use crate::domain::shared::game_stats::{
    PlayerGameBatting, PlayerGameBattingView, PlayerGameEntry, PlayerGameEntryView,
    PlayerGameFielding, PlayerGameHomeRunView, PlayerGamePitching, PlayerGamePitchingDecisionView,
    PlayerGamePitchingView, PlayerGameRunning, PlayerGameRunningView,
};
use crate::domain::shared::player::{PitchType, PlayerInfo};
use crate::domain::shared::stadium::Base;
use crate::domain::strategy::pitching_strategy::TargetZone;
use crate::domain::util::PolarPosition;
use crate::domain::util::Vector3D;
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

const INSERT_PLAYER_GAME_PITCHING_SQL: &str = "INSERT INTO player_game_pitching (
        game_id, count_seq, pitcher_id, pitch_type, speed, spin_rate, spin_angle,
        spin_efficiency, release_point_x, release_point_y, release_point_z, flight_time,
        aim_zone, aim_location_x, aim_location_y, actual_location_x, actual_location_y,
        ball_movement_x_m, ball_movement_z_m, timing_bias_sec, spatial_bias_x, spatial_bias_y,
        crossfire_multiplier, release_x_factor, horizontal_offset_m, vertical_offset_m,
        timing_offset_sec
    ) VALUES (
        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19,
        ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27
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

const DELETE_PLAYER_GAME_PITCHING_DECISION_SQL: &str =
    "DELETE FROM player_game_pitching_decision WHERE game_id = ?1";

const INSERT_PLAYER_GAME_PITCHING_DECISION_SQL: &str = "
    WITH
    game_info AS (
        SELECT
            id AS game_id,
            away_team_id,
            home_team_id,
            away_points,
            home_points,
            CASE
                WHEN away_points > home_points THEN away_team_id
                WHEN home_points > away_points THEN home_team_id
            END AS winner_team_id,
            CASE
                WHEN away_points > home_points THEN home_team_id
                WHEN home_points > away_points THEN away_team_id
            END AS loser_team_id,
            ABS(away_points - home_points) AS final_margin
        FROM game
        WHERE id = ?1
            AND actual_date IS NOT NULL
            AND away_points IS NOT NULL
            AND home_points IS NOT NULL
            AND away_points <> home_points
    ),
    score_timeline AS (
        SELECT
            c.game_id,
            c.seq,
            c.inning_tb,
            c.point,
            SUM(CASE WHEN c.inning_tb = 'Top' THEN c.point ELSE 0 END)
                OVER (PARTITION BY c.game_id ORDER BY c.seq) AS away_after,
            SUM(CASE WHEN c.inning_tb = 'Bottom' THEN c.point ELSE 0 END)
                OVER (PARTITION BY c.game_id ORDER BY c.seq) AS home_after
        FROM count c
        WHERE c.game_id = ?1
    ),
    score_timeline_with_before AS (
        SELECT
            game_id,
            seq,
            inning_tb,
            point,
            away_after,
            home_after,
            away_after - CASE WHEN inning_tb = 'Top' THEN point ELSE 0 END AS away_before,
            home_after - CASE WHEN inning_tb = 'Bottom' THEN point ELSE 0 END AS home_before
        FROM score_timeline
    ),
    final_count AS (
        SELECT game_id, MAX(seq) AS seq
        FROM score_timeline
        GROUP BY game_id
    ),
    go_ahead AS (
        SELECT
            st.game_id,
            MAX(st.seq) AS seq
        FROM score_timeline_with_before st
        JOIN game_info g ON g.game_id = st.game_id
        WHERE
            CASE
                WHEN g.winner_team_id = g.away_team_id
                    THEN st.away_after > st.home_after AND st.away_before <= st.home_before
                ELSE st.home_after > st.away_after AND st.home_before <= st.away_before
            END
        GROUP BY st.game_id
    ),
    pitcher_appearances AS (
        SELECT
            pge.game_id,
            pge.player_id AS pitcher_id,
            pi.team_id,
            pge.start_count_seq,
            CASE
                WHEN pge.end_count_seq IS NULL OR pge.end_count_seq = 0 THEN fc.seq
                ELSE pge.end_count_seq
            END AS end_count_seq
        FROM player_game_entry pge
        JOIN player_info pi ON pi.id = pge.player_id
        JOIN final_count fc ON fc.game_id = pge.game_id
        WHERE pge.game_id = ?1
            AND pge.position = 'P'
    ),
    win_decision AS (
        SELECT
            g.game_id,
            pa.pitcher_id,
            pa.team_id,
            'Win' AS decision
        FROM game_info g
        JOIN go_ahead ga ON ga.game_id = g.game_id
        JOIN pitcher_appearances pa
            ON pa.game_id = g.game_id
            AND pa.team_id = g.winner_team_id
            AND pa.start_count_seq <= ga.seq
            AND pa.end_count_seq >= ga.seq
        ORDER BY pa.start_count_seq DESC
        LIMIT 1
    ),
    loss_decision AS (
        SELECT
            g.game_id,
            COALESCE(pgb.pitcher_id, pgp.pitcher_id) AS pitcher_id,
            pi.team_id,
            'Loss' AS decision
        FROM game_info g
        JOIN go_ahead ga ON ga.game_id = g.game_id
        LEFT JOIN player_game_batting pgb
            ON pgb.game_id = ga.game_id
            AND pgb.count_seq = ga.seq
        LEFT JOIN player_game_pitching pgp
            ON pgp.game_id = ga.game_id
            AND pgp.count_seq = ga.seq
        JOIN player_info pi ON pi.id = COALESCE(pgb.pitcher_id, pgp.pitcher_id)
        LIMIT 1
    ),
    final_pitcher AS (
        SELECT
            pa.game_id,
            pa.pitcher_id,
            pa.team_id,
            pa.start_count_seq,
            pa.end_count_seq
        FROM pitcher_appearances pa
        JOIN game_info g
            ON g.game_id = pa.game_id
            AND g.winner_team_id = pa.team_id
        JOIN final_count fc
            ON fc.game_id = pa.game_id
            AND pa.end_count_seq >= fc.seq
        ORDER BY pa.start_count_seq DESC
        LIMIT 1
    ),
    save_decision AS (
        SELECT
            fp.game_id,
            fp.pitcher_id,
            fp.team_id,
            'Save' AS decision
        FROM final_pitcher fp
        JOIN game_info g ON g.game_id = fp.game_id
        LEFT JOIN win_decision w ON w.game_id = fp.game_id
        WHERE fp.start_count_seq > 1
            AND g.final_margin BETWEEN 1 AND 3
            AND (w.pitcher_id IS NULL OR w.pitcher_id <> fp.pitcher_id)
    ),
    hold_candidates AS (
        SELECT
            pa.game_id,
            pa.pitcher_id,
            pa.team_id,
            pa.start_count_seq,
            pa.end_count_seq,
            CASE
                WHEN g.winner_team_id = g.away_team_id THEN
                    COALESCE((SELECT SUM(CASE WHEN c.inning_tb = 'Top' THEN c.point ELSE -c.point END)
                        FROM count c
                        WHERE c.game_id = pa.game_id AND c.seq < pa.start_count_seq), 0)
                ELSE
                    COALESCE((SELECT SUM(CASE WHEN c.inning_tb = 'Bottom' THEN c.point ELSE -c.point END)
                        FROM count c
                        WHERE c.game_id = pa.game_id AND c.seq < pa.start_count_seq), 0)
            END AS margin_at_entry,
            CASE
                WHEN g.winner_team_id = g.away_team_id THEN
                    COALESCE((SELECT SUM(CASE WHEN c.inning_tb = 'Top' THEN c.point ELSE -c.point END)
                        FROM count c
                        WHERE c.game_id = pa.game_id AND c.seq <= pa.end_count_seq), 0)
                ELSE
                    COALESCE((SELECT SUM(CASE WHEN c.inning_tb = 'Bottom' THEN c.point ELSE -c.point END)
                        FROM count c
                        WHERE c.game_id = pa.game_id AND c.seq <= pa.end_count_seq), 0)
            END AS margin_at_exit
        FROM pitcher_appearances pa
        JOIN game_info g
            ON g.game_id = pa.game_id
            AND g.winner_team_id = pa.team_id
        LEFT JOIN final_pitcher fp
            ON fp.game_id = pa.game_id
            AND fp.pitcher_id = pa.pitcher_id
        LEFT JOIN win_decision w
            ON w.game_id = pa.game_id
            AND w.pitcher_id = pa.pitcher_id
        WHERE pa.start_count_seq > 1
            AND fp.pitcher_id IS NULL
            AND w.pitcher_id IS NULL
    ),
    hold_decisions AS (
        SELECT
            game_id,
            pitcher_id,
            team_id,
            'Hold' AS decision
        FROM hold_candidates
        WHERE margin_at_entry BETWEEN 1 AND 3
            AND margin_at_exit > 0
        GROUP BY game_id, pitcher_id, team_id
    )
    INSERT INTO player_game_pitching_decision (game_id, pitcher_id, decision)
    SELECT game_id, pitcher_id, decision FROM win_decision
    UNION ALL
    SELECT game_id, pitcher_id, decision FROM loss_decision
    UNION ALL
    SELECT game_id, pitcher_id, decision FROM save_decision
    UNION ALL
    SELECT game_id, pitcher_id, decision FROM hold_decisions";

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

#[tracing::instrument(skip(db_client, tx, player_game_pitching), fields(game_id = %game_id, count_seq = %player_game_pitching.count_seq), err)]
pub fn insert_player_game_pitching(
    db_client: &DbClient,
    tx: &Transaction,
    game_id: u32,
    player_game_pitching: &PlayerGamePitching,
) -> Result<usize, AppError> {
    info!("insert_player_game_pitching() started");

    db_client.execute_tx(
        tx,
        INSERT_PLAYER_GAME_PITCHING_SQL,
        params![
            game_id,
            player_game_pitching.count_seq,
            player_game_pitching.pitcher_id,
            player_game_pitching.ball.pitch_type.as_ref(),
            player_game_pitching.ball.speed,
            player_game_pitching.ball.spin_rate,
            player_game_pitching.ball.spin_angle,
            player_game_pitching.ball.spin_efficiency,
            player_game_pitching.ball.release_point.x,
            player_game_pitching.ball.release_point.y,
            player_game_pitching.ball.release_point.z,
            player_game_pitching.ball.flight_time,
            player_game_pitching.ball.aim_zone.as_ref(),
            player_game_pitching.ball.aim_location.x,
            player_game_pitching.ball.aim_location.y,
            player_game_pitching.ball.actual_location.x,
            player_game_pitching.ball.actual_location.y,
            player_game_pitching.ball_movement.x_m,
            player_game_pitching.ball_movement.z_m,
            player_game_pitching.location_bias.timing_bias_sec,
            player_game_pitching.location_bias.spatial_bias_x,
            player_game_pitching.location_bias.spatial_bias_y,
            player_game_pitching.pitch_displacement.crossfire_multiplier,
            player_game_pitching.pitch_displacement.release_x_factor,
            player_game_pitching.pitch_displacement.horizontal_offset_m,
            player_game_pitching.pitch_displacement.vertical_offset_m,
            player_game_pitching.pitch_displacement.timing_offset_sec
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

#[tracing::instrument(skip(db_client, tx), fields(game_id = %game_id), err)]
pub fn refresh_player_game_pitching_decisions(
    db_client: &DbClient,
    tx: &Transaction,
    game_id: u32,
) -> Result<usize, AppError> {
    info!("refresh_player_game_pitching_decisions() started");

    db_client.execute_tx(
        tx,
        DELETE_PLAYER_GAME_PITCHING_DECISION_SQL,
        params![game_id],
    )?;
    db_client.execute_tx(
        tx,
        INSERT_PLAYER_GAME_PITCHING_DECISION_SQL,
        params![game_id],
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

impl FromSql for TargetZone {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        match gt {
            "Center" => Ok(TargetZone::Center),
            "LowInside" => Ok(TargetZone::LowInside),
            "LowOutside" => Ok(TargetZone::LowOutside),
            "HighInside" => Ok(TargetZone::HighInside),
            "HighOutside" => Ok(TargetZone::HighOutside),
            _ => {
                eprintln!("{} {}", "Parse error at ", gt);
                Err(rusqlite::types::FromSqlError::InvalidType)
            }
        }
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

impl FromRow for PlayerGamePitching {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let player_game_pitching = PlayerGamePitching {
            count_seq: row.get("count_seq")?,
            pitcher_id: row.get("pitcher_id")?,
            ball: PitchedBall {
                pitch_type: row.get::<_, PitchType>("pitch_type")?,
                speed: row.get("speed")?,
                spin_rate: row.get("spin_rate")?,
                spin_angle: row.get("spin_angle")?,
                spin_efficiency: row.get("spin_efficiency")?,
                release_point: Vector3D {
                    x: row.get("release_point_x")?,
                    y: row.get("release_point_y")?,
                    z: row.get("release_point_z")?,
                },
                flight_time: row.get("flight_time")?,
                aim_zone: row.get::<_, TargetZone>("aim_zone")?,
                aim_location: BallLocation {
                    x: row.get("aim_location_x")?,
                    y: row.get("aim_location_y")?,
                },
                actual_location: BallLocation {
                    x: row.get("actual_location_x")?,
                    y: row.get("actual_location_y")?,
                },
            },
            ball_movement: BallMovement {
                x_m: row.get("ball_movement_x_m")?,
                z_m: row.get("ball_movement_z_m")?,
            },
            location_bias: LocationBias {
                timing_bias_sec: row.get("timing_bias_sec")?,
                spatial_bias_x: row.get("spatial_bias_x")?,
                spatial_bias_y: row.get("spatial_bias_y")?,
            },
            pitch_displacement: PitchDisplacement {
                crossfire_multiplier: row.get("crossfire_multiplier")?,
                release_x_factor: row.get("release_x_factor")?,
                horizontal_offset_m: row.get("horizontal_offset_m")?,
                vertical_offset_m: row.get("vertical_offset_m")?,
                timing_offset_sec: row.get("timing_offset_sec")?,
            },
        };

        player_game_pitching.validate()?;

        Ok(player_game_pitching)
    }
}

impl FromRow for PlayerGamePitchingView {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let player_game_pitching_view = PlayerGamePitchingView {
            count_seq: row.get("count_seq")?,
            pitcher_id: row.get("pitcher_id")?,
            ball: PitchedBall {
                pitch_type: row.get::<_, PitchType>("pitch_type")?,
                speed: row.get("speed")?,
                spin_rate: row.get("spin_rate")?,
                spin_angle: row.get("spin_angle")?,
                spin_efficiency: row.get("spin_efficiency")?,
                release_point: Vector3D {
                    x: row.get("release_point_x")?,
                    y: row.get("release_point_y")?,
                    z: row.get("release_point_z")?,
                },
                flight_time: row.get("flight_time")?,
                aim_zone: row.get::<_, TargetZone>("aim_zone")?,
                aim_location: BallLocation {
                    x: row.get("aim_location_x")?,
                    y: row.get("aim_location_y")?,
                },
                actual_location: BallLocation {
                    x: row.get("actual_location_x")?,
                    y: row.get("actual_location_y")?,
                },
            },
        };

        player_game_pitching_view.validate()?;

        Ok(player_game_pitching_view)
    }
}

impl FromRow for PlayerGamePitchingDecisionView {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let pitcher = PlayerInfo {
            id: row.get("pitcher_id")?,
            first_name: row.get("pitcher_first_name")?,
            last_name: row.get("pitcher_last_name")?,
            age: row.get("pitcher_age")?,
            uniform_number: row.get("pitcher_uniform_number")?,
        };

        let decision = PlayerGamePitchingDecisionView {
            pitcher,
            decision: row.get("decision")?,
        };

        decision.validate()?;

        Ok(decision)
    }
}

impl FromRow for PlayerGameBattingView {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
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
            pitcher_id: row.get("pitcher_id")?,
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

impl FromRow for PlayerGameHomeRunView {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let batter = PlayerInfo {
            id: row.get("batter_id")?,
            first_name: row.get("batter_first_name")?,
            last_name: row.get("batter_last_name")?,
            age: row.get("batter_age")?,
            uniform_number: row.get("batter_uniform_number")?,
        };

        let home_run = PlayerGameHomeRunView {
            count_seq: row.get("count_seq")?,
            batter,
            season_home_runs: row.get("season_home_runs")?,
        };

        home_run.validate()?;

        Ok(home_run)
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
