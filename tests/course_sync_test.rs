mod common;

use common::start_server;
use moodle_mcp::moodle::client::{MoodleClient, MoodleClientBuilder};
use moodle_mcp::moodle::course_tree::Section;
use moodle_mcp::moodle::files::FileClient;
use moodle_mcp::moodle::gate::MoodleGate;
use moodle_mcp::sync::sync_course;
use rmcp::model::CallToolRequestParams;
use serde_json::json;
use std::collections::HashMap;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_sync_tools_annotations() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    let tools = client.list_all_tools().await.expect("list_all_tools");
    let tool_map: HashMap<String, _> = tools.into_iter().map(|t| (t.name.to_string(), t)).collect();

    let sync_tool = tool_map
        .get("moodle_sync_course")
        .expect("moodle_sync_course debe estar registrada");
    let ann = sync_tool.annotations.as_ref().expect("annotations");
    assert_eq!(ann.read_only_hint, Some(false));
    assert_ne!(ann.destructive_hint, Some(true));
    assert_eq!(ann.idempotent_hint, Some(true));
}

#[test]
fn test_course_contents_fixture_deserialization() {
    let fixture = include_str!("fixtures/course_contents.json");
    let sections: Vec<Section> =
        serde_json::from_str(fixture).expect("deserializar course_contents.json");
    assert!(!sections.is_empty(), "debe contener secciones");

    let first = &sections[0];
    assert_eq!(first.section, 0);
    assert!(!first.name.is_empty());
    assert!(!first.modules.is_empty());

    // Verificar que existen módulos con contenidos
    let has_contents = sections
        .iter()
        .flat_map(|s| &s.modules)
        .any(|m| !m.contents.is_empty());
    assert!(has_contents, "deben existir módulos con contenidos");
}

