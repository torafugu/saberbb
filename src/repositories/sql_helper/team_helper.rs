use crate::domain::shared::team::Team;
use crate::error::AppError;
use crate::repositories::db::FromRow;
use validator::Validate;

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
