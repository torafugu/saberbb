use crate::domain::shared::game::{BattingResult, GameType, InningType};
use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

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

impl ToSql for InningType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        let s = match self {
            InningType::Top => "Top",
            InningType::Bottom => "Bottom",
        };
        Ok(ToSqlOutput::from(s))
    }
}

impl FromSql for InningType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let tb = value.as_str()?;

        tb.parse::<InningType>().map_err(|e| {
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
