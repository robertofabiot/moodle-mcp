use crate::moodle::client::{MoodleClient, DEFAULT_USER_AGENT};
use crate::moodle::envelope::decode_envelope;
use crate::moodle::error::{MoodleError, Result};
use crate::moodle::gate::{MoodleGate, Priority};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use url::Url;

/// Representa un archivo descargado exitosamente de Moodle.
#[derive(Debug, Clone)]
pub struct DownloadedFile {
    pub path: PathBuf,
    pub mime: String,
    pub size: u64,
    pub url_sin_token: String,
}

/// Resultado de una subida de archivos al área de borrador de Moodle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadResult {
    pub itemid: i64,
    pub files: Vec<String>,
}

/// Cliente HTTP especializado para descarga y subida de archivos de Moodle.
/// Utiliza rustls, tiempos de espera específicos para transferencias largas
/// y comparte el gate de concurrencia de MoodleClient sin reintentos automáticos.
#[derive(Clone)]
pub struct FileClient {
    client: reqwest::Client,
    gate: MoodleGate,
}

/// Sanea un nombre de archivo eliminando separadores (`/` y `\`), secuencias relativas (`..`),
/// y caracteres de control, limitando la longitud máxima a 200 bytes en un límite UTF-8 válido.
pub fn sanitize_filename(filename: &str) -> String {
    let no_dots = filename.replace("..", "");
    let cleaned: String = no_dots
        .chars()
        .filter(|&c| c != '/' && c != '\\' && !c.is_control())
        .collect();

    let trimmed = cleaned.trim().trim_matches('.');
    let base = if trimmed.is_empty() {
        "archivo"
    } else {
        trimmed
    };

    if base.len() > 200 {
        let mut end = 200;
        while !base.is_char_boundary(end) {
            end -= 1;
        }
        let truncated = base[..end].trim().trim_matches('.');
        if truncated.is_empty() {
            "archivo".to_string()
        } else {
            truncated.to_string()
        }
    } else {
        base.to_string()
    }
}

/// Normaliza un tipo MIME eliminando parámetros (`; charset=...`), convirtiendo a minúsculas
/// y unificando alias comunes (`image/jpg` -> `image/jpeg`).
pub fn normalize_mime(raw: &str) -> String {
    let base = raw
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if base == "image/jpg" {
        "image/jpeg".to_string()
    } else {
        base
    }
}

/// Resuelve el tipo MIME efectivo de un archivo a partir de la cabecera HTTP opcional
/// y la extensión de la ruta en disco.
///
/// Normaliza a minúsculas sin parámetros, mapea `image/jpg` a `image/jpeg` y recurre
/// a `mime_guess` cuando la cabecera está ausente, vacía o es genérica
/// (`application/octet-stream` o `binary/octet-stream`).
pub fn resolve_mime(path: &Path, header_mime: Option<&str>) -> String {
    if let Some(raw) = header_mime {
        let normalized = normalize_mime(raw);
        if !normalized.is_empty()
            && normalized != "application/octet-stream"
            && normalized != "binary/octet-stream"
        {
            return normalized;
        }
    }

    let guessed = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();
    normalize_mime(&guessed)
}

