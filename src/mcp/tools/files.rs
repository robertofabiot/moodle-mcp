use crate::content::extract::{extract_text, is_supported_document_mime, is_supported_image_mime};
use crate::credentials::Credentials;
use crate::mcp::server::MoodleMcp;
use crate::moodle::error::MoodleError;
use crate::moodle::files::{authorize_file_url, normalize_mime, resolve_mime, DownloadedFile};
use base64::Engine as _;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;
use url::Url;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DownloadParams {
    /// URL del archivo en Moodle (ej. pluginfile.php)
    pub url: String,
    /// Ruta de destino opcional (relativa a la carpeta de descargas o absoluta)
    pub dest: Option<String>,
    /// Si es true, visualiza o extrae el contenido del archivo descargado en la respuesta
    #[serde(default)]
    pub inline: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ViewParams {
    /// Ruta local de un archivo (absoluta, relativa al directorio de trabajo o a la carpeta de descargas)
    pub path: Option<String>,
    /// URL del archivo en Moodle (ej. pluginfile.php) para descargarlo y visualizarlo
    pub url: Option<String>,
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

#[derive(Debug, Serialize)]
struct ViewImageOutput {
    path: String,
    mime: String,
    size: u64,
    inline_image: bool,
}

#[derive(Debug, Serialize)]
struct ViewDocumentOutput<'a> {
    path: &'a str,
    mime: &'a str,
    size: u64,
    content_markdown: &'a str,
}

#[derive(Debug, Serialize)]
struct ViewTruncatedOutput {
    path: String,
    mime: String,
    size: u64,
    truncated: bool,
    text_saved_to: String,
    preview: String,
}

#[derive(Debug, Serialize)]
struct ViewNoteOutput {
    path: String,
    mime: String,
    size: u64,
    note: &'static str,
}

enum ViewSource {
    Path(String),
    Url(String),
}

fn saved_text_path(original: &Path) -> PathBuf {
    let mut os = original.as_os_str().to_os_string();
    os.push(".txt");
    PathBuf::from(os)
}

fn floor_char_boundary(s: &str, max_bytes: usize) -> usize {
    let mut idx = max_bytes.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn build_truncated_doc_json(
    path: String,
    mime: String,
    size: u64,
    text_saved_to: String,
    scrubbed_text: &str,
    max_response_bytes: usize,
) -> Result<String, serde_json::Error> {
    let mut out = ViewTruncatedOutput {
        path,
        mime,
        size,
        truncated: true,
        text_saved_to,
        preview: String::new(),
    };

    let overhead = serde_json::to_string(&out)?.len();
    if max_response_bytes > overhead {
        let mut cut = floor_char_boundary(scrubbed_text, max_response_bytes - overhead);
        loop {
            out.preview = scrubbed_text[..cut].to_string();
            let serialized = serde_json::to_string(&out)?;
            if serialized.len() <= max_response_bytes || cut == 0 {
                return Ok(serialized);
            }
            let excess = serialized.len() - max_response_bytes;
            let next_target = cut.saturating_sub(excess.max(1));
            cut = floor_char_boundary(scrubbed_text, next_target);
        }
    } else {
        let cut = floor_char_boundary(scrubbed_text, max_response_bytes);
        out.preview = scrubbed_text[..cut].to_string();
        serde_json::to_string(&out)
    }
}

impl MoodleMcp {
    async fn download_authorized(
        &self,
        url_str: &str,
        dest: Option<&Path>,
        creds: &Credentials,
    ) -> Result<DownloadedFile, CallToolResult> {
        let base_url = Url::parse(&creds.base_url).map_err(|e| {
            self.ctx.tool_error(
                "protocol",
                format!("URL base del sitio inválida: {e}"),
                None,
                None,
                None,
                Some(creds),
            )
        })?;

        let authorized_url = authorize_file_url(&base_url, url_str, &creds.token)
            .map_err(|e| self.ctx.tool_error_from_moodle(&e, Some(creds)))?;

        match self
            .ctx
            .file_client
            .download(&authorized_url, dest, &self.ctx.config.download_dir)
            .await
        {
            Ok(df) => Ok(df),
            Err(MoodleError::InvalidToken) => Err(self.ctx.handle_invalid_token().await),
            Err(e) => Err(self.ctx.tool_error_from_moodle(&e, Some(creds))),
        }
    }

    async fn view_file_inline(
        &self,
        path: &Path,
        mime: &str,
        size: u64,
        tool_name: &str,
        creds: Option<&Credentials>,
    ) -> CallToolResult {
        let norm_mime = normalize_mime(mime);
        let path_display = path.to_string_lossy().into_owned();

        if is_supported_image_mime(&norm_mime) {
            if size > self.ctx.config.max_inline_image_bytes as u64 {
                let out = ViewNoteOutput {
                    path: path_display,
                    mime: norm_mime,
                    size,
                    note: "imagen supera el límite para mostrarse en línea",
                };
                return self.ctx.ok_json(&out, tool_name, creds);
            }

            let file = match tokio::fs::File::open(path).await {
                Ok(file) => file,
                Err(e) => {
                    return self
                        .ctx
                        .tool_error_from_moodle(&MoodleError::from(e), creds);
                }
            };
            let mut bytes = Vec::new();
            let read_limit = (self.ctx.config.max_inline_image_bytes as u64).saturating_add(1);
            if let Err(e) = file.take(read_limit).read_to_end(&mut bytes).await {
                return self
                    .ctx
                    .tool_error_from_moodle(&MoodleError::from(e), creds);
            }

            if bytes.len() > self.ctx.config.max_inline_image_bytes {
                let out = ViewNoteOutput {
                    path: path_display,
                    mime: norm_mime,
                    size: bytes.len() as u64,
                    note: "imagen supera el límite para mostrarse en línea",
                };
                return self.ctx.ok_json(&out, tool_name, creds);
            }

            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let out = ViewImageOutput {
                path: path_display,
                mime: norm_mime.clone(),
                size: bytes.len() as u64,
                inline_image: true,
            };
            return self
                .ctx
                .ok_json_with_image(&out, encoded, &norm_mime, creds);
        }

        if is_supported_document_mime(&norm_mime) {
            let extracted = match extract_text(path, &norm_mime).await {
                Ok(opt) => opt,
                Err(e) => return self.ctx.tool_error_from_moodle(&e, creds),
            };

            let Some(text) = extracted else {
                let out = ViewNoteOutput {
                    path: path_display,
                    mime: norm_mime,
                    size,
                    note: "sin texto extraíble (posible escaneo)",
                };
                return self.ctx.ok_json(&out, tool_name, creds);
            };

            let tok = creds.map(|c| c.token.as_str());
            let priv_tok = creds.and_then(|c| c.private_token.as_deref());
            let scrubbed_text = crate::mcp::output::scrub(&text, tok, priv_tok);
            let path_scrubbed = crate::mcp::output::scrub(&path_display, tok, priv_tok);

            if scrubbed_text.len() <= self.ctx.config.max_response_bytes {
                let out = ViewDocumentOutput {
                    path: &path_scrubbed,
                    mime: &norm_mime,
                    size,
                    content_markdown: &scrubbed_text,
                };
                let serialized = match serde_json::to_string(&out) {
                    Ok(serialized) => serialized,
                    Err(e) => {
                        let error =
                            MoodleError::Protocol(format!("Error al serializar respuesta: {e}"));
                        return self.ctx.tool_error_from_moodle(&error, creds);
                    }
                };
                if serialized.len() <= self.ctx.config.max_response_bytes {
                    return crate::mcp::output::tool_success(&serialized, tok, priv_tok);
                }
            }

            let text_path = saved_text_path(path);
            if let Err(e) = tokio::fs::write(&text_path, scrubbed_text.as_bytes()).await {
                return self.ctx.tool_error(
                    "io",
                    format!(
                        "Error al guardar texto completo en {}: {e}",
                        text_path.display()
                    ),
                    None,
                    None,
                    None,
                    creds,
                );
            }

            let text_saved_to =
                crate::mcp::output::scrub(&text_path.to_string_lossy(), tok, priv_tok);

            let serialized = match build_truncated_doc_json(
                path_scrubbed,
                norm_mime,
                size,
                text_saved_to,
                &scrubbed_text,
                self.ctx.config.max_response_bytes,
            ) {
                Ok(s) => s,
                Err(e) => {
                    return self.ctx.tool_error(
                        "protocol",
                        format!("Error al serializar respuesta truncada: {e}"),
                        None,
                        None,
                        None,
                        creds,
                    );
                }
            };

            return crate::mcp::output::tool_success(&serialized, tok, priv_tok);
        }

        let out = ViewNoteOutput {
            path: path_display,
            mime: norm_mime,
            size,
            note: "formato sin visor",
        };
        self.ctx.ok_json(&out, tool_name, creds)
    }
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

        let dest_path = params.0.dest.as_deref().map(Path::new);
        let downloaded = match self
            .download_authorized(&params.0.url, dest_path, &creds)
            .await
        {
            Ok(df) => df,
            Err(err) => return err,
        };

        if params.0.inline {
            return self
                .view_file_inline(
                    &downloaded.path,
                    &downloaded.mime,
                    downloaded.size,
                    "moodle_download",
                    Some(&creds),
                )
                .await;
        }

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

    /// Visualiza un archivo local o remoto de Moodle extrayendo su texto/Markdown (PDF, DOCX, PPTX, HTML, texto)
    /// o incrustando imágenes raster (PNG, JPEG, GIF, WebP) en línea dentro de los límites configurados.
    #[rmcp::tool(
        name = "moodle_view",
        description = "Visualiza un archivo local (path) o de Moodle (url), extrayendo texto de PDF, DOCX, PPTX, HTML y texto o devolviendo imágenes en línea",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn moodle_view(&self, params: Parameters<ViewParams>) -> CallToolResult {
        let source = match (params.0.path, params.0.url) {
            (Some(p), None) => {
                let trimmed = p.trim();
                if trimmed.is_empty() {
                    return self.ctx.tool_error(
                        "invalid_arguments",
                        "El parámetro 'path' no puede estar vacío",
                        None,
                        None,
                        None,
                        None,
                    );
                }
                ViewSource::Path(trimmed.to_string())
            }
            (None, Some(u)) => {
                let trimmed = u.trim();
                if trimmed.is_empty() {
                    return self.ctx.tool_error(
                        "invalid_arguments",
                        "El parámetro 'url' no puede estar vacío",
                        None,
                        None,
                        None,
                        None,
                    );
                }
                ViewSource::Url(trimmed.to_string())
            }
            (Some(_), Some(_)) => {
                return self.ctx.tool_error(
                    "invalid_arguments",
                    "Debes especificar exactamente uno de 'path' o 'url', no ambos",
                    None,
                    None,
                    None,
                    None,
                );
            }
            (None, None) => {
                return self.ctx.tool_error(
                    "invalid_arguments",
                    "Debes especificar exactamente uno de 'path' o 'url'",
                    None,
                    None,
                    None,
                    None,
                );
            }
        };

        match source {
            ViewSource::Url(url_str) => {
                let creds = match self.ctx.session().await {
                    Ok(c) => c,
                    Err(err) => return err,
                };
                let downloaded = match self.download_authorized(&url_str, None, &creds).await {
                    Ok(df) => df,
                    Err(err) => return err,
                };
                self.view_file_inline(
                    &downloaded.path,
                    &downloaded.mime,
                    downloaded.size,
                    "moodle_view",
                    Some(&creds),
                )
                .await
            }
            ViewSource::Path(path_str) => {
                let creds = self.ctx.session().await.ok();
                let raw_path = Path::new(&path_str);
                let resolved = if raw_path.is_absolute()
                    || tokio::fs::metadata(raw_path)
                        .await
                        .is_ok_and(|m| m.is_file())
                {
                    raw_path.to_path_buf()
                } else {
                    self.ctx.config.download_dir.join(raw_path)
                };

                let meta = match tokio::fs::metadata(&resolved).await {
                    Ok(m) if m.is_file() => m,
                    _ => {
                        return self.ctx.tool_error(
                            "invalid_arguments",
                            format!(
                                "El archivo no existe o no es un archivo regular: {}",
                                resolved.display()
                            ),
                            None,
                            None,
                            None,
                            creds.as_deref(),
                        );
                    }
                };

                let size = meta.len();
                let mime = resolve_mime(&resolved, None);
                self.view_file_inline(&resolved, &mime, size, "moodle_view", creds.as_deref())
                    .await
            }
        }
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