#[tokio::test]
async fn test_sync_course_incremental_manifest_and_skip() {
    let mock = MockServer::start().await;
    let base_url = mock.uri();
    let gate = MoodleGate::new(4, 3).without_jitter();
    let client =
        MoodleClient::from_builder(MoodleClientBuilder::new().base_url(&base_url).gate(gate));
    let file_client = FileClient::new(&client);

    let temp = tempdir().expect("tempdir");
    let download_dir = temp.path().join("downloads");
    let dest_dir = temp.path().join("curso_sync");

    // Mock para descarga de archivo 1
    Mock::given(method("GET"))
        .and(path("/webservice/pluginfile.php/101/doc1.pdf"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"contenido doc 1"))
        .mount(&mock)
        .await;

    // Mock para descarga de archivo 2
    Mock::given(method("GET"))
        .and(path("/webservice/pluginfile.php/102/doc2.pdf"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"contenido doc 2"))
        .mount(&mock)
        .await;

    // Primer árbol del curso: 2 archivos
    let tree_run1 = json!([
        {
            "id": 1,
            "section": 1,
            "name": "Unidad 1",
            "summary": "",
            "visible": 1,
            "modules": [
                {
                    "id": 10,
                    "modname": "resource",
                    "name": "Lectura 1",
                    "visible": 1,
                    "uservisible": true,
                    "contents": [
                        {
                            "type": "file",
                            "filename": "doc1.pdf",
                            "filepath": "/",
                            "filesize": 15,
                            "fileurl": format!("{base_url}/pluginfile.php/101/doc1.pdf"),
                            "timemodified": 1000
                        }
                    ]
                },
                {
                    "id": 20,
                    "modname": "resource",
                    "name": "Lectura 2",
                    "visible": 1,
                    "uservisible": true,
                    "contents": [
                        {
                            "type": "file",
                            "filename": "doc2.pdf",
                            "filepath": "/",
                            "filesize": 15,
                            "fileurl": format!("{base_url}/pluginfile.php/102/doc2.pdf"),
                            "timemodified": 2000
                        }
                    ]
                }
            ]
        }
    ]);

    // Mock de core_course_get_contents para run 1
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&tree_run1))
        .up_to_n_times(1)
        .mount(&mock)
        .await;

    // Ejecución 1: debe descargar ambos archivos
    let res1 = sync_course(
        &client,
        &file_client,
        "tok123",
        55,
        Some("Curso Prueba"),
        Some(dest_dir.clone()),
        &download_dir,
    )
    .await
    .expect("sync_course run 1");

    assert_eq!(
        res1.downloaded, 2,
        "debe descargar 2 archivos en la primera ejecución"
    );
    assert_eq!(
        res1.skipped, 0,
        "debe saltar 0 archivos en la primera ejecución"
    );
    assert!(res1.failed.is_empty());

    // Verificar que el manifiesto existe
    let manifest_file = dest_dir.join(".moodle-sync.json");
    assert!(manifest_file.exists(), ".moodle-sync.json debe existir");

    // Mock de core_course_get_contents para run 2 (mismo contenido)
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&tree_run1))
        .up_to_n_times(1)
        .mount(&mock)
        .await;

    // Ejecución 2: sin cambios, debe saltar 2 y descargar 0
    let res2 = sync_course(
        &client,
        &file_client,
        "tok123",
        55,
        Some("Curso Prueba"),
        Some(dest_dir.clone()),
        &download_dir,
    )
    .await
    .expect("sync_course run 2");

    assert_eq!(
        res2.downloaded, 0,
        "debe descargar 0 archivos en la segunda ejecución sin cambios"
    );
    assert_eq!(
        res2.skipped, 2,
        "debe saltar 2 archivos en la segunda ejecución sin cambios"
    );
    assert!(res2.failed.is_empty());

    // Modificar timemodified de doc1.pdf para run 3
    let tree_run3 = json!([
        {
            "id": 1,
            "section": 1,
            "name": "Unidad 1",
            "summary": "",
            "visible": 1,
            "modules": [
                {
                    "id": 10,
                    "modname": "resource",
                    "name": "Lectura 1",
                    "visible": 1,
                    "uservisible": true,
                    "contents": [
                        {
                            "type": "file",
                            "filename": "doc1.pdf",
                            "filepath": "/",
                            "filesize": 15,
                            "fileurl": format!("{base_url}/pluginfile.php/101/doc1.pdf"),
                            "timemodified": 1005 // Modificado!
                        }
                    ]
                },
                {
                    "id": 20,
                    "modname": "resource",
                    "name": "Lectura 2",
                    "visible": 1,
                    "uservisible": true,
                    "contents": [
                        {
                            "type": "file",
                            "filename": "doc2.pdf",
                            "filepath": "/",
                            "filesize": 15,
                            "fileurl": format!("{base_url}/pluginfile.php/102/doc2.pdf"),
                            "timemodified": 2000 // Sin cambio
                        }
                    ]
                }
            ]
        }
    ]);

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&tree_run3))
        .up_to_n_times(1)
        .mount(&mock)
        .await;

    // Ejecución 3: debe descargar solo doc1 (downloaded=1, skipped=1)
    let res3 = sync_course(
        &client,
        &file_client,
        "tok123",
        55,
        Some("Curso Prueba"),
        Some(dest_dir.clone()),
        &download_dir,
    )
    .await
    .expect("sync_course run 3");

    assert_eq!(
        res3.downloaded, 1,
        "debe descargar solo el archivo modificado"
    );
    assert_eq!(res3.skipped, 1, "debe saltar el archivo no modificado");
    assert!(res3.failed.is_empty());
}

#[tokio::test]
async fn test_sync_course_page_module_generates_markdown() {
    let mock = MockServer::start().await;
    let base_url = mock.uri();
    let gate = MoodleGate::new(4, 3).without_jitter();
    let client =
        MoodleClient::from_builder(MoodleClientBuilder::new().base_url(&base_url).gate(gate));
    let file_client = FileClient::new(&client);

    let temp = tempdir().expect("tempdir");
    let download_dir = temp.path().join("downloads");
    let dest_dir = temp.path().join("curso_page");

    let page_html = "<h2>Bienvenida al Curso</h2><p>Contenido detallado en <b>HTML</b>.</p>";

    Mock::given(method("GET"))
        .and(path("/webservice/pluginfile.php/500/index.html"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(page_html.as_bytes())
                .insert_header("Content-Type", "text/html"),
        )
        .mount(&mock)
        .await;

    let tree = json!([
        {
            "id": 1,
            "section": 1,
            "name": "General",
            "modules": [
                {
                    "id": 99,
                    "modname": "page",
                    "name": "Guia de Estudio",
                    "uservisible": true,
                    "contents": [
                        {
                            "type": "file",
                            "filename": "index.html",
                            "filepath": "/",
                            "filesize": page_html.len() as i64,
                            "fileurl": format!("{base_url}/pluginfile.php/500/index.html"),
                            "timemodified": 3000
                        }
                    ]
                }
            ]
        }
    ]);

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&tree))
        .mount(&mock)
        .await;

    let res = sync_course(
        &client,
        &file_client,
        "tok123",
        10,
        None,
        Some(dest_dir.clone()),
        &download_dir,
    )
    .await
    .expect("sync page");

    assert_eq!(res.downloaded, 1);
    assert_eq!(res.skipped, 0);

    let html_path = dest_dir
        .join("01 - General")
        .join("Guia de Estudio")
        .join("index.html");
    let md_path = dest_dir
        .join("01 - General")
        .join("Guia de Estudio")
        .join("index.md");

    assert!(html_path.exists(), "index.html debe existir");
    assert!(md_path.exists(), "index.md debe existir");

    let md_content = tokio::fs::read_to_string(&md_path)
        .await
        .expect("leer index.md");
    assert!(md_content.contains("## Bienvenida al Curso"));
    assert!(md_content.contains("HTML"));
}

