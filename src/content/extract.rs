use crate::content::html_to_markdown;
use crate::moodle::error::{MoodleError, Result};
use crate::moodle::files::normalize_mime;
use quick_xml::escape::resolve_xml_entity;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::{BufReader, Read};
use std::path::Path;
use zip::result::ZipError;
use zip::{read::ZipFile, ZipArchive};

/// Límite máximo de bytes descomprimidos por archivo XML interno en OOXML (64 MiB)
/// para evitar agotamiento de memoria ante archivos ZIP maliciosos.
const MAX_OOXML_XML_BYTES: u64 = 64 * 1024 * 1024;

/// Límite máximo acumulado de bytes descomprimidos entre todas las diapositivas de un PPTX (128 MiB).
const MAX_OOXML_TOTAL_BYTES: u64 = 128 * 1024 * 1024;

/// Tipo MIME estándar para documentos PDF.
pub const MIME_PDF: &str = "application/pdf";

/// Tipo MIME estándar para documentos Word OOXML (`.docx`).
pub const MIME_DOCX: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

/// Tipo MIME estándar para presentaciones PowerPoint OOXML (`.pptx`).
pub const MIME_PPTX: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation";

/// Indica si el tipo MIME corresponde a una imagen raster soportada para visualización en línea.
pub fn is_supported_image_mime(mime: &str) -> bool {
    let norm = normalize_mime(mime);
    matches!(
        norm.as_str(),
        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
    )
}

/// Indica si el tipo MIME corresponde a un documento o archivo textual soportado por `extract_text`.
pub fn is_supported_document_mime(mime: &str) -> bool {
    let norm = normalize_mime(mime);
    norm == MIME_PDF
        || norm == MIME_DOCX
        || norm == MIME_PPTX
        || norm == "text/html"
        || norm.starts_with("text/")
        || norm == "application/json"
        || norm == "application/xml"
}

/// Extrae el contenido textual o Markdown de un archivo local según su tipo MIME.
///
/// Devuelve `Ok(None)` cuando el formato no tiene extractor o cuando un documento soportado
/// (por ejemplo, un PDF escaneado o una presentación sin texto) no contiene texto extraíble.
pub async fn extract_text(path: &Path, mime: &str) -> Result<Option<String>> {
    let norm = normalize_mime(mime);

    match norm.as_str() {
        MIME_PDF => extract_pdf(path).await,
        MIME_DOCX => {
            let path_buf = path.to_path_buf();
            tokio::task::spawn_blocking(move || extract_docx_sync(&path_buf))
                .await
                .map_err(|e| {
                    MoodleError::Protocol(format!("Error interno al procesar DOCX: {e}"))
                })?
        }
        MIME_PPTX => {
            let path_buf = path.to_path_buf();
            tokio::task::spawn_blocking(move || extract_pptx_sync(&path_buf))
                .await
                .map_err(|e| {
                    MoodleError::Protocol(format!("Error interno al procesar PPTX: {e}"))
                })?
        }
        "text/html" => {
            let raw = read_text_lossy(path).await?;
            let md = html_to_markdown(&raw);
            if md.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(md))
            }
        }
        m if m.starts_with("text/") || m == "application/json" || m == "application/xml" => {
            let raw = read_text_lossy(path).await?;
            if raw.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(raw))
            }
        }
        _ => Ok(None),
    }
}

async fn read_text_lossy(path: &Path) -> Result<String> {
    let bytes = tokio::fs::read(path).await.map_err(MoodleError::from)?;
    let text = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(err) => String::from_utf8_lossy(err.as_bytes()).into_owned(),
    };
    Ok(text)
}

