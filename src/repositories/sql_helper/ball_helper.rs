use crate::domain::shared::ball::TrajectoryType;
use rusqlite::types::{ToSql, ToSqlOutput};

impl ToSql for TrajectoryType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}
