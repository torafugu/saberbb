use crate::domain::shared::player::{FullName, PitchType, PitcherStyle, Player, Position, RL};
use crate::error::AppError;
use crate::repositories::db::FromRow;
use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use validator::Validate;

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

impl FromSql for Position {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<Position>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", gt, e);
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

impl FromSql for RL {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<RL>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl FromRow for FullName {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let full_name = FullName {
            first: row.get("first_name")?,
            last: row.get("last_name")?,
        };

        full_name.validate()?;

        Ok(full_name)
    }
}

impl ToSql for PitchType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        let s = match self {
            PitchType::FourSeamFastball => "FourSeamFastball",
            PitchType::TwoSeamFastball => "TwoSeamFastball",
            PitchType::Cutter => "Cutter",
            PitchType::Curveball => "Curveball",
            PitchType::Slider => "Slider",
            PitchType::Sweeper => "Sweeper",
            PitchType::Changeup => "Changeup",
            PitchType::Forkball => "Forkball",
            PitchType::SplitFingerFastball => "SplitFingerFastball",
            PitchType::Knuckleball => "Knuckleball",
        };
        Ok(ToSqlOutput::from(s))
    }
}

impl FromSql for PitchType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<PitchType>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl FromSql for PitcherStyle {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<PitcherStyle>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl ToSql for PitcherStyle {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        let s = match self {
            PitcherStyle::PowerPitcher => "PowerPitcher",
            PitcherStyle::FinessePitcher => "FinessePitcher",
            PitcherStyle::BalancedPitcher => "BalancedPitcher",
        };
        Ok(ToSqlOutput::from(s))
    }
}

impl FromRow for Player {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let player = Player::batter(
            row.get("id")?,
            &row.get::<_, String>("first_name")?,
            &row.get::<_, String>("last_name")?,
            row.get("mod_ba")?,
            row.get("mod_slg")?,
        );

        player.validate()?;

        Ok(player)
    }
}