/// Autoriza una URL de archivo para Moodle:
/// 1. Verifica que el esquema, host y puerto coincidan exactamente con `base_url`.
/// 2. Verifica que la ruta comience con la ruta de `base_url`. Si no, retorna `MoodleError::InvalidArguments`.
/// 3. Reescribe `/pluginfile.php/` a `/webservice/pluginfile.php/` (conservando `/webservice/pluginfile.php/` y `/tokenpluginfile.php/`).
/// 4. Elimina cualquier query param `token` preexistente y añade `token=<token>`.
pub fn authorize_file_url(base_url: &Url, file_url_str: &str, token: &str) -> Result<Url> {
    let mut url = match Url::parse(file_url_str) {
        Ok(u) => u,
        Err(url::ParseError::RelativeUrlWithoutBase) => base_url
            .join(file_url_str)
            .map_err(|e| MoodleError::InvalidArguments(format!("URL de archivo inválida: {e}")))?,
        Err(e) => {
            return Err(MoodleError::InvalidArguments(format!(
                "URL de archivo inválida: {e}"
            )))
        }
    };

    if url.scheme() != base_url.scheme()
        || url.host_str() != base_url.host_str()
        || url.port_or_known_default() != base_url.port_or_known_default()
    {
        return Err(MoodleError::InvalidArguments(
            "URL externa: no se envía el token".into(),
        ));
    }

    let base_path = base_url.path().trim_end_matches('/');
    if !base_path.is_empty() {
        let url_path = url.path();
        let matches_subpath =
            url_path == base_path || url_path.starts_with(&format!("{base_path}/"));
        if !matches_subpath {
            return Err(MoodleError::InvalidArguments(
                "URL externa: no se envía el token".into(),
            ));
        }
    }

    let path = url.path().to_string();
    if !path.contains("/webservice/pluginfile.php/") && !path.contains("/tokenpluginfile.php/") {
        if let Some(pos) = path.find("/pluginfile.php/") {
            let mut new_path = String::with_capacity(path.len() + 11);
            new_path.push_str(&path[..pos]);
            new_path.push_str("/webservice/pluginfile.php/");
            new_path.push_str(&path[pos + "/pluginfile.php/".len()..]);
            url.set_path(&new_path);
        }
    }

    let mut query_pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != "token")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    query_pairs.push(("token".to_string(), token.to_string()));

    url.query_pairs_mut().clear().extend_pairs(query_pairs);
    Ok(url)
}

fn parse_content_disposition_filename(header: &str) -> Option<String> {
    if let Some(pos) = header.find("filename*=") {
        let val = &header[pos + "filename*=".len()..];
        let token = val.split(';').next()?.trim();
        let stripped = token.trim_matches('"');
        if let Some(rest) = stripped
            .strip_prefix("UTF-8''")
            .or_else(|| stripped.strip_prefix("utf-8''"))
        {
            if let Ok(decoded) = percent_encoding::percent_decode_str(rest).decode_utf8() {
                return Some(decoded.into_owned());
            }
        }
    }

    if let Some(pos) = header.find("filename=") {
        let val = &header[pos + "filename=".len()..];
        let token = val.split(';').next()?.trim();
        let unquoted = token.trim_matches('"');
        if !unquoted.is_empty() {
            return Some(unquoted.to_string());
        }
    }
    None
}

fn map_file_reqwest_error(err: reqwest::Error) -> MoodleError {
    use std::error::Error;
    let mut has_redirect_msg = err
        .to_string()
        .contains("Redirección a un dominio externo: no se sigue");
    let mut source = err.source();
    while let Some(s) = source {
        if s.to_string()
            .contains("Redirección a un dominio externo: no se sigue")
        {
            has_redirect_msg = true;
            break;
        }
        source = s.source();
    }
    if has_redirect_msg {
        return MoodleError::InvalidArguments(
            "Redirección a un dominio externo: no se sigue".into(),
        );
    }
    MoodleError::from(err.without_url())
}

impl FileClient {
    /// Inicializa un FileClient compartiendo el MoodleGate del MoodleClient original,
    /// con connect_timeout de 10s, read_timeout de 60s, sin timeout total y con DEFAULT_USER_AGENT.
    pub fn new(client: &MoodleClient) -> Self {
        Self::with_read_timeout(client, Duration::from_secs(60))
    }

