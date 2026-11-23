use crate::content::html_to_markdown;
use crate::moodle::client::MoodleClient;
use crate::moodle::error::Result;
use crate::moodle::files::sanitize_filename;
use crate::moodle::files::{authorize_file_url, FileClient};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use url::Url;

/// Archivo que falló durante la sincronización.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedSyncItem {
    pub path: String,
    pub error: String,
}

/// Enlace externo detectado (no se descarga).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalLinkItem {
    pub section: String,
    pub module: String,
    pub url: String,
}

/// Resultado de una operación de sincronización de curso.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub dir: String,
    pub downloaded: usize,
    pub skipped: usize,
    pub failed: Vec<FailedSyncItem>,
    pub external_links: Vec<ExternalLinkItem>,
}

/// Manifiesto incremental que registra el estado de los archivos sincronizados.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncManifest {
    pub files: HashMap<String, ManifestEntry>,
}

/// Entrada del manifiesto para un archivo sincronizado.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub timemodified: i64,
    pub filesize: i64,
    pub path: String,
}

const MANIFEST_FILENAME: &str = ".moodle-sync.json";
const ENLACES_FILENAME: &str = "enlaces.md";

/// Carga el manifiesto desde disco o devuelve uno vacío si no existe.
async fn load_manifest(dir: &Path) -> SyncManifest {
    let manifest_path = dir.join(MANIFEST_FILENAME);
    match tokio::fs::read(&manifest_path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => SyncManifest::default(),
    }
}

/// Guarda el manifiesto actualizado en disco.
async fn save_manifest(dir: &Path, manifest: &SyncManifest) -> std::io::Result<()> {
    let manifest_path = dir.join(MANIFEST_FILENAME);
    let json = serde_json::to_vec_pretty(manifest).unwrap_or_default();
    tokio::fs::write(&manifest_path, json).await
}

/// Determina si una URL de archivo apunta a un dominio externo al `base_url`.
fn is_external_url(base_url: &Url, file_url_str: &str) -> bool {
    match Url::parse(file_url_str) {
        Ok(url) => {
            url.host_str() != base_url.host_str()
                || url.scheme() != base_url.scheme()
                || url.port_or_known_default() != base_url.port_or_known_default()
        }
        Err(_) => false,
    }
}

