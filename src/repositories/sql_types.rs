use crate::domain::shared::game::GameType;
use crate::domain::shared::player::{PitchType, RL};
use crate::domain::shared::types::{BattingResult, InningType, Position};
use crate::t;
use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

impl ToSql for Position {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        let s = match self {
            Position::P => "P",
            Position::C => "C",
            Position::FB => "1B",
            Position::SB => "2B",
            Position::TB => "3B",
            Position::SS => "SS",
            Position::LF => "LF",
            Position::CF => "CF",
            Position::RF => "RF",
            Position::DH => "DH",
        };
        Ok(ToSqlOutput::from(s))
    }
}

pub struct SqlPosition(pub Position);
impl FromSql for SqlPosition {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<Position>().map(SqlPosition).map_err(|e| {
            eprintln!("{} {}: {:?}", t!("error_parse"), gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl ToSql for RL {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        let s = match self {
            RL::Right => "Right",
            RL::Left => "Left",
        };
        Ok(ToSqlOutput::from(s))
    }
}

pub struct SqlRL(pub RL);
impl FromSql for SqlRL {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<RL>().map(SqlRL).map_err(|e| {
            eprintln!("{} {}: {:?}", t!("error_parse"), gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl ToSql for PitchType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        let s = match self {
            PitchType::FourSeamFastball => "FourSeamFastball",
            PitchType::TwoSeamFastball => "TwoSeamFastballC",
            PitchType::Cutter => "Cutter",
            PitchType::Curveball => "Curveball",
            PitchType::Slider => "Slider",
            PitchType::Sweeper => "Sweeper",
            PitchType::Changeup => "Changeup",
            PitchType::Forkball => "Forkball",
            PitchType::SplitFingerFastball => "SplitFingerFastball",
            PitchType::Knuckleball => "PitchType::Knuckleball",
        };
        Ok(ToSqlOutput::from(s))
    }
}

pub struct SqlPitchType(pub PitchType);
impl FromSql for SqlPitchType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<PitchType>().map(SqlPitchType).map_err(|e| {
            eprintln!("{} {}: {:?}", t!("error_parse"), gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

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

pub struct SqlGameType(pub GameType);
impl FromSql for SqlGameType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<GameType>().map(SqlGameType).map_err(|e| {
            eprintln!("{} {}: {:?}", t!("error_parse"), gt, e);
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

pub struct SqlInningType(pub InningType);
impl FromSql for SqlInningType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let tb = value.as_str()?;

        tb.parse::<InningType>().map(SqlInningType).map_err(|e| {
            eprintln!("{} {}: {:?}", t!("error_parse"), tb, e);
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

pub struct SqlBattingResult(pub BattingResult);
impl FromSql for SqlBattingResult {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let br = value.as_str()?;

        br.parse::<BattingResult>()
            .map(SqlBattingResult)
            .map_err(|e| {
                eprintln!("{} {}: {:?}", t!("error_parse"), br, e);
                rusqlite::types::FromSqlError::InvalidType
            })
    }
}
