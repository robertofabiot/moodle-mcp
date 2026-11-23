mod common;

use common::start_server;
use moodle_mcp::moodle::client::{MoodleClient, MoodleClientBuilder};
use moodle_mcp::moodle::error::MoodleError;
use moodle_mcp::moodle::files::{authorize_file_url, sanitize_filename, FileClient};
use moodle_mcp::moodle::gate::MoodleGate;
use rmcp::model::CallToolRequestParams;
use std::collections::HashMap;
use std::io::Write;
use tempfile::{tempdir, NamedTempFile};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_files_tools_annotations() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    let tools = client.list_all_tools().await.expect("list_all_tools");
    let tool_map: HashMap<String, _> = tools.into_iter().map(|t| (t.name.to_string(), t)).collect();

    // 1. moodle_download: read_only_hint = false, destructive_hint = false, idempotent_hint = true
    let dl = tool_map
        .get("moodle_download")
        .expect("moodle_download debe estar registrada");
    let dl_ann = dl.annotations.as_ref().expect("annotations");
    assert_eq!(dl_ann.read_only_hint, Some(false));
    assert_ne!(dl_ann.destructive_hint, Some(true));
    assert_eq!(dl_ann.idempotent_hint, Some(true));

    // 2. moodle_upload: destructive_hint = false
    let up = tool_map
        .get("moodle_upload")
        .expect("moodle_upload debe estar registrada");
    let up_ann = up.annotations.as_ref().expect("annotations");
    assert_ne!(up_ann.destructive_hint, Some(true));
}

#[test]
fn test_authorize_file_url_unit_matrix() {
    let base = Url::parse("https://uamvirtual.uam.edu.ni/grado").unwrap();

    // 1. Reescribe pluginfile.php
    let u1 = authorize_file_url(
        &base,
        "https://uamvirtual.uam.edu.ni/grado/pluginfile.php/456/mod_assign/intro/guia.pdf",
        "mi_token",
    )
    .unwrap();
    assert_eq!(
        u1.path(),
        "/grado/webservice/pluginfile.php/456/mod_assign/intro/guia.pdf"
    );
    assert_eq!(u1.query(), Some("token=mi_token"));

    // 2. Conserva query params previos y sobreescribe token
    let u2 = authorize_file_url(
        &base,
        "https://uamvirtual.uam.edu.ni/grado/webservice/pluginfile.php/456/doc.pdf?forcedownload=1&token=previo",
        "nuevo_token",
    )
    .unwrap();
    let q: HashMap<_, _> = u2.query_pairs().collect();
    assert_eq!(q.get("forcedownload"), Some(&"1".into()));
    assert_eq!(q.get("token"), Some(&"nuevo_token".into()));

    // 3. No altera tokenpluginfile.php
    let u3 = authorize_file_url(
        &base,
        "https://uamvirtual.uam.edu.ni/grado/tokenpluginfile.php/456/doc.pdf",
        "mi_token",
    )
    .unwrap();
    assert_eq!(u3.path(), "/grado/tokenpluginfile.php/456/doc.pdf");
    assert_eq!(u3.query(), Some("token=mi_token"));

    // 4. Rechaza host externo
    let err_host = authorize_file_url(&base, "https://drive.google.com/open?id=123", "tok");
    assert!(matches!(err_host, Err(MoodleError::InvalidArguments(_))));

    // 5. Rechaza subruta fuera de /grado en el mismo host
    let err_path = authorize_file_url(
        &base,
        "https://uamvirtual.uam.edu.ni/otra/pluginfile.php/1/doc.pdf",
        "tok",
    );
    assert!(matches!(err_path, Err(MoodleError::InvalidArguments(_))));
}

#[test]
fn test_sanitize_filename_unit_matrix() {
    assert_eq!(sanitize_filename("../../etc/shadow"), "etcshadow");
    assert_eq!(sanitize_filename("a/b/c\\d.txt"), "abcd.txt");
    assert_eq!(sanitize_filename("..."), "archivo");
    assert_eq!(sanitize_filename("   "), "archivo");
    assert_eq!(sanitize_filename("prueba\r\n\t.pdf"), "prueba.pdf");

    // Límite de 200 bytes asegurando char boundary
    let long_str = "x".repeat(300);
    assert_eq!(sanitize_filename(&long_str).len(), 200);

    let emojis = "🔥".repeat(60); // 4 bytes cada uno = 240 bytes
    let sanitized_emojis = sanitize_filename(&emojis);
    assert!(sanitized_emojis.len() <= 200);
    assert_eq!(sanitized_emojis.len() % 4, 0);
}

