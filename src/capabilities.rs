use crate::{config::Settings, core::contracts::Error, models::ModelRegistry};
use serde_json::Value;

pub fn capabilities(settings: &Settings) -> Result<Value, Error> {
    let mut document: Value = serde_json::from_str(include_str!("../resources/capabilities.json"))
        .map_err(|_| Error::new("invalid_config", "Capability manifest is invalid."))?;
    document["release"] = serde_json::json!(env!("CARGO_PKG_VERSION"));
    let status = ModelRegistry::new(settings.clone()).verify("ppocr-v6-small")?;
    let rows = document["capabilities"]
        .as_array_mut()
        .ok_or_else(|| Error::new("invalid_config", "Capability manifest is invalid."))?;
    for row in rows {
        if row["model_id"] == "ppocr-v6-small" {
            row["availability"] = status["availability"].clone();
        }
    }
    Ok(document)
}
