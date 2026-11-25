use serde::Deserialize;

pub mod assignments;
pub mod browser;
pub mod courses;
pub mod files;
pub mod forums;
pub mod generic;
pub mod messaging;
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

/// Deserializa un valor tratando `null` explícito en JSON como `T::default()`.
pub(crate) fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    T: Default + serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    let opt = Option::<T>::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

/// Deserializa un booleano opcional aceptando `bool`, enteros (`0`/`1`) o `null`.
pub(crate) fn deserialize_opt_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
    match opt {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Bool(b)) => Ok(Some(b)),
        Some(serde_json::Value::Number(n)) => match n.as_i64() {
            Some(0) => Ok(Some(false)),
            Some(1) => Ok(Some(true)),
            _ => Err(serde::de::Error::custom(format!(
                "Valor entero booleano inválido: {n}"
            ))),
        },
        Some(serde_json::Value::String(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => Ok(Some(true)),
            "false" | "0" => Ok(Some(false)),
            "" => Ok(None),
            other => Err(serde::de::Error::custom(format!(
                "Valor textual booleano inválido: {other}"
            ))),
        },
        Some(other) => Err(serde::de::Error::custom(format!(
            "Tipo inesperado para booleano: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Deserialize)]
    struct DummyNullable {
        #[serde(default, deserialize_with = "deserialize_null_default")]
        text: String,
        #[serde(default, deserialize_with = "deserialize_null_default")]
        items: Vec<i64>,
        #[serde(default, deserialize_with = "deserialize_opt_bool")]
        flag: Option<bool>,
    }

    #[test]
    fn test_extract_warnings_variants() {
        assert_eq!(extract_warnings(&json!({})), None);
        assert_eq!(extract_warnings(&json!([])), None);
        assert_eq!(extract_warnings(&json!({"warnings": []})), None);

        let root_arr = json!([
            {"message": "  Primer aviso  ", "warningcode": "w1"},
            {"message": "   ", "warningcode": "w2"}
        ]);
        assert_eq!(
            extract_warnings(&root_arr).as_deref(),
            Some("Primer aviso; w2")
        );

        let nested = json!({
            "warnings": [
                {"warningcode": "cannotreply", "message": "Hilo cerrado"}
            ]
        });
        assert_eq!(extract_warnings(&nested).as_deref(), Some("Hilo cerrado"));
    }

    #[test]
    fn test_format_epoch_iso_boundaries() {
        assert_eq!(format_epoch_iso(0), None);
        assert_eq!(format_epoch_iso(-100), None);
        assert!(format_epoch_iso(1_700_000_000).is_some_and(|iso| iso.starts_with("2023-11-14T")));
    }

    #[test]
    fn test_deserializers_handle_null_and_numeric_bools() {
        let parsed: DummyNullable = serde_json::from_value(json!({
            "text": null,
            "items": null,
            "flag": 1
        }))
        .unwrap();
        assert_eq!(parsed.text, "");
        assert!(parsed.items.is_empty());
        assert_eq!(parsed.flag, Some(true));

        let parsed_zero: DummyNullable = serde_json::from_value(json!({
            "flag": 0
        }))
        .unwrap();
        assert_eq!(parsed_zero.flag, Some(false));

        let parsed_null: DummyNullable = serde_json::from_value(json!({
            "flag": null
        }))
        .unwrap();
        assert_eq!(parsed_null.flag, None);
    }
}
