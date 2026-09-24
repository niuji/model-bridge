use anyhow::{anyhow, Result};
use serde_json::Value;

pub(super) fn parse_models(data: Value) -> Result<Vec<(String, String)>> {
    let models = data
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Invalid subscription model catalog"))?;
    let mut result = Vec::new();
    for model in models {
        if model
            .get("visibility")
            .and_then(Value::as_str)
            .is_some_and(|v| v != "list")
            || model.get("show_in_picker").and_then(Value::as_bool) == Some(false)
        {
            continue;
        }
        let id = model
            .get("slug")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| anyhow!("Invalid subscription model entry"))?;
        let name = model
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or(id);
        result.push((id.to_owned(), name.to_owned()));
    }
    result.sort();
    result.dedup_by(|a, b| a.0 == b.0);
    Ok(result)
}
