mod common;

use base64::Engine as _;
use common::{start_server, start_server_with_options};
use moodle_mcp::config::Config;
use moodle_mcp::mcp::output::ToolErrorEnvelope;
use rmcp::model::CallToolRequestParams;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn build_minimal_pdf(lines: &[&str]) -> Vec<u8> {
    let stream_content = if lines.is_empty() {
        "0 0 100 100 re f".to_string()
    } else {
        let mut ops = String::from("BT /FontA 12 Tf 72 720 Td\n");
        for (idx, line) in lines.iter().enumerate() {
            if idx > 0 {
                ops.push_str("0 -16 Td\n");
            }
            ops.push('(');
            ops.push_str(line);
            ops.push_str(") Tj\n");
        }
        ops.push_str("ET");
        ops
    };

    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /FontA 5 0 R >> >> >>".to_string(),
        format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream_content.len(),
            stream_content
        ),
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

fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn adler32_zlib(data: &[u8]) -> u32 {
    let mut s1 = 1u32;
    let mut s2 = 0u32;
    for &b in data {
        s1 = (s1 + u32::from(b)) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    (s2 << 16) | s1
}

fn push_png_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(chunk_type);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32_ieee(&crc_input).to_be_bytes());
}

fn build_minimal_png(rows: u32) -> Vec<u8> {
    assert!((1..=10_000).contains(&rows));
    let mut png = Vec::from(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&1u32.to_be_bytes()); // ancho = 1
    ihdr.extend_from_slice(&rows.to_be_bytes()); // alto = rows
    ihdr.push(8); // profundidad de bit
    ihdr.push(6); // RGBA
    ihdr.push(0); // compresión
    ihdr.push(0); // filtro
    ihdr.push(0); // entrelazado
    push_png_chunk(&mut png, b"IHDR", &ihdr);

    let mut raw = Vec::with_capacity(rows as usize * 5);
    for _ in 0..rows {
        raw.extend_from_slice(&[0x00, 0xFF, 0x00, 0x00, 0xFF]);
    }
    let len = raw.len() as u16;
    let mut zlib = Vec::with_capacity(raw.len() + 11);
    zlib.push(0x78);
    zlib.push(0x01);
    zlib.push(0x01); // bloque deflate final sin compresión
    zlib.extend_from_slice(&len.to_le_bytes());
    zlib.extend_from_slice(&(!len).to_le_bytes());
    zlib.extend_from_slice(&raw);
    zlib.extend_from_slice(&adler32_zlib(&raw).to_be_bytes());
    push_png_chunk(&mut png, b"IDAT", &zlib);

    push_png_chunk(&mut png, b"IEND", &[]);
    png
}

fn args_obj(val: Value) -> serde_json::Map<String, Value> {
    val.as_object().unwrap().clone()
}

#[tokio::test]
async fn test_moodle_view_annotations_and_download_inline_schema() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    let tools = client.list_all_tools().await.expect("list_all_tools");
    let tool_map: HashMap<String, _> = tools.into_iter().map(|t| (t.name.to_string(), t)).collect();

    let view = tool_map
        .get("moodle_view")
        .expect("moodle_view debe estar registrada");
    let ann = view.annotations.as_ref().expect("annotations");
    assert_eq!(ann.read_only_hint, Some(false));
    assert_ne!(ann.destructive_hint, Some(true));
    assert_eq!(ann.idempotent_hint, Some(true));

    let dl = tool_map
        .get("moodle_download")
        .expect("moodle_download debe estar registrada");
    let props = dl
        .input_schema
        .get("properties")
        .and_then(|v| v.as_object())
        .expect("properties en moodle_download");
    assert!(
        props.contains_key("inline"),
        "moodle_download debe exponer parámetro inline"
    );
}

