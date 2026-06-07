use crate::domain::shared::game::{
    BattingResult, Count, GameDetail, GameHeader, GameRow, GameScheduler, GameSeason, GameType,
    Inning, TB,
};
use crate::domain::shared::team::Team;
use crate::error::AppError;
use crate::repositories::db::FromRow;
use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use validator::Validate;

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

impl FromRow for GameScheduler {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let game_scheduler = GameScheduler {
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
            batting_order_histories: Vec::new(),
        };

        game_detail.validate()?;

        Ok(game_detail)
    }
}

impl FromRow for GameRow {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let game_row = GameRow {
            id: row.get("id")?,
            season: row.get("season")?,
            round_seq: row.get("round_seq")?,
            seq: row.get("seq")?,
            planned_date: row.get("planned_date")?,
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
        };

        game_row.validate()?;

        Ok(game_row)
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
            bases_occupied: row.get("bases_occupied")?,
            result: row.get::<_, BattingResult>("result")?,
            point: row.get("point")?,
            ball: row.get("ball")?,
            strike: row.get("strike")?,
            out: row.get("out")?,
        };

        count.validate()?;

        Ok(count)
    }
}
