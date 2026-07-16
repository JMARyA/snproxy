pub mod csv;
pub mod json;
pub mod table;

use crate::error::Result;
use serde_json::Value;

pub enum Format {
    Csv(Option<String>),
    Json(Option<String>),
    Table,
}

pub async fn write(_rows: Vec<Value>, _format: Format) -> Result<()> {
    Err(crate::error::SnpipeError::other("output not yet implemented"))
}

pub fn write_csv(_rows: &[Value], _path: Option<&str>) -> Result<()> {
    todo!("CSV output not yet implemented")
}

pub fn write_json(_rows: &[Value], _path: Option<&str>) -> Result<()> {
    todo!("JSON output not yet implemented")
}

pub fn write_table(_rows: &[Value]) -> Result<()> {
    todo!("table output not yet implemented")
}
