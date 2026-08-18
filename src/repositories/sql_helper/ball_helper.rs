use crate::domain::shared::ball::{OutboundResult, TrajectoryType};
use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

impl ToSql for TrajectoryType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for TrajectoryType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let gt = value.as_str()?;

        gt.parse::<TrajectoryType>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", gt, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}

impl ToSql for OutboundResult {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_ref()))
    }
}

impl FromSql for OutboundResult {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let outbound_result = value.as_str()?;

        outbound_result.parse::<OutboundResult>().map_err(|e| {
            eprintln!("{} {}: {:?}", "Parse error at ", outbound_result, e);
            rusqlite::types::FromSqlError::InvalidType
        })
    }
}
