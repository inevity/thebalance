use anyhow::Result;
use serde::de::DeserializeOwned;
use toasty::Model;
use worker::D1Result;

/// Map D1 results to Toasty model instances
pub fn map_d1_results<M: Model + DeserializeOwned>(result: D1Result) -> Result<Vec<M>> {
    // D1Result has a results() method that deserializes to the specified type
    let models: Vec<M> = result.results()?;
    Ok(models)
}

/// Map a single D1 row to a model instance
pub fn map_d1_row<M: Model + DeserializeOwned>(
    row: serde_json::Value,
) -> Result<M> {
    let model: M = serde_json::from_value(row)?;
    Ok(model)
}

/// Convert D1 result metadata to useful information
pub struct D1ResultInfo {
    pub rows_read: u64,
    pub rows_written: u64,
    pub duration: f64,
}

impl From<D1Result> for D1ResultInfo {
    fn from(result: D1Result) -> Self {
        match result.meta() {
            Ok(Some(meta)) => Self {
                rows_read: meta.rows_read.unwrap_or(0) as u64,
                rows_written: meta.rows_written.unwrap_or(0) as u64,
                duration: meta.duration.unwrap_or(0.0),
            },
            Ok(None) | Err(_) => Self {
                rows_read: 0,
                rows_written: 0,
                duration: 0.0,
            }
        }
    }
}