#[tokio::test]
async fn test_sync_course_external_links_and_enlaces_md() {
    let mock = MockServer::start().await;
    let base_url = mock.uri();
    let gate = MoodleGate::new(4, 3).without_jitter();
    let client =
        MoodleClient::from_builder(MoodleClientBuilder::new().base_url(&base_url).gate(gate));
    let file_client = FileClient::new(&client);

    let temp = tempdir().expect("tempdir");
    let download_dir = temp.path().join("downloads");
    let dest_dir = temp.path().join("curso_links");

    let tree = json!([
        {
            "id": 1,
            "section": 1,
            "name": "Recursos Externos",
            "modules": [
                {
                    "id": 101,
                    "modname": "url",
                    "name": "Clases en Google Meet",
                    "uservisible": true,
                    "contents": [
                        {
                            "type": "url",
                            "filename": "meet_link",
                            "fileurl": "https://meet.google.com/abc-defg-hij"
                        }
                    ]
                },
                {
                    "id": 102,
                    "modname": "resource",
                    "name": "Carpeta Google Drive",
                    "uservisible": true,
                    "contents": [
                        {
                            "type": "file",
                            "filename": "drive_folder",
                            "fileurl": "https://drive.google.com/drive/folders/xyz123"
                        }
                    ]
                }
            ]
        }
    ]);

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&tree))
        .mount(&mock)
        .await;

    let res = sync_course(
        &client,
        &file_client,
        "tok123",
        20,
        None,
        Some(dest_dir.clone()),
        &download_dir,
    )
    .await
    .expect("sync external links");

    assert_eq!(res.downloaded, 0, "no debe descargar enlaces externos");
    assert_eq!(
        res.external_links.len(),
        2,
        "debe registrar 2 enlaces externos"
    );

    let enlaces_file = dest_dir.join("enlaces.md");
    assert!(enlaces_file.exists(), "enlaces.md debe haberse generado");

    let enlaces_text = tokio::fs::read_to_string(&enlaces_file)
        .await
        .expect("leer enlaces.md");
    assert!(enlaces_text.contains("https://meet.google.com/abc-defg-hij"));
    assert!(enlaces_text.contains("https://drive.google.com/drive/folders/xyz123"));
}

#[tokio::test]
async fn test_sync_course_failed_file_404_recorded_in_failed_and_continues() {
    let mock = MockServer::start().await;
    let base_url = mock.uri();
    let gate = MoodleGate::new(4, 3).without_jitter();
    let client =
        MoodleClient::from_builder(MoodleClientBuilder::new().base_url(&base_url).gate(gate));
    let file_client = FileClient::new(&client);

    let temp = tempdir().expect("tempdir");
    let download_dir = temp.path().join("downloads");
    let dest_dir = temp.path().join("curso_failed");

    // Archivo 1 devuelve 404
    Mock::given(method("GET"))
        .and(path("/webservice/pluginfile.php/404/not_found.pdf"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;

    // Archivo 2 devuelve 200
    Mock::given(method("GET"))
        .and(path("/webservice/pluginfile.php/200/ok.pdf"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"exito"))
        .mount(&mock)
        .await;

    let tree = json!([
        {
            "id": 1,
            "section": 1,
            "name": "Seccion Prueba",
            "modules": [
                {
                    "id": 1,
                    "modname": "resource",
                    "name": "Archivo Roto",
                    "uservisible": true,
                    "contents": [
                        {
                            "type": "file",
                            "filename": "not_found.pdf",
                            "fileurl": format!("{base_url}/pluginfile.php/404/not_found.pdf")
                        }
                    ]
                },
                {
                    "id": 2,
                    "modname": "resource",
                    "name": "Archivo Bueno",
                    "uservisible": true,
                    "contents": [
                        {
                            "type": "file",
                            "filename": "ok.pdf",
                            "fileurl": format!("{base_url}/pluginfile.php/200/ok.pdf")
                        }
                    ]
                }
            ]
        }
    ]);

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&tree))
        .mount(&mock)
        .await;

    let res = sync_course(
        &client,
        &file_client,
        "tok123",
        30,
        None,
        Some(dest_dir.clone()),
        &download_dir,
    )
    .await
    .expect("sync con fallo parcial");

    assert_eq!(res.downloaded, 1, "el archivo correcto debe descargarse");
    assert_eq!(res.failed.len(), 1, "debe registrar 1 fallo");
    assert!(res.failed[0].path.contains("not_found.pdf"));
}

