use crate::moodle::error::MoodleError;
use rmcp::model::{CallToolResult, ContentBlock};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Sanea un texto reemplazando tokens y private_tokens conocidos por `<token>`.
pub fn scrub(text: &str, token: Option<&str>, private_token: Option<&str>) -> String {
    let mut result = text.to_string();
    if let Some(tok) = token {
        let tok = tok.trim();
        if !tok.is_empty() {
            result = result.replace(tok, "<token>");
        }
    }
    if let Some(ptok) = private_token {
        let ptok = ptok.trim();
        if !ptok.is_empty() {
            result = result.replace(ptok, "<token>");
        }
    }
    result
}

/// Estructura devuelta cuando una respuesta de herramienta excede `max_response_bytes`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TruncatedResponse {
    pub truncated: bool,
    pub saved_to: String,
    pub preview: String,
}

/// Serializa `value` a JSON y aplica sanitización (`scrub`) de tokens ANTES de medir su tamaño.
/// Si el tamaño excede `max_bytes`, guarda la respuesta completa saneada
/// en `download_dir/responses/<unix_ts_millis>-<function>.json` y retorna un JSON con
/// `{"truncated":true,"saved_to":…,"preview":…}` donde preview está cortado en un límite UTF-8 válido.
/// Si falla la creación del directorio o la escritura del archivo, devuelve un `CallToolResult` de error con `kind = "io"`.
pub fn ok_json<T: Serialize>(
    value: &T,
    max_bytes: usize,
    download_dir: &Path,
    function: &str,
    token: Option<&str>,
    private_token: Option<&str>,
) -> Result<String, CallToolResult> {
    let raw_serialized = serde_json::to_string(value).map_err(|e| {
        tool_error(
            "protocol",
            format!("Error al serializar respuesta: {e}"),
            None,
            None,
            None,
            token,
            private_token,
        )
    })?;

    // Aplicar scrub ANTES de medir, guardar en disco y cortar preview
    let serialized = scrub(&raw_serialized, token, private_token);

    if serialized.len() <= max_bytes {
        return Ok(serialized);
    }

    let responses_dir = download_dir.join("responses");
    if let Err(e) = std::fs::create_dir_all(&responses_dir) {
        tracing::error!("Error al crear directorio de respuestas: {e}");
        return Err(tool_error(
            "io",
            format!("Error al crear directorio de respuestas: {e}"),
            None,
            None,
            None,
            token,
            private_token,
        ));
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    let file_path = responses_dir.join(format!("{now_ms}-{function}.json"));
    if let Err(e) = std::fs::write(&file_path, &serialized) {
        tracing::error!("Error al guardar respuesta truncada: {e}");
        return Err(tool_error(
            "io",
            format!("Error al guardar respuesta truncada: {e}"),
            None,
            None,
            None,
            token,
            private_token,
        ));
    }

    // Truncar exactamente en un límite de carácter UTF-8 válido
    let mut cut = max_bytes;
    while cut > 0 && !serialized.is_char_boundary(cut) {
        cut -= 1;
    }
    let preview = &serialized[..cut];

    let truncated = TruncatedResponse {
        truncated: true,
        saved_to: file_path.to_string_lossy().to_string(),
        preview: preview.to_string(),
    };

    serde_json::to_string(&truncated).map_err(|e| {
        tool_error(
            "protocol",
            format!("Error al serializar respuesta truncada: {e}"),
            None,
            None,
            None,
            token,
            private_token,
        )
    })
}

/// Cuerpo de error normalizado según el estándar MCP del proyecto.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolErrorBody {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errorcode: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debuginfo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Envoltorio estándar de error de herramientas MCP.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolErrorEnvelope {
    pub error: ToolErrorBody,
}

