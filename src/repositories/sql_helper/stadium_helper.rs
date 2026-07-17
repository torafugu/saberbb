use crate::domain::shared::stadium::Stadium;
use crate::error::AppError;
use crate::repositories::db::FromRow;
use validator::Validate;

impl FromRow for Stadium {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let id = row.get("id").map_err(AppError::Database)?;
        let name = row.get("name").map_err(AppError::Database)?;
        let foul_pole_distance = row.get("foul_pole_distance").map_err(AppError::Database)?;
        let center_fence_distance = row
            .get("center_fence_distance")
            .map_err(AppError::Database)?;
        let fence_height = row.get("fence_height").map_err(AppError::Database)?;

        let mut stadium = Stadium::new(
            id,
            name,
            foul_pole_distance,
            center_fence_distance,
            fence_height,
        );

        let fence_line_json: Option<String> = row.get("fence_line").map_err(AppError::Database)?;
        if let Some(fence_line_json) = fence_line_json {
            stadium.fence_line = serde_json::from_str(&fence_line_json).map_err(|e| {
                AppError::Internal(anyhow::anyhow!(
                    "failed to deserialize stadium fence_line: {}",
                    e
                ))
            })?;
        }

        stadium.validate()?;

        Ok(stadium)
    }
}
