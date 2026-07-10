use crate::domain::shared::prob::{GammaParam, ItemWeighted, NormalParam};
use crate::error::AppError;
use crate::repositories::db::FromRow;
use rusqlite::types::FromSql;
use validator::Validate;

impl FromRow for NormalParam {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let normal_param = NormalParam {
            mean: row.get("mean").map_err(|e| AppError::Database(e))?,
            std_dev: row.get("std_dev").map_err(|e| AppError::Database(e))?,
            skew: row.get("skew").map_err(|e| AppError::Database(e))?,
            coefficient: row.get("coefficient").map_err(|e| AppError::Database(e))?,
            offset: row.get("offset").map_err(|e| AppError::Database(e))?,
        };

        normal_param.validate()?;

        Ok(normal_param)
    }
}

impl FromRow for GammaParam {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let gamma_param = GammaParam {
            shape: row.get("shape").map_err(|e| AppError::Database(e))?,
            scale: row.get("scale").map_err(|e| AppError::Database(e))?,
            offset: row.get("offset").map_err(|e| AppError::Database(e))?,
        };

        gamma_param.validate()?;

        Ok(gamma_param)
    }
}

impl<T: FromSql> FromRow for ItemWeighted<T> {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let item_prob = ItemWeighted {
            name: row.get("name").map_err(|e| AppError::Database(e))?,
            weight: row.get("prob").map_err(|e| AppError::Database(e))?,
        };

        item_prob.validate()?;

        Ok(item_prob)
    }
}