/// Genera un resultado de error normalizado de MCP con los campos saneados (`scrub`).
pub fn tool_error(
    kind: impl Into<String>,
    message: impl Into<String>,
    errorcode: Option<String>,
    debuginfo: Option<String>,
    hint: Option<String>,
    token: Option<&str>,
    private_token: Option<&str>,
) -> CallToolResult {
    let body = ToolErrorBody {
        kind: kind.into(),
        errorcode,
        message: message.into(),
        debuginfo,
        hint,
    };
    let envelope = ToolErrorEnvelope { error: body };
    let raw = serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string());
    let scrubbed = scrub(&raw, token, private_token);
    CallToolResult::error(vec![ContentBlock::text(scrubbed)])
}

/// Genera un `CallToolResult::success` conteniendo la salida formateada y saneada (`scrub`).
pub fn tool_success(
    json_text: &str,
    token: Option<&str>,
    private_token: Option<&str>,
) -> CallToolResult {
    let scrubbed = scrub(json_text, token, private_token);
    CallToolResult::success(vec![ContentBlock::text(scrubbed)])
}

/// Serializa un valor JSON de metadatos, lo sanea con `scrub` y devuelve un `CallToolResult`
/// con un bloque de texto JSON seguido de un `ContentBlock::image` con base64 crudo (sin prefijo `data:...`).
pub fn ok_json_with_image<T: Serialize>(
    value: &T,
    base64_data: String,
    mime: &str,
    token: Option<&str>,
    private_token: Option<&str>,
) -> CallToolResult {
    let raw_serialized = match serde_json::to_string(value) {
        Ok(s) => s,
        Err(e) => {
            return tool_error(
                "protocol",
                format!("Error al serializar respuesta: {e}"),
                None,
                None,
                None,
                token,
                private_token,
            );
        }
    };

    let scrubbed = scrub(&raw_serialized, token, private_token);
    CallToolResult::success(vec![
        ContentBlock::text(scrubbed),
        ContentBlock::image(base64_data, mime),
    ])
}