    /// Inicializa un FileClient compartiendo el MoodleGate del MoodleClient original
    /// con un read_timeout personalizado.
    pub fn with_read_timeout(client: &MoodleClient, read_timeout: Duration) -> Self {
        let reqwest_client = reqwest::Client::builder()
            .use_rustls_tls()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(read_timeout)
            .user_agent(DEFAULT_USER_AGENT)
            .referer(false)
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 {
                    return attempt.error("Demasiadas redirecciones (máximo 5)");
                }
                let initial = &attempt.previous()[0];
                let target = attempt.url();
                if initial.scheme() != target.scheme()
                    || initial.host_str() != target.host_str()
                    || initial.port_or_known_default() != target.port_or_known_default()
                {
                    return attempt.error("Redirección a un dominio externo: no se sigue");
                }
                attempt.follow()
            }))
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("Fallo al construir reqwest::Client para FileClient: {e}");
                reqwest::Client::new()
            });

        Self {
            client: reqwest_client,
            gate: client.gate().clone(),
        }
    }

    /// Descarga un archivo por fragmentos escribiendo a `<destino>.part` y renombrando al final.
    /// Si la respuesta es application/json, decodifica el error con decode_envelope y retorna MoodleError.
    pub async fn download(
        &self,
        url: &Url,
        dest: Option<&Path>,
        download_dir: &Path,
    ) -> Result<DownloadedFile> {
        let _permit = self.gate.acquire(Priority::Interactive).await?;

        let mut response = self
            .client
            .get(url.as_str())
            .send()
            .await
            .map_err(map_file_reqwest_error)?;

        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if content_type.starts_with("application/json") {
            let body = response.text().await.map_err(map_file_reqwest_error)?;
            match decode_envelope::<serde_json::Value>(&body) {
                Ok(_) => {
                    return Err(MoodleError::Protocol(
                        "pluginfile devolvió una respuesta JSON no esperada".into(),
                    ))
                }
                Err(err) => return Err(err),
            }
        }

        if !status.is_success() {
            if status.as_u16() == 503 || status.is_server_error() {
                return Err(MoodleError::Maintenance);
            }
            return Err(MoodleError::Api {
                errorcode: status.to_string(),
                message: format!("HTTP error: {status}"),
                debuginfo: None,
            });
        }

        let raw_filename = response
            .headers()
            .get(reqwest::header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_disposition_filename)
            .unwrap_or_else(|| {
                url.path_segments()
                    .and_then(|mut segments| segments.rfind(|s| !s.is_empty()))
                    .map(|s| {
                        percent_encoding::percent_decode_str(s)
                            .decode_utf8_lossy()
                            .into_owned()
                    })
                    .unwrap_or_else(|| "archivo".to_string())
            });

        let filename = sanitize_filename(&raw_filename);

        let dest_path = match dest {
            None => download_dir.join("archivos").join(&filename),
            Some(p) => {
                let base = if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    download_dir.join(p)
                };
                let path_str = p.to_string_lossy();
                let is_dir_target =
                    path_str.ends_with('/') || path_str.ends_with('\\') || base.is_dir();
                if is_dir_target {
                    base.join(&filename)
                } else {
                    base
                }
            }
        };

        if let Some(parent) = dest_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(MoodleError::from)?;
        }

        let part_filename = match dest_path.file_name() {
            Some(name) => format!("{}.part", name.to_string_lossy()),
            None => format!("{filename}.part"),
        };
        let part_path = dest_path.with_file_name(part_filename);

        let mut file = match tokio::fs::File::create(&part_path).await {
            Ok(f) => f,
            Err(e) => return Err(MoodleError::from(e)),
        };
        let mut total_bytes = 0u64;

        let write_result: Result<()> = async {
            while let Some(chunk) = response.chunk().await.map_err(map_file_reqwest_error)? {
                file.write_all(&chunk).await.map_err(MoodleError::from)?;
                total_bytes += chunk.len() as u64;
            }
            file.flush().await.map_err(MoodleError::from)?;
            Ok(())
        }
        .await;

        drop(file);

        if let Err(e) = write_result {
            let _ = tokio::fs::remove_file(&part_path).await;
            return Err(e);
        }

        if let Err(e) = tokio::fs::rename(&part_path, &dest_path).await {
            let _ = tokio::fs::remove_file(&part_path).await;
            return Err(MoodleError::from(e));
        }

        let mut url_no_token = url.clone();
        let clean_pairs: Vec<(String, String)> = url_no_token
            .query_pairs()
            .filter(|(k, _)| k != "token")
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        if clean_pairs.is_empty() {
            url_no_token.set_query(None);
        } else {
            url_no_token
                .query_pairs_mut()
                .clear()
                .extend_pairs(clean_pairs);
        }

        let mime = resolve_mime(&dest_path, Some(&content_type));

        Ok(DownloadedFile {
            path: dest_path,
            mime,
            size: total_bytes,
            url_sin_token: url_no_token.to_string(),
        })
    }

    /// Sube una lista de archivos locales al área de borrador (draft) de Moodle usando POST /webservice/upload.php.
    /// Decodifica la respuesta sin decode_envelope (manejando array de objetos como éxito y objeto con error/errorcode como MoodleError::Api).
    pub async fn upload(
        &self,
        base_url: &Url,
        token: &str,
        paths: &[PathBuf],
        itemid: Option<i64>,
    ) -> Result<UploadResult> {
        let _permit = self.gate.acquire(Priority::Interactive).await?;

        if paths.is_empty() {
            return Err(MoodleError::InvalidArguments(
                "Debe especificarse al menos una ruta de archivo para subir".into(),
            ));
        }

        for path in paths {
            if !path.exists() || !path.is_file() {
                return Err(MoodleError::InvalidArguments(format!(
                    "El archivo no existe o no es válido: {}",
                    path.display()
                )));
            }
        }

        let base_trimmed = base_url.as_str().trim_end_matches('/');
        let upload_url = format!("{base_trimmed}/webservice/upload.php");

        let mut form = reqwest::multipart::Form::new()
            .text("token", token.to_string())
            .text("filearea", "draft")
            .text("itemid", itemid.unwrap_or(0).to_string())
            .text("filepath", "/");

        for (idx, path) in paths.iter().enumerate() {
            let field_name = format!("file_{}", idx + 1);
            let file_bytes = tokio::fs::read(path).await.map_err(MoodleError::from)?;
            let raw_filename = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("archivo");
            let sanitized_name = sanitize_filename(raw_filename);
            let mime = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();

            let part = reqwest::multipart::Part::bytes(file_bytes)
                .file_name(sanitized_name)
                .mime_str(&mime)
                .map_err(|e| MoodleError::InvalidArguments(format!("Tipo MIME inválido: {e}")))?;

            form = form.part(field_name, part);
        }

        let response = self
            .client
            .post(&upload_url)
            .multipart(form)
            .send()
            .await
            .map_err(map_file_reqwest_error)?;

        let status = response.status();
        let body_text = response.text().await.map_err(map_file_reqwest_error)?;

        let value: serde_json::Value = serde_json::from_str(&body_text).map_err(|e| {
            if status.as_u16() == 503 || status.is_server_error() {
                MoodleError::Maintenance
            } else {
                MoodleError::Protocol(format!("Respuesta no JSON de upload.php: {e}"))
            }
        })?;

        match value {
            serde_json::Value::Array(items) => {
                let first_itemid = items
                    .first()
                    .and_then(|item| item.get("itemid"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or_else(|| itemid.unwrap_or(0));

                let files = items
                    .iter()
                    .filter_map(|item| {
                        item.get("filename")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect();

                Ok(UploadResult {
                    itemid: first_itemid,
                    files,
                })
            }
            serde_json::Value::Object(map) => {
                if let Some(err_val) = map.get("error").or_else(|| map.get("errorcode")) {
                    let errorcode = map
                        .get("errorcode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("upload_error")
                        .to_string();
                    let message = map
                        .get("error")
                        .or_else(|| map.get("message"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_else(|| err_val.as_str().unwrap_or("Error en upload.php"))
                        .to_string();
                    let debuginfo = map
                        .get("debuginfo")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    Err(MoodleError::Api {
                        errorcode,
                        message,
                        debuginfo,
                    })
                } else {
                    Err(MoodleError::Protocol(
                        "Respuesta inesperada de upload.php".into(),
                    ))
                }
            }
            _ => Err(MoodleError::Protocol(
                "Respuesta no estructurada de upload.php".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename_removes_traversal_and_separators() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "etcpasswd");
        assert_eq!(
            sanitize_filename("carpeta/sub\\archivo.pdf"),
            "carpetasubarchivo.pdf"
        );
        assert_eq!(
            sanitize_filename("archivo\0con\ncontrol.txt"),
            "archivoconcontrol.txt"
        );
        assert_eq!(sanitize_filename("..."), "archivo");
        assert_eq!(sanitize_filename(""), "archivo");
        assert_eq!(sanitize_filename("  mi_tarea.docx.  "), "mi_tarea.docx");
    }

    #[test]
    fn test_sanitize_filename_truncates_at_boundary() {
        let long_name = "a".repeat(250);
        let sanitized = sanitize_filename(&long_name);
        assert_eq!(sanitized.len(), 200);

        let multi_byte = "🦀".repeat(60); // 4 bytes cada uno = 240 bytes
        let sanitized_mb = sanitize_filename(&multi_byte);
        assert!(sanitized_mb.len() <= 200);
        assert_eq!(sanitized_mb.len() % 4, 0);
    }

    #[test]
    fn test_authorize_file_url_rewrites_and_adds_token() {
        let base = Url::parse("https://uamvirtual.uam.edu.ni/grado").unwrap();

        // Reescribe pluginfile.php
        let u1 = authorize_file_url(
            &base,
            "https://uamvirtual.uam.edu.ni/grado/pluginfile.php/123/mod_resource/content/1/doc.pdf",
            "tok_abc",
        )
        .unwrap();
        assert_eq!(
            u1.path(),
            "/grado/webservice/pluginfile.php/123/mod_resource/content/1/doc.pdf"
        );
        assert_eq!(u1.query(), Some("token=tok_abc"));

        // No altera webservice/pluginfile.php ni tokenpluginfile.php
        let u2 = authorize_file_url(
            &base,
            "https://uamvirtual.uam.edu.ni/grado/webservice/pluginfile.php/123/mod/content/doc.pdf?forcedownload=1",
            "tok_abc",
        )
        .unwrap();
        assert_eq!(
            u2.path(),
            "/grado/webservice/pluginfile.php/123/mod/content/doc.pdf"
        );
        let q2: Vec<_> = u2.query_pairs().collect();
        assert!(q2.contains(&(
            std::borrow::Cow::Borrowed("forcedownload"),
            std::borrow::Cow::Borrowed("1")
        )));
        assert!(q2.contains(&(
            std::borrow::Cow::Borrowed("token"),
            std::borrow::Cow::Borrowed("tok_abc")
        )));

        // Reemplaza token existente
        let u3 = authorize_file_url(
            &base,
            "https://uamvirtual.uam.edu.ni/grado/tokenpluginfile.php/123/doc.pdf?token=viejo",
            "nuevo",
        )
        .unwrap();
        assert_eq!(u3.query(), Some("token=nuevo"));

        // Rechaza dominio externo
        let err_ext = authorize_file_url(&base, "https://drive.google.com/file/d/123", "tok_abc");
        assert!(matches!(err_ext, Err(MoodleError::InvalidArguments(_))));

        // Rechaza subruta diferente en el mismo host
        let err_path = authorize_file_url(
            &base,
            "https://uamvirtual.uam.edu.ni/postgrado/pluginfile.php/1/doc.pdf",
            "tok_abc",
        );
        assert!(matches!(err_path, Err(MoodleError::InvalidArguments(_))));
    }

    #[test]
    fn test_resolve_mime_normalizes_and_falls_back() {
        assert_eq!(
            resolve_mime(Path::new("guia.bin"), Some("TEXT/HTML; charset=UTF-8")),
            "text/html"
        );
        assert_eq!(
            resolve_mime(Path::new("foto.jpg"), Some("image/jpg; charset=binary")),
            "image/jpeg"
        );
        assert_eq!(
            resolve_mime(Path::new("clase.pdf"), Some("application/octet-stream")),
            "application/pdf"
        );
        assert_eq!(
            resolve_mime(Path::new("diapos.pptx"), Some("binary/octet-stream")),
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        );
        assert_eq!(
            resolve_mime(Path::new("documento.docx"), Some("   ")),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert_eq!(resolve_mime(Path::new("imagen.png"), None), "image/png");
        assert_eq!(resolve_mime(Path::new("foto.jpg"), None), "image/jpeg");
        assert_eq!(
            resolve_mime(Path::new("archivo_sin_extension"), None),
            "application/octet-stream"
        );
    }
}
