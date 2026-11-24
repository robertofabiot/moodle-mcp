pub mod assignments;
pub mod courses;
pub mod files;
pub mod forums;
pub mod generic;
pub mod quiz;
pub mod sync;

/// Extrae mensajes o códigos de advertencia devueltos por Moodle en un array raíz o en el campo `warnings`.
pub(crate) fn extract_warnings(val: &serde_json::Value) -> Option<String> {
    let items = val
        .as_array()
        .or_else(|| val.get("warnings").and_then(|w| w.as_array()))?;

    let mut warnings = Vec::new();
    for item in items {
        if let Some(msg) = item
            .get("message")
            .and_then(|m| m.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            warnings.push(msg.to_string());
        } else if let Some(code) = item
            .get("warningcode")
            .and_then(|c| c.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            warnings.push(code.to_string());
        }
    }

    if warnings.is_empty() {
        None
    } else {
        Some(warnings.join("; "))
    }
}

/// Convierte un timestamp Unix epoch positivo a su representación ISO 8601 (RFC 3339).
pub(crate) fn format_epoch_iso(epoch: i64) -> Option<String> {
    if epoch > 0 {
        chrono::DateTime::from_timestamp(epoch, 0).map(|dt| dt.to_rfc3339())
    } else {
        None
    }
}