/// Ejecuta la réplica local incremental de un curso de Moodle.
///
/// Descarga todos los archivos del árbol del curso de forma secuencial,
/// omitiendo los que no hayan cambiado según el manifiesto.
/// Los módulos `page` guardan su contenido como `index.html` e `index.md`.
/// Los enlaces externos se listan en `enlaces.md` en la raíz.
pub async fn sync_course(
    client: &MoodleClient,
    file_client: &FileClient,
    token: &str,
    courseid: i64,
    course_name_hint: Option<&str>,
    dest: Option<PathBuf>,
    download_dir: &Path,
) -> Result<SyncResult> {
    let sections = client.course_tree(token, courseid).await?;

    // Determinar directorio raíz del curso
    let course_dir = match dest {
        Some(d) => d,
        None => {
            let folder_name = course_name_hint
                .map(sanitize_filename)
                .unwrap_or_else(|| format!("curso-{courseid}"));
            download_dir.join(&folder_name)
        }
    };

    tokio::fs::create_dir_all(&course_dir)
        .await
        .map_err(crate::moodle::error::MoodleError::from)?;

    let mut manifest = load_manifest(&course_dir).await;

    let mut downloaded = 0usize;
    let mut skipped = 0usize;
    let mut failed: Vec<FailedSyncItem> = Vec::new();
    let mut external_links: Vec<ExternalLinkItem> = Vec::new();

    // Parsear base_url para validación de dominios
    let base_url = Url::parse(client.base_url()).map_err(|e| {
        crate::moodle::error::MoodleError::InvalidArguments(format!("URL base inválida: {e}"))
    })?;

    // Colección de líneas para el archivo de enlaces externos
    let mut enlaces_lines: Vec<String> =
        vec!["# Enlaces externos del curso".to_string(), String::new()];

    for section in &sections {
        let section_num = section.section;
        let section_name_safe = sanitize_filename(&section.name);
        let section_folder_name = format!("{section_num:02} - {section_name_safe}");

        for module in &section.modules {
            // Omitir módulos no visibles
            if module.uservisible == Some(false) {
                continue;
            }

            let module_name_safe = sanitize_filename(&module.name);
            let module_dir = course_dir
                .join(&section_folder_name)
                .join(&module_name_safe);

            for content in &module.contents {
                // Detectar tipo URL o dominio externo
                if content.r#type == "url" {
                    if let Some(ref furl) = content.fileurl {
                        if is_external_url(&base_url, furl) {
                            let item = ExternalLinkItem {
                                section: section.name.clone(),
                                module: module.name.clone(),
                                url: furl.clone(),
                            };
                            enlaces_lines.push(format!(
                                "- **{}** / **{}**: <{}>",
                                section.name, module.name, furl
                            ));
                            external_links.push(item);
                            continue;
                        }
                    }
                    // URL interna también la tratamos como enlace externo (tipo=url)
                    if let Some(ref furl) = content.fileurl {
                        let item = ExternalLinkItem {
                            section: section.name.clone(),
                            module: module.name.clone(),
                            url: furl.clone(),
                        };
                        enlaces_lines.push(format!(
                            "- **{}** / **{}**: <{}>",
                            section.name, module.name, furl
                        ));
                        external_links.push(item);
                    }
                    continue;
                }

                let file_url_str = match &content.fileurl {
                    Some(u) => u.clone(),
                    None => continue,
                };

                // Detectar enlaces externos en tipo "file" u otros
                if is_external_url(&base_url, &file_url_str) {
                    let item = ExternalLinkItem {
                        section: section.name.clone(),
                        module: module.name.clone(),
                        url: file_url_str.clone(),
                    };
                    enlaces_lines.push(format!(
                        "- **{}** / **{}**: <{}>",
                        section.name, module.name, file_url_str
                    ));
                    external_links.push(item);
                    continue;
                }

                // Autorizar URL con token
                let authorized_url = match authorize_file_url(&base_url, &file_url_str, token) {
                    Ok(u) => u,
                    Err(e) => {
                        failed.push(FailedSyncItem {
                            path: file_url_str.clone(),
                            error: e.to_string(),
                        });
                        continue;
                    }
                };

                // Construir ruta de destino: <section>/<module>/<filepath>/<filename>
                let filepath_safe = content
                    .filepath
                    .as_deref()
                    .unwrap_or("/")
                    .trim_matches('/')
                    .split('/')
                    .filter(|s| !s.is_empty())
                    .map(sanitize_filename)
                    .collect::<Vec<_>>()
                    .join("/");

                let filename_safe = sanitize_filename(&content.filename);

                let dest_path = if filepath_safe.is_empty() {
                    module_dir.join(&filename_safe)
                } else {
                    module_dir.join(&filepath_safe).join(&filename_safe)
                };

                // URL limpia (sin token) para el manifiesto
                let url_key = {
                    let mut clean =
                        Url::parse(&file_url_str).unwrap_or_else(|_| authorized_url.clone());
                    let clean_pairs: Vec<(String, String)> = clean
                        .query_pairs()
                        .filter(|(k, _)| k != "token")
                        .map(|(k, v)| (k.into_owned(), v.into_owned()))
                        .collect();
                    if clean_pairs.is_empty() {
                        clean.set_query(None);
                    } else {
                        clean.query_pairs_mut().clear().extend_pairs(&clean_pairs);
                    }
                    clean.to_string()
                };

                // Comprobar si ya está sincronizado (mismos timemodified y filesize)
                if let Some(entry) = manifest.files.get(&url_key) {
                    let same_time = content
                        .timemodified
                        .map(|t| t == entry.timemodified)
                        .unwrap_or(false);
                    let same_size = content
                        .filesize
                        .map(|s| s == entry.filesize)
                        .unwrap_or(false);
                    if same_time && same_size && dest_path.exists() {
                        skipped += 1;
                        continue;
                    }
                }

                // Descargar archivo (secuencial, uno a la vez)
                match file_client
                    .download(&authorized_url, Some(dest_path.as_path()), download_dir)
                    .await
                {
                    Ok(df) => {
                        // Guardar en manifiesto
                        manifest.files.insert(
                            url_key,
                            ManifestEntry {
                                timemodified: content.timemodified.unwrap_or(0),
                                filesize: content.filesize.unwrap_or(df.size as i64),
                                path: df.path.to_string_lossy().into_owned(),
                            },
                        );

                        // Para módulos page con index.html: generar index.md
                        if module.modname == "page" && content.filename == "index.html" {
                            if let Ok(bytes) = tokio::fs::read(&df.path).await {
                                if let Ok(html) = String::from_utf8(bytes) {
                                    let md = html_to_markdown(&html);
                                    let md_path = df.path.with_file_name("index.md");
                                    let _ = tokio::fs::write(&md_path, md.as_bytes()).await;
                                }
                            }
                        }

                        downloaded += 1;
                    }
                    Err(e) => {
                        failed.push(FailedSyncItem {
                            path: dest_path.to_string_lossy().into_owned(),
                            error: e.to_string(),
                        });
                    }
                }
            }
        }
    }

    // Guardar manifiesto actualizado
    let _ = save_manifest(&course_dir, &manifest).await;

    // Escribir enlaces.md si hay enlaces externos
    if !external_links.is_empty() {
        let content = enlaces_lines.join("\n");
        let enlaces_path = course_dir.join(ENLACES_FILENAME);
        let _ = tokio::fs::write(&enlaces_path, content.as_bytes()).await;
    }

    Ok(SyncResult {
        dir: course_dir.to_string_lossy().into_owned(),
        downloaded,
        skipped,
        failed,
        external_links,
    })
}
