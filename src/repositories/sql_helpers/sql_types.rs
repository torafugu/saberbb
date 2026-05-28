use crate::domain::shared::game::GameType;
use crate::domain::shared::player::PitchType;
use crate::domain::shared::probabilities::{
    BatterSkillProb, DefensiveSkillProb, ItemProb, PitchSkillProb, PitcherAttributeProb,
    PlayerAttributeProb,
};

use crate::domain::shared::team::Team;
use crate::domain::shared::types::{BattingResult, InningType};
use crate::error::AppError;
use crate::repositories::db::{FromRow, FromRowWithCtx};
use crate::t;
use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
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

impl FromRow for PlayerAttributeProb {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let player_attribute_prob = PlayerAttributeProb {
            age_shape: row.get("age_shape")?,
            age_scale: row.get("age_scale")?,
            age_offset: row.get("age_offset")?,
            throw_lefty: row.get("throw_lefty")?,
            bat_lefty: row.get("bat_lefty")?,
        };

        player_attribute_prob.validate()?;

        Ok(player_attribute_prob)
    }
}

impl FromRow for BatterSkillProb {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let batter_skill_prob = BatterSkillProb {
            ba_skew: row.get("ba_skew")?,
            slg_skew: row.get("slg_skew")?,
        };

        batter_skill_prob.validate()?;

        Ok(batter_skill_prob)
    }
}

impl FromRow for DefensiveSkillProb {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let defensive_skill_prob = DefensiveSkillProb {
            uzr_skew: row.get("uzr_skew")?,
        };

        defensive_skill_prob.validate()?;

        Ok(defensive_skill_prob)
    }
}

impl FromRow for PitcherAttributeProb {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let pitcher_attribute_prob = PitcherAttributeProb {
            velocity_skew: row.get("velocity_skew")?,
            control_skew: row.get("control_skew")?,
            stamina_skew: row.get("stamina_skew")?,
            injury_proneness_skew: row.get("injury_proneness_skew")?,
            clutch_skew: row.get("clutch_skew")?,
            hpp_skew: row.get("hpp_skew")?,
            platoon_splitting_skew: row.get("platoon_splitting_skew")?,
        };

        pitcher_attribute_prob.validate()?;

        Ok(pitcher_attribute_prob)
    }
}

impl FromRowWithCtx<PitchType> for PitchSkillProb {
    type Error = AppError;

    fn from_row_with_ctx(row: &rusqlite::Row, ctx: &PitchType) -> Result<Self, Self::Error> {
        let pitch_skill_prob = PitchSkillProb {
            pitch_type: ctx.clone(),
            velocity_skew: row.get("velocity_skew")?,
            control_skew: row.get("control_skew")?,
            stamina_skew: row.get("stamina_skew")?,
            injury_proneness_skew: row.get("injury_proneness_skew")?,
            stuff_skew: row.get("stuff_skew")?,
            fb_skew: row.get("fb_skew")?,
            gp_skew: row.get("gp_skew")?,
            horizontal_movement_skew: row.get("horizontal_movement_skew")?,
            vertical_movement_skew: row.get("vertical_movement_skew")?,
            spin_rate_skew: row.get("spin_rate_skew")?,
            usage_skew: row.get("usage_skew")?,
        };

        pitch_skill_prob.validate()?;

        Ok(pitch_skill_prob)
    }
}

impl<T: FromSql> FromRow for ItemProb<T> {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let item_prob = ItemProb {
            name: row.get("name").map_err(|e| AppError::Database(e))?,
            prob: row.get("prob").map_err(|e| AppError::Database(e))?,
        };

        item_prob.validate()?;

        Ok(item_prob)
    }
}

impl FromRow for Team {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let team = Team {
            id: row.get("id")?,
            name: row.get("name")?,
            players: Vec::new(),
        };

        team.validate()?;

        Ok(team)
    }
}
