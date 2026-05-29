use crate::domain::shared::game::{
    BattingResult, Count, GameHeader, GameRow, GameScheduler, GameSeason, GameType, Inning, TB,
};
use crate::domain::shared::player::Player;
use crate::domain::shared::team::Team;
use crate::error::AppError;
use crate::repositories::db::FromRow;
use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use std::sync::Arc;
use validator::Validate;

impl ToSql for GameType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        let s = match self {
            GameType::Exhibition => "Exhibition",
            GameType::Regular => "Regular",
            GameType::Postseason => "Postseason",
        };
        Ok(ToSqlOutput::from(s))
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
        let s = match self {
            TB::Top => "Top",
            TB::Bottom => "Bottom",
        };
        Ok(ToSqlOutput::from(s))
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
        let s = match self {
            BattingResult::Single => "Single",
            BattingResult::Double => "Double",
            BattingResult::Triple => "Triple",
            BattingResult::HomeRun => "HomeRun",
            BattingResult::Out => "Out",
        };
        Ok(ToSqlOutput::from(s))
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
            pitcher: Arc::from(Player::min(
                row.get("p_id")?,
                &row.get::<_, String>("p_first_name")?,
                &row.get::<_, String>("p_last_name")?,
            )),
            catcher: Arc::from(Player::min(
                row.get("c_id")?,
                &row.get::<_, String>("c_first_name")?,
                &row.get::<_, String>("c_last_name")?,
            )),
            first_baseman: Arc::from(Player::min(
                row.get("fb_id")?,
                &row.get::<_, String>("fb_first_name")?,
                &row.get::<_, String>("fb_last_name")?,
            )),
            second_baseman: Arc::from(Player::min(
                row.get("sb_id")?,
                &row.get::<_, String>("sb_first_name")?,
                &row.get::<_, String>("sb_last_name")?,
            )),
            third_baseman: Arc::from(Player::min(
                row.get("tb_id")?,
                &row.get::<_, String>("tb_first_name")?,
                &row.get::<_, String>("tb_last_name")?,
            )),
            shortstop: Arc::from(Player::min(
                row.get("ss_id")?,
                &row.get::<_, String>("ss_first_name")?,
                &row.get::<_, String>("ss_last_name")?,
            )),
            left_fielder: Arc::from(Player::min(
                row.get("lf_id")?,
                &row.get::<_, String>("lf_first_name")?,
                &row.get::<_, String>("lf_last_name")?,
            )),
            center_fielder: Arc::from(Player::min(
                row.get("cf_id")?,
                &row.get::<_, String>("cf_first_name")?,
                &row.get::<_, String>("cf_last_name")?,
            )),
            right_fielder: Arc::from(Player::min(
                row.get("rf_id")?,
                &row.get::<_, String>("rf_first_name")?,
                &row.get::<_, String>("rf_last_name")?,
            )),
            batter: Arc::from(Player::batter(
                row.get("b_id")?,
                &row.get::<_, String>("b_first_name")?,
                &row.get::<_, String>("b_last_name")?,
                row.get("ba")?,
                row.get("slg")?,
            )),
            point: row.get("point")?,
            out: row.get("out")?,
        };

        count.validate()?;

        Ok(count)
    }
}