async fn extract_pdf(path: &Path) -> Result<Option<String>> {
    let meta = tokio::fs::metadata(path).await.map_err(MoodleError::from)?;
    if !meta.is_file() {
        return Err(MoodleError::Io(format!(
            "La ruta no corresponde a un archivo regular: {}",
            path.display()
        )));
    }
    let _ = tokio::fs::File::open(path)
        .await
        .map_err(MoodleError::from)?;

    let path_buf = path.to_path_buf();
    match tokio::task::spawn_blocking(move || pdf_extract::extract_text(&path_buf)).await {
        Ok(Ok(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        Ok(Err(err)) => {
            tracing::debug!("No se pudo extraer texto del PDF {}: {err}", path.display());
            Ok(None)
        }
        Err(join_err) => {
            tracing::warn!(
                "Fallo en tarea bloqueante al extraer PDF {}: {join_err}",
                path.display()
            );
            Ok(None)
        }
    }
}

fn map_zip_error(err: ZipError) -> MoodleError {
    match err {
        ZipError::Io(io_err) => MoodleError::Io(io_err.to_string()),
        other => MoodleError::Protocol(format!("Archivo OOXML o ZIP inválido: {other}")),
    }
}

fn extract_ooxml_entry<R: Read>(
    entry: ZipFile<'_, R>,
    remaining_bytes: &mut u64,
) -> Result<Option<String>> {
    let limit = MAX_OOXML_XML_BYTES.min(*remaining_bytes);
    let limit_error =
        || MoodleError::Protocol("El XML del documento supera el límite de descompresión".into());
    if entry.size() > limit {
        return Err(limit_error());
    }

    let mut bounded = entry.take(limit + 1);
    let text = extract_xml_text(BufReader::new(&mut bounded));
    let bytes_read = limit + 1 - bounded.limit();
    if bytes_read > limit {
        return Err(limit_error());
    }
    *remaining_bytes -= bytes_read;
    text
}

fn extract_docx_sync(path: &Path) -> Result<Option<String>> {
    let file = std::fs::File::open(path).map_err(MoodleError::from)?;
    let mut archive = ZipArchive::new(file).map_err(map_zip_error)?;
    let entry = archive
        .by_name("word/document.xml")
        .map_err(map_zip_error)?;
    let mut remaining_bytes = MAX_OOXML_XML_BYTES;
    extract_ooxml_entry(entry, &mut remaining_bytes)
}

fn parse_pptx_slide_number(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("ppt/slides/slide")?;
    let num_str = rest.strip_suffix(".xml")?;
    if num_str.is_empty() || !num_str.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    num_str.parse::<u32>().ok()
}

fn extract_pptx_sync(path: &Path) -> Result<Option<String>> {
    let file = std::fs::File::open(path).map_err(MoodleError::from)?;
    let mut archive = ZipArchive::new(file).map_err(map_zip_error)?;

    let mut slides: Vec<(u32, String)> = archive
        .file_names()
        .filter_map(|res| res.ok())
        .filter_map(|name| parse_pptx_slide_number(&name).map(|n| (n, name.into_owned())))
        .collect();
    slides.sort_by_key(|(n, _)| *n);

    let mut sections = Vec::new();
    let mut remaining_bytes = MAX_OOXML_TOTAL_BYTES;
    for (num, entry_name) in slides {
        let entry = archive.by_name(&entry_name).map_err(map_zip_error)?;
        if let Some(slide_text) = extract_ooxml_entry(entry, &mut remaining_bytes)? {
            sections.push(format!("## Diapositiva {num}\n{slide_text}"));
        }
    }

    if sections.is_empty() {
        Ok(None)
    } else {
        Ok(Some(sections.join("\n\n")))
    }
}

fn extract_xml_text<R: std::io::BufRead>(reader: R) -> Result<Option<String>> {
    let mut xml = Reader::from_reader(reader);
    // Preservar espacios significativos dentro de nodos <w:t xml:space="preserve"> / <a:t>
    xml.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut out = String::new();
    let mut in_text = false;
    let mut depth = 0;

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                match e.local_name().as_ref() {
                    "t" => in_text = true,
                    "br" | "cr" => out.push('\n'),
                    "tab" => out.push('\t'),
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                "p" | "br" | "cr" => out.push('\n'),
                "tab" => out.push('\t'),
                _ => {}
            },
            Ok(Event::End(e)) => {
                depth -= 1;
                match e.local_name().as_ref() {
                    "t" => in_text = false,
                    "p" => out.push('\n'),
                    _ => {}
                }
            }
            Ok(Event::Text(e)) if in_text => {
                out.push_str(&e.xml10_content());
            }
            Ok(Event::CData(e)) if in_text => {
                out.push_str(&e.xml10_content());
            }
            Ok(Event::GeneralRef(e)) if in_text => match e.resolve_char_ref() {
                Ok(Some(ch)) => out.push(ch),
                Ok(None) => {
                    if let Some(resolved) = resolve_xml_entity(e.as_ref()) {
                        out.push_str(resolved);
                    } else {
                        out.push('&');
                        out.push_str(e.as_ref());
                        out.push(';');
                    }
                }
                Err(err) => {
                    return Err(MoodleError::Protocol(format!(
                        "Referencia de carácter XML inválida: {err}"
                    )));
                }
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(err) => {
                return Err(MoodleError::Protocol(format!(
                    "XML inválido al extraer texto del documento: {err}"
                )));
            }
        }
        buf.clear();
    }

    if depth != 0 {
        return Err(MoodleError::Protocol(
            "XML incompleto al extraer texto del documento".into(),
        ));
    }

    if out.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(out.trim_matches(['\n', '\r']).to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn build_minimal_pdf(text: Option<&str>) -> Vec<u8> {
        let stream_content = match text {
            Some(t) => format!("BT /FontA 12 Tf 72 720 Td ({t}) Tj ET"),
            None => "0 0 100 100 re f".to_string(),
        };
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /FontA 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{}\nendstream", stream_content.len(), stream_content),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];

        let mut pdf = Vec::from(&b"%PDF-1.4\n"[..]);
        let mut offsets = vec![0usize];
        for (idx, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            let obj = format!("{} 0 obj\n{}\nendobj\n", idx + 1, body);
            pdf.extend_from_slice(obj.as_bytes());
        }

        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for off in offsets.iter().skip(1) {
            pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                objects.len() + 1,
                xref_offset
            )
            .as_bytes(),
        );
        pdf
    }

    fn build_zip(entries: &[(&str, &str)]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let opts = SimpleFileOptions::default();
        for (name, body) in entries {
            writer.start_file(*name, opts).unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn test_ooxml_entries_share_the_decompression_budget() {
        let first = "<p><t>primera</t></p>";
        let second = "<p><t>segunda</t></p>";
        let data = build_zip(&[("primera.xml", first), ("segunda.xml", second)]);
        let mut zip = ZipArchive::new(std::io::Cursor::new(data)).unwrap();
        let mut remaining_bytes = (first.len() + second.len() - 1) as u64;

        let entry = zip.by_name("primera.xml").unwrap();
        assert_eq!(
            extract_ooxml_entry(entry, &mut remaining_bytes).unwrap(),
            Some("primera".into())
        );
        let entry = zip.by_name("segunda.xml").unwrap();
        assert!(matches!(
            extract_ooxml_entry(entry, &mut remaining_bytes),
            Err(MoodleError::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn test_extract_pdf_with_text_and_scanned_empty() {
        let dir = tempdir().unwrap();

        let pdf_text_path = dir.path().join("apuntes.pdf");
        std::fs::write(&pdf_text_path, build_minimal_pdf(Some("Hola Moodle PDF"))).unwrap();
        let extracted = extract_text(&pdf_text_path, MIME_PDF).await.unwrap();
        assert!(extracted
            .as_deref()
            .is_some_and(|s| s.contains("Hola Moodle PDF")));

        let pdf_empty_path = dir.path().join("escaneo.pdf");
        std::fs::write(&pdf_empty_path, build_minimal_pdf(None)).unwrap();
        let extracted_empty = extract_text(&pdf_empty_path, MIME_PDF).await.unwrap();
        assert_eq!(extracted_empty, None);

        let pdf_corrupt_path = dir.path().join("corrupto.pdf");
        std::fs::write(&pdf_corrupt_path, b"%PDF-corrupto-sin-estructura").unwrap();
        let extracted_corrupt = extract_text(&pdf_corrupt_path, MIME_PDF).await.unwrap();
        assert_eq!(extracted_corrupt, None);

        let missing_path = dir.path().join("no_existe.pdf");
        let missing_err = extract_text(&missing_path, MIME_PDF).await.unwrap_err();
        assert!(matches!(missing_err, MoodleError::Io(_)));
    }

    #[tokio::test]
    async fn test_extract_docx_preserves_spaces_entities_tabs_and_breaks() {
        let dir = tempdir().unwrap();
        let docx_path = dir.path().join("guia.docx");
        let doc_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p>
      <w:r><w:t xml:space="preserve">Hola </w:t></w:r>
      <w:r><w:t>mundo &amp; &#x2605;</w:t></w:r>
    </w:p>
    <w:p/>
    <w:p>
      <w:r><w:t>Col1</w:t><w:tab/><w:t>Col2</w:t><w:br/><w:t><![CDATA[CData <ok>]]></w:t></w:r>
    </w:p>
  </w:body>
</w:document>"#;

        let bytes = build_zip(&[("word/document.xml", doc_xml)]);
        std::fs::write(&docx_path, bytes).unwrap();

        let extracted = extract_text(&docx_path, MIME_DOCX).await.unwrap().unwrap();
        assert_eq!(extracted, "Hola mundo & ★\n\nCol1\tCol2\nCData <ok>");
    }

    #[tokio::test]
    async fn test_extract_docx_empty_and_invalid() {
        let dir = tempdir().unwrap();

        let empty_docx = dir.path().join("vacio.docx");
        let empty_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>   </w:t></w:r></w:p></w:body></w:document>"#;
        std::fs::write(&empty_docx, build_zip(&[("word/document.xml", empty_xml)])).unwrap();
        assert_eq!(extract_text(&empty_docx, MIME_DOCX).await.unwrap(), None);

        let bad_zip = dir.path().join("roto.docx");
        std::fs::write(&bad_zip, b"no es un zip").unwrap();
        assert!(matches!(
            extract_text(&bad_zip, MIME_DOCX).await,
            Err(MoodleError::Protocol(_))
        ));

        let bad_xml_docx = dir.path().join("mal_xml.docx");
        std::fs::write(
            &bad_xml_docx,
            build_zip(&[("word/document.xml", "<w:document><w:p></w:document>")]),
        )
        .unwrap();
        assert!(matches!(
            extract_text(&bad_xml_docx, MIME_DOCX).await,
            Err(MoodleError::Protocol(_))
        ));

        std::fs::write(
            &bad_xml_docx,
            build_zip(&[("word/document.xml", "<w:document><w:p><w:t>parcial</w:t>")]),
        )
        .unwrap();
        assert!(matches!(
            extract_text(&bad_xml_docx, MIME_DOCX).await,
            Err(MoodleError::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn test_extract_pptx_numeric_slide_order_and_empty_slides() {
        let dir = tempdir().unwrap();
        let pptx_path = dir.path().join("clase.pptx");

        // Insertadas intencionalmente fuera de orden numérico (10, 2, 3, 1)
        let slide10 = r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Cierre diez</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
        let slide2 = r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Tema dos</a:t></a:r><a:br/><a:r><a:t>Detalle dos</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
        let slide3_empty = r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree/></p:cSld></p:sld>"#;
        let slide1 = r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Introducción uno</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
        let rels = r#"<Relationships/>"#;

        let bytes = build_zip(&[
            ("ppt/slides/slide10.xml", slide10),
            ("ppt/slides/_rels/slide10.xml.rels", rels),
            ("ppt/slides/slide2.xml", slide2),
            ("ppt/slides/slide3.xml", slide3_empty),
            ("ppt/slides/slide1.xml", slide1),
        ]);
        std::fs::write(&pptx_path, bytes).unwrap();

        let extracted = extract_text(&pptx_path, MIME_PPTX).await.unwrap().unwrap();
        assert_eq!(
            extracted,
            "## Diapositiva 1\nIntroducción uno\n\n## Diapositiva 2\nTema dos\nDetalle dos\n\n## Diapositiva 10\nCierre diez"
        );
    }

    #[tokio::test]
    async fn test_extract_html_text_json_xml_and_unsupported() {
        let dir = tempdir().unwrap();

        let html_path = dir.path().join("pagina.html");
        std::fs::write(&html_path, "<h1>Título</h1><p>Contenido <b>clave</b>.</p>").unwrap();
        let md = extract_text(&html_path, "text/html; charset=utf-8")
            .await
            .unwrap()
            .unwrap();
        assert!(md.contains("# Título"));
        assert!(md.contains("clave"));

        let latin_txt = dir.path().join("notas.txt");
        std::fs::write(&latin_txt, b"Hola \xFF mundo").unwrap();
        let txt = extract_text(&latin_txt, "text/plain")
            .await
            .unwrap()
            .unwrap();
        assert!(txt.contains("Hola"));
        assert!(txt.contains("mundo"));

        let json_path = dir.path().join("datos.json");
        std::fs::write(&json_path, r#"{"ok":true}"#).unwrap();
        assert_eq!(
            extract_text(&json_path, "application/json").await.unwrap(),
            Some(r#"{"ok":true}"#.to_string())
        );

        let xml_path = dir.path().join("esquema.xml");
        std::fs::write(&xml_path, "<root><item>1</item></root>").unwrap();
        assert_eq!(
            extract_text(&xml_path, "application/xml").await.unwrap(),
            Some("<root><item>1</item></root>".to_string())
        );

        let empty_txt = dir.path().join("vacio.txt");
        std::fs::write(&empty_txt, "   \n\t  ").unwrap();
        assert_eq!(extract_text(&empty_txt, "text/plain").await.unwrap(), None);

        let legacy_doc = dir.path().join("antiguo.doc");
        std::fs::write(&legacy_doc, b"OLE2_BINARY").unwrap();
        assert_eq!(
            extract_text(&legacy_doc, "application/msword")
                .await
                .unwrap(),
            None
        );
    }
}