#[tokio::test]
async fn test_moodle_download_binary_file_success() {
    let mock = MockServer::start().await;
    let mock_bytes = vec![0x25, 0x50, 0x44, 0x46, 0x2D, 0x31, 0x2E, 0x34]; // %PDF-1.4

    Mock::given(method("GET"))
        .and(path(
            "/webservice/pluginfile.php/123/mod_resource/content/1/reporte.pdf",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(mock_bytes.clone())
                .insert_header("content-type", "application/pdf")
                .insert_header(
                    "content-disposition",
                    "attachment; filename=\"reporte_final.pdf\"",
                ),
        )
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let file_url = format!(
        "{}/pluginfile.php/123/mod_resource/content/1/reporte.pdf",
        mock.uri()
    );
    let args = serde_json::json!({
        "url": file_url,
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_download").with_arguments(args);
    let result = client
        .call_tool(params)
        .await
        .expect("call_tool moodle_download");

    assert!(!result.is_error.unwrap_or(false));
    let text = result.content[0].as_text().unwrap();
    let val: serde_json::Value = serde_json::from_str(&text.text).unwrap();

    assert_eq!(val["mime"], "application/pdf");
    assert_eq!(val["size"], 8);
    let path_str = val["path"].as_str().unwrap();
    assert!(path_str.ends_with("reporte_final.pdf"));
    assert!(std::path::Path::new(path_str).exists());
}

#[tokio::test]
async fn test_moodle_download_html_file_includes_markdown() {
    let mock = MockServer::start().await;
    let html_content = "<h1>Guía de estudio</h1><p>Revisar capítulo <b>tres</b>.</p>";

    Mock::given(method("GET"))
        .and(path("/webservice/pluginfile.php/999/guia.html"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(html_content.as_bytes().to_vec())
                .insert_header("content-type", "text/html; charset=utf-8"),
        )
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let file_url = format!("{}/pluginfile.php/999/guia.html", mock.uri());
    let args = serde_json::json!({
        "url": file_url,
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_download").with_arguments(args);
    let result = client.call_tool(params).await.expect("call_tool");

    assert!(!result.is_error.unwrap_or(false));
    let text = result.content[0].as_text().unwrap();
    let val: serde_json::Value = serde_json::from_str(&text.text).unwrap();

    assert_eq!(val["mime"], "text/html");
    let md = val["content_markdown"]
        .as_str()
        .expect("debe incluir content_markdown");
    assert!(md.contains("# Guía de estudio"));
    assert!(md.contains("capítulo"));
}

#[tokio::test]
async fn test_moodle_download_pluginfile_json_error_maps_to_typed_error() {
    let mock = MockServer::start().await;
    let json_error = r#"{"error":"Acceso denegado a este archivo","errorcode":"accessdenied"}"#;

    Mock::given(method("GET"))
        .and(path("/webservice/pluginfile.php/private/secreto.pdf"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(json_error.as_bytes().to_vec())
                .insert_header("content-type", "application/json"),
        )
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let file_url = format!("{}/pluginfile.php/private/secreto.pdf", mock.uri());
    let args = serde_json::json!({
        "url": file_url,
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_download").with_arguments(args);
    let result = client.call_tool(params).await.expect("call_tool");

    assert!(result.is_error.unwrap_or(false));
    let text = result.content[0].as_text().unwrap();
    let val: serde_json::Value = serde_json::from_str(&text.text).unwrap();

    assert_eq!(val["error"]["kind"], "api");
    assert_eq!(val["error"]["errorcode"], "accessdenied");
}

#[tokio::test]
async fn test_moodle_download_external_url_rejected_with_invalid_arguments() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    let args = serde_json::json!({
        "url": "https://otro-dominio-externo.com/archivo.pdf",
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_download").with_arguments(args);
    let result = client.call_tool(params).await.expect("call_tool");

    assert!(result.is_error.unwrap_or(false));
    let text = result.content[0].as_text().unwrap();
    let val: serde_json::Value = serde_json::from_str(&text.text).unwrap();

    assert_eq!(val["error"]["kind"], "invalid_arguments");
    assert!(val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("URL externa"));
}

#[tokio::test]
async fn test_moodle_upload_files_success() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/upload.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "component": "user",
                "contextid": 12,
                "userid": 10001,
                "filearea": "draft",
                "filename": "tarea_entrega.pdf",
                "filepath": "/",
                "itemid": 789456,
                "license": "allrightsreserved",
                "author": "Estudiante",
                "filesize": 1024
            }
        ])))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"Contenido de prueba de la entrega").unwrap();
    tmp.flush().unwrap();

    let path_str = tmp.path().to_string_lossy().to_string();

    let args = serde_json::json!({
        "paths": [path_str],
        "itemid": 0,
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_upload").with_arguments(args);
    let result = client.call_tool(params).await.expect("call_tool");

    assert!(!result.is_error.unwrap_or(false));
    let text = result.content[0].as_text().unwrap();
    let val: serde_json::Value = serde_json::from_str(&text.text).unwrap();

    assert_eq!(val["itemid"], 789456);
    assert_eq!(val["files"][0], "tarea_entrega.pdf");
}

#[tokio::test]
async fn test_moodle_upload_error_maps_to_api_not_invalid_login() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/upload.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "error": "El archivo excede el tamaño máximo permitido",
            "errorcode": "upload_error_max_size",
            "debuginfo": "Limite configurado: 2MB"
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"datos de prueba").unwrap();
    tmp.flush().unwrap();

    let path_str = tmp.path().to_string_lossy().to_string();

    let args = serde_json::json!({
        "paths": [path_str],
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_upload").with_arguments(args);
    let result = client.call_tool(params).await.expect("call_tool");

    assert!(result.is_error.unwrap_or(false));
    let text = result.content[0].as_text().unwrap();
    let val: serde_json::Value = serde_json::from_str(&text.text).unwrap();

    assert_eq!(val["error"]["kind"], "api");
    assert_eq!(val["error"]["errorcode"], "upload_error_max_size");
}

#[tokio::test]
async fn test_moodle_upload_nonexistent_file_returns_invalid_arguments() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    let args = serde_json::json!({
        "paths": ["/ruta/inexistente/archivo_fantasma_12345.xyz"],
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_upload").with_arguments(args);
    let result = client.call_tool(params).await.expect("call_tool");

    assert!(result.is_error.unwrap_or(false));
    let text = result.content[0].as_text().unwrap();
    let val: serde_json::Value = serde_json::from_str(&text.text).unwrap();

    assert_eq!(val["error"]["kind"], "invalid_arguments");
}

#[tokio::test]
async fn test_moodle_upload_empty_paths_returns_invalid_arguments() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    let args = serde_json::json!({
        "paths": [],
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_upload").with_arguments(args);
    let result = client.call_tool(params).await.expect("call_tool");

    assert!(result.is_error.unwrap_or(false));
    let text = result.content[0].as_text().unwrap();
    let val: serde_json::Value = serde_json::from_str(&text.text).unwrap();

    assert_eq!(val["error"]["kind"], "invalid_arguments");
}

#[tokio::test]
async fn test_file_client_large_file_download_success() {
    let mock = MockServer::start().await;
    let large_size = 5 * 1024 * 1024; // 5 MB
    let large_payload = vec![0x41; large_size];

    Mock::given(method("GET"))
        .and(path("/webservice/pluginfile.php/1/large.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(large_payload)
                .insert_header("content-type", "application/octet-stream")
                .insert_header("content-disposition", "attachment; filename=\"large.bin\""),
        )
        .expect(1)
        .mount(&mock)
        .await;

    let gate = MoodleGate::new(4, 3).without_jitter();
    let moodle_client =
        MoodleClient::from_builder(MoodleClientBuilder::new().base_url(mock.uri()).gate(gate));
    let file_client = FileClient::new(&moodle_client);

    let base_url = Url::parse(&mock.uri()).unwrap();
    let authorized_url = authorize_file_url(
        &base_url,
        &format!("{}/pluginfile.php/1/large.bin", mock.uri()),
        "tok123",
    )
    .unwrap();

    let td = tempdir().unwrap();
    let downloaded = file_client
        .download(&authorized_url, None, td.path())
        .await
        .expect("descarga de 5 MB exitosa");

    assert_eq!(downloaded.size, large_size as u64);
    assert!(downloaded.path.exists());
    assert_eq!(
        std::fs::metadata(&downloaded.path).unwrap().len(),
        large_size as u64
    );
}