#[tokio::test]
async fn test_mcp_tool_moodle_sync_course_e2e() {
    let mock = MockServer::start().await;
    let base_url = mock.uri();
    let client = start_server(&mock).await;

    let tree = json!([
        {
            "id": 1,
            "section": 1,
            "name": "Seccion E2E",
            "modules": []
        }
    ]);

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&tree))
        .mount(&mock)
        .await;

    let temp = tempdir().expect("tempdir");
    let dest_path = temp.path().join("curso_e2e").to_string_lossy().into_owned();

    let args = json!({
        "courseid": 42,
        "dest": dest_path
    })
    .as_object()
    .unwrap()
    .clone();

    let res = client
        .call_tool(CallToolRequestParams::new("moodle_sync_course").with_arguments(args))
        .await
        .expect("call moodle_sync_course");

    let is_error = res.is_error.unwrap_or(false);
    assert!(!is_error, "no debe retornar error: {res:?}");
    let text = res.content[0].as_text().expect("texto").text.clone();
    assert!(text.contains("\"downloaded\":0"));
    assert!(text.contains("\"skipped\":0"));
    assert!(text.contains("\"failed\":[]"));
    assert!(
        !text.contains(&base_url),
        "no debe contener URLs internas desanitizadas"
    );
}

#[tokio::test]
async fn test_moodle_sync_course_network_error_never_leaks_token() {
    let mock = MockServer::start().await;
    let base_url = mock.uri();
    let secret_token = "SECRET_SYNC_TOKEN_ABC123";
    let client = common::start_server_with_options(&mock, Some(secret_token), None).await;

    let tree = json!([
        {
            "id": 1,
            "section": 1,
            "name": "Seccion Con Fallo",
            "modules": [
                {
                    "id": 10,
                    "modname": "resource",
                    "name": "Recurso Roto",
                    "contents": [
                        {
                            "type": "file",
                            "filename": "broken.pdf",
                            "filepath": "/",
                            "filesize": 100,
                            "fileurl": format!("{base_url}/pluginfile.php/1/broken.pdf"),
                            "timemodified": 1000
                        }
                    ]
                }
            ]
        }
    ]);

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&tree))
        .mount(&mock)
        .await;

    // El archivo falla con 500
    Mock::given(method("GET"))
        .and(path("/webservice/pluginfile.php/1/broken.pdf"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock)
        .await;

    let temp = tempdir().expect("tempdir");
    let dest_path = temp
        .path()
        .join("curso_leak_test")
        .to_string_lossy()
        .into_owned();

    let args = json!({
        "courseid": 99,
        "dest": dest_path
    })
    .as_object()
    .unwrap()
    .clone();

    let res = client
        .call_tool(CallToolRequestParams::new("moodle_sync_course").with_arguments(args))
        .await
        .expect("call moodle_sync_course");

    let text = res.content[0].as_text().expect("texto").text.clone();
    assert!(
        !text.contains(secret_token),
        "La respuesta MCP no debe contener el token en texto ni en failed[].error: {text}"
    );

    let val: serde_json::Value = serde_json::from_str(&text).unwrap();
    let failed = val["failed"].as_array().expect("failed array");
    assert_eq!(failed.len(), 1);
    let failed_err = failed[0]["error"].as_str().expect("error string");
    assert!(
        !failed_err.contains(secret_token),
        "failed[].error no debe contener el token: {failed_err}"
    );
}
