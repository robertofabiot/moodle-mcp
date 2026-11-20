use crate::moodle::error::{MoodleError, Result};
use serde_json::{Map, Value};

const RESERVED_KEYS: &[&str] = &["wstoken", "wsfunction", "moodlewsrestformat"];

/// Aplana un objeto JSON de parámetros al formato form-encoded esperado por el web service de Moodle.
///
/// Convierte arrays en `clave[0]`, objetos anidados en `clave[sub]`, booleanos en `"1"`/`"0"`,
/// números en cadena y omite valores `null`.
pub fn flatten_params(params: &Map<String, Value>) -> Result<Vec<(String, String)>> {
    let mut result = Vec::new();

    for (key, val) in params {
        if RESERVED_KEYS.contains(&key.as_str()) {
            return Err(MoodleError::InvalidArguments(format!(
                "La clave '{key}' es reservada del protocolo Moodle"
            )));
        }
        flatten_value(key, val, &mut result);
    }

    Ok(result)
}

fn flatten_value(prefix: &str, val: &Value, out: &mut Vec<(String, String)>) {
    match val {
        Value::Null => {}
        Value::Bool(b) => {
            out.push((prefix.to_string(), if *b { "1".into() } else { "0".into() }));
        }
        Value::Number(num) => {
            out.push((prefix.to_string(), num.to_string()));
        }
        Value::String(s) => {
            out.push((prefix.to_string(), s.clone()));
        }
        Value::Array(arr) => {
            for (idx, item) in arr.iter().enumerate() {
                let sub_prefix = format!("{prefix}[{idx}]");
                flatten_value(&sub_prefix, item, out);
            }
        }
        Value::Object(obj) => {
            for (sub_key, item) in obj {
                let sub_prefix = format!("{prefix}[{sub_key}]");
                flatten_value(&sub_prefix, item, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_flatten_params_basic_types() {
        let mut map = Map::new();
        map.insert("courseid".into(), json!(10));
        map.insert("active".into(), json!(true));
        map.insert("inactive".into(), json!(false));
        map.insert("ignored".into(), json!(null));
        map.insert("name".into(), json!("Matemáticas"));

        let res = flatten_params(&map).unwrap();
        assert!(res.contains(&("courseid".into(), "10".into())));
        assert!(res.contains(&("active".into(), "1".into())));
        assert!(res.contains(&("inactive".into(), "0".into())));
        assert!(res.contains(&("name".into(), "Matemáticas".into())));
        assert!(!res.iter().any(|(k, _)| k == "ignored"));
    }

    #[test]
    fn test_flatten_params_nested_object_and_array() {
        let mut map = Map::new();
        map.insert(
            "plugindata".into(),
            json!({
                "onlinetext_editor": {
                    "text": "Mi entrega de prueba",
                    "format": 1
                }
            }),
        );
        map.insert(
            "options".into(),
            json!([
                {"name": "opt1", "value": "val1"},
                {"name": "opt2", "value": "val2"}
            ]),
        );
        map.insert("courseids".into(), json!([5, 6, 7]));

        let res = flatten_params(&map).unwrap();

        assert!(res.contains(&(
            "plugindata[onlinetext_editor][text]".into(),
            "Mi entrega de prueba".into()
        )));
        assert!(res.contains(&("plugindata[onlinetext_editor][format]".into(), "1".into())));
        assert!(res.contains(&("options[0][name]".into(), "opt1".into())));
        assert!(res.contains(&("options[0][value]".into(), "val1".into())));
        assert!(res.contains(&("options[1][name]".into(), "opt2".into())));
        assert!(res.contains(&("options[1][value]".into(), "val2".into())));
        assert!(res.contains(&("courseids[0]".into(), "5".into())));
        assert!(res.contains(&("courseids[1]".into(), "6".into())));
        assert!(res.contains(&("courseids[2]".into(), "7".into())));
    }

    #[test]
    fn test_flatten_params_reserved_keys_error() {
        let mut map = Map::new();
        map.insert("wstoken".into(), json!("fake"));
        let err = flatten_params(&map).unwrap_err();
        assert!(matches!(err, MoodleError::InvalidArguments(_)));

        let mut map2 = Map::new();
        map2.insert("wsfunction".into(), json!("core_course_get_courses"));
        let err2 = flatten_params(&map2).unwrap_err();
        assert!(matches!(err2, MoodleError::InvalidArguments(_)));

        let mut map3 = Map::new();
        map3.insert("moodlewsrestformat".into(), json!("json"));
        let err3 = flatten_params(&map3).unwrap_err();
        assert!(matches!(err3, MoodleError::InvalidArguments(_)));
    }
}
