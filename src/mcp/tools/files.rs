use crate::mcp::server::MoodleMcp;
use crate::moodle::error::MoodleError;
use crate::moodle::files::authorize_file_url;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DownloadParams {
    /// URL del archivo en Moodle (ej. pluginfile.php)
    pub url: String,
    /// Ruta de destino opcional (relativa a la carpeta de descargas o absoluta)
    pub dest: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UploadParams {
    /// Lista de rutas de archivos locales a subir
    pub paths: Vec<String>,
    /// ID del área de borrador (0 o nulo para generar uno nuevo)
    pub itemid: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct DownloadOutput {
    pub path: String,
    pub mime: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_markdown: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UploadOutput {
    pub itemid: i64,
    pub files: Vec<String>,
}

#[tool_router(router = files_router, vis = "pub(crate)")]
impl MoodleMcp {
    /// Descarga cualquier archivo de Moodle (recursos, tareas, adjuntos) a disco local de manera segura.
    #[rmcp::tool(
        name = "moodle_download",
        description = "Descarga cualquier archivo de Moodle (recursos, tareas, adjuntos) a disco local de manera segura",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn moodle_download(&self, params: Parameters<DownloadParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let base_url = match Url::parse(&creds.base_url) {
            Ok(u) => u,
            Err(e) => {
                return self.ctx.tool_error(
                    "protocol",
                    format!("URL base del sitio inválida: {e}"),
                    None,
                    None,
                    None,
                    Some(&creds),
                );
            }
        };

        let authorized_url = match authorize_file_url(&base_url, &params.0.url, &creds.token) {
            Ok(u) => u,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let dest_path = params.0.dest.as_deref().map(Path::new);

        let downloaded = match self
            .ctx
            .file_client
            .download(&authorized_url, dest_path, &self.ctx.config.download_dir)
            .await
        {
            Ok(df) => df,
            Err(MoodleError::InvalidToken) => {
                return self.ctx.handle_invalid_token().await;
            }
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let mut content_markdown = None;
        let is_textual = downloaded.mime == "text/html" || downloaded.mime.starts_with("text/");
        if is_textual && downloaded.size < self.ctx.config.max_response_bytes as u64 {
            if let Ok(raw_bytes) = tokio::fs::read(&downloaded.path).await {
                if let Ok(text_content) = String::from_utf8(raw_bytes) {
                    if downloaded.mime == "text/html" {
                        content_markdown = Some(crate::content::html_to_markdown(&text_content));
                    } else {
                        content_markdown = Some(text_content);
                    }
                }
            }
        }

        let output = DownloadOutput {
            path: downloaded.path.to_string_lossy().into_owned(),
            mime: downloaded.mime,
            size: downloaded.size,
            content_markdown,
        };

        self.ctx.ok_json(&output, "moodle_download", Some(&creds))
    }

    /// Sube uno o más archivos locales al área de borrador de Moodle para entregas o foros.
    #[rmcp::tool(
        name = "moodle_upload",
        description = "Sube uno o más archivos locales al área de borrador (draft) de Moodle devolviendo el itemid",
        annotations(destructive_hint = false)
    )]
    pub async fn moodle_upload(&self, params: Parameters<UploadParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        if params.0.paths.is_empty() {
            return self.ctx.tool_error(
                "invalid_arguments",
                "Debe especificarse al menos una ruta de archivo para subir",
                None,
                None,
                None,
                Some(&creds),
            );
        }

        let base_url = match Url::parse(&creds.base_url) {
            Ok(u) => u,
            Err(e) => {
                return self.ctx.tool_error(
                    "protocol",
                    format!("URL base del sitio inválida: {e}"),
                    None,
                    None,
                    None,
                    Some(&creds),
                );
            }
        };

        let path_bufs: Vec<PathBuf> = params.0.paths.iter().map(PathBuf::from).collect();

        let upload_result = match self
            .ctx
            .file_client
            .upload(&base_url, &creds.token, &path_bufs, params.0.itemid)
            .await
        {
            Ok(res) => res,
            Err(MoodleError::InvalidToken) => {
                return self.ctx.handle_invalid_token().await;
            }
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let output = UploadOutput {
            itemid: upload_result.itemid,
            files: upload_result.files,
        };

        self.ctx.ok_json(&output, "moodle_upload", Some(&creds))
    }
}