/// Convierte un `MoodleError` en una respuesta `CallToolResult::error` normalizada.
pub fn tool_error_from_moodle(
    err: &MoodleError,
    token: Option<&str>,
    private_token: Option<&str>,
) -> CallToolResult {
    let (kind, message, errorcode, debuginfo, hint) = match err {
        MoodleError::InvalidToken => (
            "invalid_token",
            "Token de Moodle inválido o expirado".to_string(),
            Some("invalidtoken".to_string()),
            None,
            Some("Ejecuta 'moodle-mcp login' para renovar el token".to_string()),
        ),
        MoodleError::InvalidLogin => (
            "access_denied",
            "Credenciales UAM inválidas".to_string(),
            Some("invalidlogin".to_string()),
            None,
            Some("Verifica tu usuario y contraseña".to_string()),
        ),
        MoodleError::AccessDenied(msg) => (
            "access_denied",
            format!("Acceso denegado en Moodle: {msg}"),
            None,
            None,
            None,
        ),
        MoodleError::Maintenance => (
            "maintenance",
            "Moodle se encuentra en mantenimiento o no disponible".to_string(),
            None,
            None,
            Some("Espera unos minutos y vuelve a intentarlo".to_string()),
        ),
        MoodleError::Timeout => (
            "timeout",
            "Tiempo de espera agotado conectando a Moodle".to_string(),
            None,
            None,
            None,
        ),
        MoodleError::Network(msg) => (
            "network",
            format!("Error de red conectando a Moodle: {msg}"),
            None,
            None,
            None,
        ),
        MoodleError::Protocol(msg) => (
            "protocol",
            format!("Error de protocolo Moodle: {msg}"),
            None,
            None,
            None,
        ),
        MoodleError::Api {
            errorcode,
            message,
            debuginfo,
        } => (
            "api",
            message.clone(),
            Some(errorcode.clone()),
            debuginfo.clone(),
            None,
        ),
        MoodleError::InvalidArguments(msg) => ("invalid_arguments", msg.clone(), None, None, None),
        MoodleError::Io(msg) => ("io", format!("Error de E/S: {msg}"), None, None, None),
    };

    tool_error(
        kind,
        message,
        errorcode,
        debuginfo,
        hint,
        token,
        private_token,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_scrub_replaces_tokens() {
        let text = "Result with tok_12345 and secret_priv";
        let scrubbed = scrub(text, Some("tok_12345"), Some("secret_priv"));
        assert_eq!(scrubbed, "Result with <token> and <token>");

        let no_tokens = scrub(text, None, None);
        assert_eq!(no_tokens, text);
    }

    #[test]
    fn test_ok_json_truncation() {
        let dir = tempdir().unwrap();
        let download_dir = dir.path();

        let large_data = serde_json::json!({
            "items": vec!["test_item_abcdefghij"; 20]
        });

        let json_res = ok_json(&large_data, 50, download_dir, "test_func", None, None).unwrap();
        let parsed: TruncatedResponse = serde_json::from_str(&json_res).unwrap();
        assert!(parsed.truncated);
        assert!(std::path::Path::new(&parsed.saved_to).exists());
        assert!(parsed.preview.len() <= 50);

        // Archivo guardado debe contener el JSON completo
        let file_contents = std::fs::read_to_string(&parsed.saved_to).unwrap();
        assert_eq!(file_contents, serde_json::to_string(&large_data).unwrap());
    }

    #[test]
    fn test_ok_json_truncation_scrubs_tokens_before_saving_and_preview() {
        let dir = tempdir().unwrap();
        let download_dir = dir.path();

        let large_data = serde_json::json!({
            "token": "secret_token_12345",
            "private": "secret_priv_67890",
            "items": vec!["filler_data_to_exceed_limits"; 10]
        });

        let json_res = ok_json(
            &large_data,
            50,
            download_dir,
            "test_scrub",
            Some("secret_token_12345"),
            Some("secret_priv_67890"),
        )
        .unwrap();

        let parsed: TruncatedResponse = serde_json::from_str(&json_res).unwrap();
        assert!(parsed.truncated);
        assert!(!parsed.preview.contains("secret_token_12345"));
        assert!(!parsed.preview.contains("secret_priv_67890"));

        let file_contents = std::fs::read_to_string(&parsed.saved_to).unwrap();
        assert!(!file_contents.contains("secret_token_12345"));
        assert!(!file_contents.contains("secret_priv_67890"));
        assert!(file_contents.contains("<token>"));
    }

    #[test]
    fn test_tool_error_format() {
        let result = tool_error(
            "invalid_arguments",
            "Parámetro incorrecto",
            Some("invalidparam".to_string()),
            Some("debug line".to_string()),
            Some("hint text".to_string()),
            Some("tok"),
            None,
        );

        let content = &result.content[0];
        let text = content.as_text().unwrap();
        let parsed: ToolErrorEnvelope = serde_json::from_str(&text.text).unwrap();
        assert_eq!(parsed.error.kind, "invalid_arguments");
        assert_eq!(parsed.error.errorcode.as_deref(), Some("invalidparam"));
        assert_eq!(parsed.error.debuginfo.as_deref(), Some("debug line"));
        assert_eq!(parsed.error.hint.as_deref(), Some("hint text"));
    }

    #[test]
    fn test_ok_json_with_image_scrubs_metadata_and_embeds_raw_base64() {
        let meta = serde_json::json!({
            "path": "/tmp/img.png?token=tok_secreto",
            "mime": "image/png",
            "size": 4,
            "inline_image": true
        });
        let result = ok_json_with_image(
            &meta,
            "iVBORw0=".to_string(),
            "image/png",
            Some("tok_secreto"),
            None,
        );

        assert!(!result.is_error.unwrap_or(false));
        assert_eq!(result.content.len(), 2);

        let text = result.content[0].as_text().unwrap();
        assert!(!text.text.contains("tok_secreto"));
        assert!(text.text.contains("<token>"));

        let img = result.content[1].as_image().unwrap();
        assert_eq!(img.data, "iVBORw0=");
        assert_eq!(img.mime_type, "image/png");
    }
}
