use crate::moodle::client::MoodleClient;
use crate::moodle::error::{MoodleError, Result};
use crate::moodle::gate::Priority;
use crate::moodle::params::flatten_params;
use serde_json::{Map, Value};

impl MoodleClient {
    /// Invoca una función arbitraria del web service de Moodle con parámetros JSON.
    ///
    /// Valida que el nombre de la función cumpla `^[a-z][a-z0-9_]*$`, aplana los parámetros
    /// al formato form-encoded de PHP y ejecuta la petición con prioridad interactiva.
    pub async fn call_raw(
        &self,
        token: &str,
        function: &str,
        params: &Map<String, Value>,
    ) -> Result<Value> {
        validate_function_name(function)?;

        let mut flattened = flatten_params(params)?;
        flattened.insert(0, ("wstoken".to_string(), token.to_string()));
        flattened.insert(1, ("wsfunction".to_string(), function.to_string()));
        flattened.insert(2, ("moodlewsrestformat".to_string(), "json".to_string()));

        let pairs: Vec<(&str, &str)> = flattened
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        self.post_webservice(&pairs, Priority::Interactive).await
    }
}

fn validate_function_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => {
            return Err(MoodleError::InvalidArguments(format!(
                "Nombre de función de Moodle inválido: '{name}' (debe empezar con letra minúscula [a-z])"
            )));
        }
    }

    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Err(MoodleError::InvalidArguments(format!(
            "Nombre de función de Moodle inválido: '{name}' (solo permite [a-z0-9_])"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_function_name() {
        assert!(validate_function_name("core_course_get_courses").is_ok());
        assert!(validate_function_name("mod_assign_save_submission").is_ok());
        assert!(validate_function_name("core1_test_2").is_ok());

        assert!(validate_function_name("").is_err());
        assert!(validate_function_name("Core_course").is_err());
        assert!(validate_function_name("1core_course").is_err());
        assert!(validate_function_name("_core_course").is_err());
        assert!(validate_function_name("core-course").is_err());
        assert!(validate_function_name("core course").is_err());
    }
}
