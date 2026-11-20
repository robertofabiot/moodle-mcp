use crate::moodle::client::DEFAULT_BASE_URL;
use std::path::PathBuf;

pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 60_000;
pub const DEFAULT_MAX_INLINE_IMAGE_BYTES: usize = 1_000_000;

/// Configuración operativa para moodle-mcp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// URL base de Moodle (normalizada sin barra final).
    pub base_url: String,
    /// Directorio local de descargas de archivos.
    pub download_dir: PathBuf,
    /// Límite de bytes en respuestas de herramientas antes de truncar/guardar a archivo.
    pub max_response_bytes: usize,
    /// Límite de bytes para imágenes en línea en respuestas MCP.
    pub max_inline_image_bytes: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self::from_env()
    }
}

impl Config {
    /// Construye la configuración leyendo variables de entorno con valores por defecto.
    pub fn from_env() -> Self {
        let raw_base =
            std::env::var("MOODLE_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let base_url = normalize_base_url(&raw_base);

        let download_dir = std::env::var("MOODLE_MCP_DOWNLOAD_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::download_dir()
                    .or_else(dirs::home_dir)
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("moodle-mcp")
            });

        let max_response_bytes = std::env::var("MOODLE_MCP_MAX_RESPONSE_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_RESPONSE_BYTES);

        let max_inline_image_bytes = std::env::var("MOODLE_MCP_MAX_INLINE_IMAGE_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_INLINE_IMAGE_BYTES);

        Self {
            base_url,
            download_dir,
            max_response_bytes,
            max_inline_image_bytes,
        }
    }
}

/// Normaliza una URL base eliminando cualquier barra final sobrante.
pub fn normalize_base_url(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_base_url() {
        assert_eq!(
            normalize_base_url("https://uamvirtual.uam.edu.ni/grado/"),
            "https://uamvirtual.uam.edu.ni/grado"
        );
        assert_eq!(
            normalize_base_url("https://uamvirtual.uam.edu.ni/grado///"),
            "https://uamvirtual.uam.edu.ni/grado"
        );
        assert_eq!(
            normalize_base_url("https://uamvirtual.uam.edu.ni/grado"),
            "https://uamvirtual.uam.edu.ni/grado"
        );
    }
}
