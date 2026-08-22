use crate::domain::shared::game::{
    BattingResult, Count, GameDetail, GameHeader, GameResult, GameSchedule, GameSeason, GameType,
    Inning, TB,
};
use crate::domain::shared::stadium::Stadium;
use crate::domain::shared::team::Team;
use crate::error::AppError;
use crate::repositories::db::{DbClient, FromRow};
use rusqlite::{
    Transaction, params,
    types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef},
};
use tracing::info;
use validator::Validate;

const UPDATE_GAME_RESULT_SQL: &str =
    "UPDATE game SET actual_date = ?1, away_points = ?2, home_points = ?3 WHERE id = ?4";
const INSERT_INNING_SQL: &str = "INSERT INTO inning (game_id, seq, tb) VALUES (?1, ?2, ?3)";
const INSERT_COUNT_SQL: &str = "INSERT INTO count (
        game_id, inning_seq, inning_tb, seq, point, ball, strike, out
    ) VALUES (
        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8
    )";

#[tracing::instrument(skip(db_client, tx, game), fields(game_id = %game.id), err)]
pub fn update_game_result_header(
    db_client: &DbClient,
    tx: &Transaction,
    game: &GameResult,
) -> Result<usize, AppError> {
    info!("update_game_result_header() started");

    db_client.execute_tx(
        tx,
        UPDATE_GAME_RESULT_SQL,
        params![
            game.actual_date,
            game.away_total_point,
            game.home_total_point,
            game.id
        ],
    )
}

#[tracing::instrument(skip(db_client, tx, inning), fields(game_id = %game_id, inning_seq = %inning.seq, inning_tb = %inning.tb), err)]
pub fn insert_inning_with_counts(
    db_client: &DbClient,
    tx: &Transaction,
    game_id: u32,
    inning: &Inning,
) -> Result<(), AppError> {
    info!("insert_inning_with_counts() started");

    db_client.execute_tx(
        tx,
        INSERT_INNING_SQL,
        params![game_id, inning.seq, inning.tb],
    )?;

    for count in &inning.counts {
        insert_count(db_client, tx, game_id, inning, count)?;
    }

    Ok(())
}

#[tracing::instrument(skip(db_client, tx, inning, count), fields(game_id = %game_id, inning_seq = %inning.seq, inning_tb = %inning.tb, count_seq = %count.seq), err)]
fn insert_count(
    db_client: &DbClient,
    tx: &Transaction,
    game_id: u32,
    inning: &Inning,
    count: &Count,
) -> Result<usize, AppError> {
    info!("insert_count() started");

    db_client.execute_tx(
        tx,
        INSERT_COUNT_SQL,
        params![
            game_id,
            inning.seq,
            inning.tb,
            count.seq,
            count.point,
            count.ball,
            count.strike,
            count.out
        ],
    )
}

impl ToSql for GameType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for GameType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<GameType>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl ToSql for TB {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for TB {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let tb = value.as_str()?;

        tb.parse::<TB>().map_err(|e| {
            eprintln!("{} {}: {:?}", "error_parse", tb, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl ToSql for BattingResult {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for BattingResult {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let br = value.as_str()?;

        br.parse::<BattingResult>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", br, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl FromRow for GameSeason {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let game_season = GameSeason {
            start_date: row.get("season_start_date")?,
            season: row.get("scheduled_season")?,
        };

        game_season.validate()?;

        Ok(game_season)
    }
}

impl FromRow for GameHeader {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let game_header = GameHeader {
            id: row.get("id")?,
            actual_date: row.get("actual_date")?,
            away_team: Team::min(
                row.get("away_team_id")?,
                &row.get::<_, String>("away_team_name")?,
            ),
            home_team: Team::min(
                row.get("home_team_id")?,
                &row.get::<_, String>("home_team_name")?,
            ),
            game_type: row.get::<_, GameType>("game_type")?,
            away_points: row.get("away_points")?,
            home_points: row.get("home_points")?,
        };

        game_header.validate()?;

        Ok(game_header)
    }
}

impl FromRow for GameSchedule {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let default_stadium = Stadium::default();
        let stadium = Stadium::new(
            row.get("stadium_id").unwrap_or(default_stadium.id),
            row.get("stadium_name").unwrap_or(default_stadium.name),
            row.get("stadium_foul_pole_distance")
                .unwrap_or(default_stadium.foul_pole_distance),
            row.get("stadium_center_fence_distance")
                .unwrap_or(default_stadium.center_fence_distance),
            row.get("stadium_fence_height")
                .unwrap_or(default_stadium.fence_height),
        );

        let game_scheduler = GameSchedule {
            id: row.get("id")?,
            season: row.get("season")?,
            round_seq: row.get("round_seq")?,
            seq: row.get("seq")?,
            planned_date: row.get("planned_date")?,
            away_team: Team::min(
                row.get("away_team_id")?,
                &row.get::<_, String>("away_team_name")?,
            ),
            home_team: Team::min(
                row.get("home_team_id")?,
                &row.get::<_, String>("home_team_name")?,
            ),
            game_type: row.get::<_, GameType>("game_type")?,
            stadium,
        };

        game_scheduler.validate()?;

        Ok(game_scheduler)
    }
}

impl FromRow for GameDetail {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let game_detail = GameDetail {
            id: row.get("id")?,
            actual_date: row.get("actual_date")?,
            away_team: Team::min(
                row.get("away_team_id")?,
                &row.get::<_, String>("away_team_name")?,
            ),
            home_team: Team::min(
                row.get("home_team_id")?,
                &row.get::<_, String>("home_team_name")?,
            ),
            game_type: row.get::<_, GameType>("game_type")?,
            innings: Vec::new(),
            away_points: row.get("away_points")?,
            home_points: row.get("home_points")?,
            player_entries: Vec::new(),
            player_battings: Vec::new(),
            player_runnings: Vec::new(),
        };

        game_detail.validate()?;

        Ok(game_detail)
    }
}

impl FromRow for Inning {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let inning = Inning {
            seq: row.get("seq")?,
            tb: row.get::<_, TB>("tb")?,
            counts: Vec::new(),
        };

        inning.validate()?;

        Ok(inning)
    }
}

impl FromRow for Count {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let count = Count {
            seq: row.get("seq")?,
            point: row.get("point")?,
            ball: row.get("ball")?,
            strike: row.get("strike")?,
            out: row.get("out")?,
        };

        count.validate()?;

        Ok(count)
    }
}
