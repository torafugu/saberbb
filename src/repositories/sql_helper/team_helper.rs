use crate::domain::shared::team::Team;
use crate::error::AppError;
use crate::repositories::db::FromRow;
use validator::Validate;

impl FromRow for Team {
    type Error = AppError;

    fn from_row(row: &rusqlite::Row) -> Result<Self, Self::Error> {
        let team = Team {
            id: row.get("id").map_err(|e| AppError::Database(e))?,
            name: row.get("name").map_err(|e| AppError::Database(e))?,
            players: Vec::new(),
        };

        team.validate()?;

        Ok(team)
    }
}