#[tokio::test]
async fn test_moodle_view_strict_xor_and_invalid_path_errors() {
    let mock = MockServer::start().await;
    let dir = tempdir().unwrap();
    let client = start_server_with_options(&mock, None, None).await;

    let invalid_cases = vec![
        serde_json::json!({}),
        serde_json::json!({ "path": "   " }),
        serde_json::json!({ "url": "" }),
        serde_json::json!({ "path": "local.pdf", "url": "https://ejemplo.local/a.pdf" }),
        serde_json::json!({ "path": "   ", "url": "https://ejemplo.local/a.pdf" }),
        serde_json::json!({ "path": dir.path().join("inexistente.pdf").to_string_lossy() }),
        serde_json::json!({ "path": dir.path().to_string_lossy() }),
    ];

    for case in invalid_cases {
        let params = CallToolRequestParams::new("moodle_view").with_arguments(args_obj(case));
        let res = client.call_tool(params).await.expect("call_tool");
        assert_eq!(res.is_error, Some(true));
        let txt = res.content[0].as_text().unwrap();
        let env: ToolErrorEnvelope = serde_json::from_str(&txt.text).unwrap();
        assert_eq!(env.error.kind, "invalid_arguments");
    }
}

#[tokio::test]
async fn test_moodle_view_by_path_pdf_text_and_scanned_empty_without_session() {
    let mock = MockServer::start().await;
    let dir = tempdir().unwrap();

    let pdf_ok = dir.path().join("lectura.pdf");
    std::fs::write(&pdf_ok, build_minimal_pdf(&["Hola contenido PDF"])).unwrap();

    let pdf_scan = dir.path().join("escaneado.pdf");
    std::fs::write(&pdf_scan, build_minimal_pdf(&[])).unwrap();

    // Sin sesión activa: moodle_view con path local debe funcionar igualmente
    let client = start_server_with_options(&mock, None, None).await;

    let res_ok = client
        .call_tool(
            CallToolRequestParams::new("moodle_view").with_arguments(args_obj(serde_json::json!({
                "path": pdf_ok.to_string_lossy()
            }))),
        )
        .await
        .unwrap();
    assert!(!res_ok.is_error.unwrap_or(false));
    let val_ok: Value = serde_json::from_str(&res_ok.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(val_ok["mime"], "application/pdf");
    assert!(val_ok["content_markdown"]
        .as_str()
        .is_some_and(|s| s.contains("Hola contenido PDF")));

    let res_scan = client
        .call_tool(
            CallToolRequestParams::new("moodle_view").with_arguments(args_obj(serde_json::json!({
                "path": pdf_scan.to_string_lossy()
            }))),
        )
        .await
        .unwrap();
    assert!(!res_scan.is_error.unwrap_or(false));
    let val_scan: Value =
        serde_json::from_str(&res_scan.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(val_scan["mime"], "application/pdf");
    assert_eq!(val_scan["note"], "sin texto extraíble (posible escaneo)");
}

#[tokio::test]
async fn test_moodle_view_by_path_docx_pptx_and_unsupported_format() {
    let mock = MockServer::start().await;
    let dir = tempdir().unwrap();
    let config = Config {
        base_url: mock.uri(),
        download_dir: dir.path().to_path_buf(),
        max_response_bytes: 60_000,
        max_inline_image_bytes: 1_000_000,
    };
    let client = start_server_with_options(&mock, None, Some(config)).await;

    let docx_rel = dir.path().join("sub/guia.docx");
    std::fs::create_dir_all(docx_rel.parent().unwrap()).unwrap();
    let doc_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t xml:space="preserve">Tema </w:t></w:r><w:r><w:t>uno &amp; dos</w:t></w:r></w:p></w:body></w:document>"#;
    std::fs::write(&docx_rel, build_zip(&[("word/document.xml", doc_xml)])).unwrap();

    // Ruta relativa resuelta contra download_dir
    let res_docx = client
        .call_tool(
            CallToolRequestParams::new("moodle_view").with_arguments(args_obj(serde_json::json!({
                "path": "sub/guia.docx"
            }))),
        )
        .await
        .unwrap();
    assert!(!res_docx.is_error.unwrap_or(false));
    let val_docx: Value =
        serde_json::from_str(&res_docx.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(val_docx["content_markdown"], "Tema uno & dos");

    let pptx_path = dir.path().join("diapositivas.pptx");
    let slide1 = r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Inicio</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
    let slide2 = r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Desarrollo</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
    let slide10 = r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Resumen final</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
    std::fs::write(
        &pptx_path,
        build_zip(&[
            ("ppt/slides/slide10.xml", slide10),
            ("ppt/slides/slide2.xml", slide2),
            ("ppt/slides/slide1.xml", slide1),
        ]),
    )
    .unwrap();

    let res_pptx = client
        .call_tool(
            CallToolRequestParams::new("moodle_view").with_arguments(args_obj(serde_json::json!({
                "path": pptx_path.to_string_lossy()
            }))),
        )
        .await
        .unwrap();
    assert!(!res_pptx.is_error.unwrap_or(false));
    let val_pptx: Value =
        serde_json::from_str(&res_pptx.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(
        val_pptx["content_markdown"],
        "## Diapositiva 1\nInicio\n\n## Diapositiva 2\nDesarrollo\n\n## Diapositiva 10\nResumen final"
    );

    let bin_path = dir.path().join("paquete.zip");
    std::fs::write(&bin_path, build_zip(&[("dato.txt", "hola")])).unwrap();
    let res_bin = client
        .call_tool(
            CallToolRequestParams::new("moodle_view").with_arguments(args_obj(serde_json::json!({
                "path": bin_path.to_string_lossy()
            }))),
        )
        .await
        .unwrap();
    assert!(!res_bin.is_error.unwrap_or(false));
    let val_bin: Value = serde_json::from_str(&res_bin.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(val_bin["note"], "formato sin visor");

    // Archivo DOCX corrupto devuelve error tipado protocol
    let corrupt_docx = dir.path().join("roto.docx");
    std::fs::write(&corrupt_docx, b"esto no es un archivo zip valido").unwrap();
    let res_corrupt = client
        .call_tool(
            CallToolRequestParams::new("moodle_view").with_arguments(args_obj(serde_json::json!({
                "path": corrupt_docx.to_string_lossy()
            }))),
        )
        .await
        .unwrap();
    assert_eq!(res_corrupt.is_error, Some(true));
    let err_corrupt: ToolErrorEnvelope =
        serde_json::from_str(&res_corrupt.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(err_corrupt.error.kind, "protocol");

    // Ruta relativa existente en el directorio de trabajo actual (README.md)
    let res_rel_cwd = client
        .call_tool(
            CallToolRequestParams::new("moodle_view").with_arguments(args_obj(serde_json::json!({
                "path": "README.md"
            }))),
        )
        .await
        .unwrap();
    assert!(!res_rel_cwd.is_error.unwrap_or(false));
    let val_readme: Value =
        serde_json::from_str(&res_rel_cwd.content[0].as_text().unwrap().text).unwrap();
    assert!(val_readme["content_markdown"]
        .as_str()
        .is_some_and(|s| s.contains("moodle_view")));

    // Imagen WebP dentro del límite se incrusta en línea
    let webp_path = dir.path().join("grafico.webp");
    std::fs::write(&webp_path, b"RIFF\x00\x00\x00\x00WEBPVP8 ").unwrap();
    let res_webp = client
        .call_tool(
            CallToolRequestParams::new("moodle_view").with_arguments(args_obj(serde_json::json!({
                "path": webp_path.to_string_lossy()
            }))),
        )
        .await
        .unwrap();
    assert!(!res_webp.is_error.unwrap_or(false));
    assert_eq!(res_webp.content.len(), 2);
    assert_eq!(
        res_webp.content[1].as_image().unwrap().mime_type,
        "image/webp"
    );
}

#[tokio::test]
async fn test_moodle_view_png_within_and_above_max_inline_image_bytes() {
    let mock = MockServer::start().await;
    let dir = tempdir().unwrap();

    let png_small = build_minimal_png(1);
    let png_small_path = dir.path().join("icono.png");
    std::fs::write(&png_small_path, &png_small).unwrap();

    let png_big = build_minimal_png(50);
    let png_big_path = dir.path().join("grande.png");
    std::fs::write(&png_big_path, &png_big).unwrap();

    let config = Config {
        base_url: mock.uri(),
        download_dir: dir.path().to_path_buf(),
        max_response_bytes: 60_000,
        max_inline_image_bytes: png_small.len(), // límite exacto al tamaño de la imagen pequeña
    };
    let client = start_server_with_options(&mock, Some("tok123"), Some(config)).await;

    // 1. Cabe en el límite: devuelve texto JSON + ContentBlock::image
    let res_ok = client
        .call_tool(
            CallToolRequestParams::new("moodle_view").with_arguments(args_obj(serde_json::json!({
                "path": png_small_path.to_string_lossy()
            }))),
        )
        .await
        .unwrap();
    assert!(!res_ok.is_error.unwrap_or(false));
    assert_eq!(res_ok.content.len(), 2);

    let meta: Value = serde_json::from_str(&res_ok.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(meta["inline_image"], true);
    assert_eq!(meta["mime"], "image/png");
    assert_eq!(meta["size"], png_small.len());

    let img = res_ok.content[1]
        .as_image()
        .expect("el segundo bloque debe ser ContentBlock::image");
    assert_eq!(img.mime_type, "image/png");
    assert!(!img.data.starts_with("data:"));
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&img.data)
        .expect("base64 estándar válido");
    assert_eq!(decoded, png_small);

    // 2. Excede el límite: solo texto con nota, sin ContentBlock::image
    let res_big = client
        .call_tool(
            CallToolRequestParams::new("moodle_view").with_arguments(args_obj(serde_json::json!({
                "path": png_big_path.to_string_lossy()
            }))),
        )
        .await
        .unwrap();
    assert!(!res_big.is_error.unwrap_or(false));
    assert_eq!(res_big.content.len(), 1);
    assert!(res_big.content[0].as_image().is_none());

    let big_meta: Value =
        serde_json::from_str(&res_big.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(
        big_meta["note"],
        "imagen supera el límite para mostrarse en línea"
    );
}

#[tokio::test]
async fn test_moodle_view_document_overflow_saves_txt_without_double_truncation() {
    let mock = MockServer::start().await;
    let dir = tempdir().unwrap();

    // Documento PDF largo con múltiples líneas de texto que supera max_response_bytes
    let pdf_path = dir.path().join("guia_larga.pdf");
    let lines: Vec<&str> = (0..18)
        .map(|_| "Linea de contenido academico suficientemente larga para superar el limite de respuesta.")
        .collect();
    std::fs::write(&pdf_path, build_minimal_pdf(&lines)).unwrap();

    // Archivo UTF-8 multibyte con token incrustado para verificar frontera UTF-8 y scrub
    let utf8_path = dir.path().join("notas_utf8.txt");
    let utf8_body = format!(
        "{} tok_secreto_abc {}",
        "café 🦀 ".repeat(40),
        "fin_del_documento"
    );
    std::fs::write(&utf8_path, &utf8_body).unwrap();

    let max_resp = 420usize;
    let config = Config {
        base_url: mock.uri(),
        download_dir: dir.path().to_path_buf(),
        max_response_bytes: max_resp,
        max_inline_image_bytes: 1_000_000,
    };
    let client = start_server_with_options(&mock, Some("tok_secreto_abc"), Some(config)).await;

    // 1. PDF desbordado -> crea guia_larga.pdf.txt sin doble truncado en responses/
    let res_pdf = client
        .call_tool(
            CallToolRequestParams::new("moodle_view").with_arguments(args_obj(serde_json::json!({
                "path": pdf_path.to_string_lossy()
            }))),
        )
        .await
        .unwrap();
    assert!(!res_pdf.is_error.unwrap_or(false));
    let raw_json = &res_pdf.content[0].as_text().unwrap().text;
    let val: Value = serde_json::from_str(raw_json).unwrap();
    assert_eq!(val["truncated"], true);
    assert_eq!(val["mime"], "application/pdf");
    let expected_txt_path = format!("{}.txt", pdf_path.to_string_lossy());
    assert_eq!(val["text_saved_to"], expected_txt_path);
    assert!(val["preview"].as_str().is_some_and(|p| !p.is_empty()));
    assert!(raw_json.len() <= max_resp);
    assert!(std::path::Path::new(&expected_txt_path).exists());
    let saved_pdf_txt = std::fs::read_to_string(&expected_txt_path).unwrap();
    assert!(saved_pdf_txt.contains("Linea de contenido academico"));
    assert!(
        !dir.path().join("responses").exists(),
        "No debe producirse doble truncado en download_dir/responses"
    );

    // 2. Texto UTF-8 multibyte con límite pequeño (100 bytes) y token saneado
    let dir_small = tempdir().unwrap();
    let utf8_small_path = dir_small.path().join("multibyte.txt");
    std::fs::write(&utf8_small_path, &utf8_body).unwrap();
    let small_config = Config {
        base_url: mock.uri(),
        download_dir: dir_small.path().to_path_buf(),
        max_response_bytes: 100,
        max_inline_image_bytes: 1_000_000,
    };
    let small_client =
        start_server_with_options(&mock, Some("tok_secreto_abc"), Some(small_config)).await;

    let res_small = small_client
        .call_tool(
            CallToolRequestParams::new("moodle_view").with_arguments(args_obj(serde_json::json!({
                "path": utf8_small_path.to_string_lossy()
            }))),
        )
        .await
        .unwrap();
    assert!(!res_small.is_error.unwrap_or(false));
    let small_val: Value =
        serde_json::from_str(&res_small.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(small_val["truncated"], true);
    let preview = small_val["preview"].as_str().unwrap();
    assert!(preview.len() <= 100);
    assert!(!preview.is_empty());
    assert!(!preview.contains("tok_secreto_abc"));
    let saved_small =
        std::fs::read_to_string(small_val["text_saved_to"].as_str().unwrap()).unwrap();
    assert!(!saved_small.contains("tok_secreto_abc"));
    assert!(saved_small.contains("<token>"));
    assert!(saved_small.contains("fin_del_documento"));
    assert!(!dir_small.path().join("responses").exists());

    // Los caracteres escapados en JSON pueden superar el límite aunque el texto original quepa.
    let escaped_path = dir.path().join("saltos.txt");
    let escaped_text = "línea\n".repeat(55);
    assert!(escaped_text.len() < max_resp);
    std::fs::write(&escaped_path, &escaped_text).unwrap();
    let escaped_result = client
        .call_tool(
            CallToolRequestParams::new("moodle_view").with_arguments(args_obj(serde_json::json!({
                "path": escaped_path
            }))),
        )
        .await
        .unwrap();
    let escaped_response = &escaped_result.content[0].as_text().unwrap().text;
    let escaped_json: Value = serde_json::from_str(escaped_response).unwrap();
    assert_eq!(escaped_json["truncated"], true);
    assert!(escaped_response.len() <= max_resp);
    assert_eq!(
        std::fs::read_to_string(escaped_json["text_saved_to"].as_str().unwrap()).unwrap(),
        escaped_text
    );
    assert!(!dir.path().join("responses").exists());
}

#[tokio::test]
async fn test_moodle_view_by_url_wiremock_and_mime_fallback() {
    let mock = MockServer::start().await;
    let slide1 = r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Diapositiva remota</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
    let pptx_bytes = build_zip(&[("ppt/slides/slide1.xml", slide1)]);

    // El servidor devuelve application/octet-stream genérico: resolve_mime debe inferir PPTX
    Mock::given(method("GET"))
        .and(path("/webservice/pluginfile.php/10/presentacion.pptx"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(pptx_bytes)
                .insert_header("content-type", "application/octet-stream"),
        )
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;
    let url = format!("{}/pluginfile.php/10/presentacion.pptx", mock.uri());

    let res = client
        .call_tool(
            CallToolRequestParams::new("moodle_view")
                .with_arguments(args_obj(serde_json::json!({ "url": url }))),
        )
        .await
        .unwrap();
    assert!(!res.is_error.unwrap_or(false));
    let val: Value = serde_json::from_str(&res.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(
        val["mime"],
        "application/vnd.openxmlformats-officedocument.presentationml.presentation"
    );
    assert_eq!(
        val["content_markdown"],
        "## Diapositiva 1\nDiapositiva remota"
    );

    // Dominio externo rechazado sin enviar token
    let ext_res = client
        .call_tool(
            CallToolRequestParams::new("moodle_view").with_arguments(args_obj(serde_json::json!({
                "url": "https://externo.example.com/presentacion.pptx"
            }))),
        )
        .await
        .unwrap();
    assert_eq!(ext_res.is_error, Some(true));
    let ext_env: ToolErrorEnvelope =
        serde_json::from_str(&ext_res.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(ext_env.error.kind, "invalid_arguments");

    // Sin sesión activa, moodle_view(url=...) devuelve no_session
    let no_session_client = start_server_with_options(&mock, None, None).await;
    let no_sess_res = no_session_client
        .call_tool(
            CallToolRequestParams::new("moodle_view")
                .with_arguments(args_obj(serde_json::json!({ "url": url }))),
        )
        .await
        .unwrap();
    assert_eq!(no_sess_res.is_error, Some(true));
    let no_sess_env: ToolErrorEnvelope =
        serde_json::from_str(&no_sess_res.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(no_sess_env.error.kind, "no_session");
}

#[tokio::test]
async fn test_moodle_download_inline_true_and_false_backward_compatibility() {
    let mock = MockServer::start().await;
    let pdf_bytes = build_minimal_pdf(&["Texto descargado inline"]);
    let png_bytes = build_minimal_png(2);

    Mock::given(method("GET"))
        .and(path("/webservice/pluginfile.php/20/apunte.pdf"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(pdf_bytes.clone())
                .insert_header("content-type", "application/pdf"),
        )
        .expect(2)
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/webservice/pluginfile.php/21/diagrama.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(png_bytes.clone())
                .insert_header("content-type", "image/png"),
        )
        .expect(2)
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/webservice/pluginfile.php/22/foto.jpg"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(vec![0xFF, 0xD8, 0xFF, 0xD9])
                .insert_header("content-type", "image/jpg"),
        )
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;
    let pdf_url = format!("{}/pluginfile.php/20/apunte.pdf", mock.uri());
    let png_url = format!("{}/pluginfile.php/21/diagrama.png", mock.uri());
    let jpg_url = format!("{}/pluginfile.php/22/foto.jpg", mock.uri());

    // 1. inline = false (retrocompatible) en PDF: sin content_markdown
    let res_pdf_legacy = client
        .call_tool(
            CallToolRequestParams::new("moodle_download").with_arguments(args_obj(
                serde_json::json!({
                    "url": pdf_url,
                    "inline": false
                }),
            )),
        )
        .await
        .unwrap();
    assert!(!res_pdf_legacy.is_error.unwrap_or(false));
    let val_legacy: Value =
        serde_json::from_str(&res_pdf_legacy.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(val_legacy["mime"], "application/pdf");
    assert!(val_legacy.get("content_markdown").is_none());

    // 2. inline = true en PDF: extrae texto
    let res_pdf_inline = client
        .call_tool(
            CallToolRequestParams::new("moodle_download").with_arguments(args_obj(
                serde_json::json!({
                    "url": pdf_url,
                    "inline": true
                }),
            )),
        )
        .await
        .unwrap();
    assert!(!res_pdf_inline.is_error.unwrap_or(false));
    let val_inline: Value =
        serde_json::from_str(&res_pdf_inline.content[0].as_text().unwrap().text).unwrap();
    assert!(val_inline["content_markdown"]
        .as_str()
        .is_some_and(|s| s.contains("Texto descargado inline")));

    // 3. inline omitido en PNG (retrocompatible): 1 bloque, sin imagen en línea
    let res_png_legacy = client
        .call_tool(
            CallToolRequestParams::new("moodle_download")
                .with_arguments(args_obj(serde_json::json!({ "url": png_url }))),
        )
        .await
        .unwrap();
    assert!(!res_png_legacy.is_error.unwrap_or(false));
    assert_eq!(res_png_legacy.content.len(), 1);

    // 4. inline = true en PNG: devuelve metadatos + ContentBlock::image
    let res_png_inline = client
        .call_tool(
            CallToolRequestParams::new("moodle_download").with_arguments(args_obj(
                serde_json::json!({
                    "url": png_url,
                    "inline": true
                }),
            )),
        )
        .await
        .unwrap();
    assert!(!res_png_inline.is_error.unwrap_or(false));
    assert_eq!(res_png_inline.content.len(), 2);
    let img = res_png_inline.content[1].as_image().unwrap();
    assert_eq!(img.mime_type, "image/png");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(&img.data)
            .unwrap(),
        png_bytes
    );

    // 5. Normalización de image/jpg -> image/jpeg
    let res_jpg = client
        .call_tool(
            CallToolRequestParams::new("moodle_download").with_arguments(args_obj(
                serde_json::json!({
                    "url": jpg_url,
                    "inline": true
                }),
            )),
        )
        .await
        .unwrap();
    assert!(!res_jpg.is_error.unwrap_or(false));
    assert_eq!(res_jpg.content.len(), 2);
    let jpg_meta: Value =
        serde_json::from_str(&res_jpg.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(jpg_meta["mime"], "image/jpeg");
    assert_eq!(
        res_jpg.content[1].as_image().unwrap().mime_type,
        "image/jpeg"
    );
}
