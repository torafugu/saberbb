use crate::domain::shared::player::PitchType;
use crate::domain::shared::prob::{
    BatterSkillProb, DefensiveSkillProb, ItemProb, PitchSkillProb, PitcherAttributeProb,
    PlayerAttributeProb,
};
use crate::error::AppError;
use crate::repositories::db::{FromRow, FromRowWithCtx};
use rusqlite::types::FromSql;
use validator::Validate;

impl FromRow for PlayerAttributeProb {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let player_attribute_prob = PlayerAttributeProb {
            age_shape: row.get("age_shape").map_err(|e| AppError::Database(e))?,
            age_scale: row.get("age_scale").map_err(|e| AppError::Database(e))?,
            age_offset: row.get("age_offset").map_err(|e| AppError::Database(e))?,
            throw_lefty: row.get("throw_lefty").map_err(|e| AppError::Database(e))?,
            bat_lefty: row.get("bat_lefty").map_err(|e| AppError::Database(e))?,
        };

        player_attribute_prob.validate()?;

        Ok(player_attribute_prob)
    }
}

impl FromRow for BatterSkillProb {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let batter_skill_prob = BatterSkillProb {
            ba_skew: row.get("ba_skew").map_err(|e| AppError::Database(e))?,
            slg_skew: row.get("slg_skew").map_err(|e| AppError::Database(e))?,
        };

        batter_skill_prob.validate()?;

        Ok(batter_skill_prob)
    }
}

impl FromRow for DefensiveSkillProb {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let defensive_skill_prob = DefensiveSkillProb {
            uzr_skew: row.get("uzr_skew").map_err(|e| AppError::Database(e))?,
        };

        defensive_skill_prob.validate()?;

        Ok(defensive_skill_prob)
    }
}

impl FromRow for PitcherAttributeProb {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let pitcher_attribute_prob = PitcherAttributeProb {
            velocity_skew: row
                .get("velocity_skew")
                .map_err(|e| AppError::Database(e))?,
            control_skew: row.get("control_skew").map_err(|e| AppError::Database(e))?,
            stamina_skew: row.get("stamina_skew").map_err(|e| AppError::Database(e))?,
            injury_proneness_skew: row
                .get("injury_proneness_skew")
                .map_err(|e| AppError::Database(e))?,
            clutch_skew: row.get("clutch_skew").map_err(|e| AppError::Database(e))?,
            hpp_skew: row.get("hpp_skew").map_err(|e| AppError::Database(e))?,
            platoon_splitting_skew: row
                .get("platoon_splitting_skew")
                .map_err(|e| AppError::Database(e))?,
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
            velocity_skew: row
                .get("velocity_skew")
                .map_err(|e| AppError::Database(e))?,
            control_skew: row.get("control_skew").map_err(|e| AppError::Database(e))?,
            stamina_skew: row.get("stamina_skew").map_err(|e| AppError::Database(e))?,
            injury_proneness_skew: row
                .get("injury_proneness_skew")
                .map_err(|e| AppError::Database(e))?,
            stuff_skew: row.get("stuff_skew").map_err(|e| AppError::Database(e))?,
            fb_skew: row.get("fb_skew").map_err(|e| AppError::Database(e))?,
            gp_skew: row.get("gp_skew").map_err(|e| AppError::Database(e))?,
            horizontal_movement_skew: row
                .get("horizontal_movement_skew")
                .map_err(|e| AppError::Database(e))?,
            vertical_movement_skew: row
                .get("vertical_movement_skew")
                .map_err(|e| AppError::Database(e))?,
            spin_rate_skew: row
                .get("spin_rate_skew")
                .map_err(|e| AppError::Database(e))?,
            usage_skew: row.get("usage_skew").map_err(|e| AppError::Database(e))?,
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
