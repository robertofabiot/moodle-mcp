mod common;

use common::{start_server, start_server_with_options};
use moodle_mcp::config::Config;
use moodle_mcp::mcp::output::{ToolErrorEnvelope, TruncatedResponse};
use moodle_mcp::mcp::AppContext;
use rmcp::model::CallToolRequestParams;
use rmcp::ServiceExt;
use serde_json::Value;
use std::collections::HashMap;
use tempfile::tempdir;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURE_SITE_INFO: &str = include_str!("fixtures/site_info.json");
const FIXTURE_USER_COURSES: &str = include_str!("fixtures/user_courses.json");

async fn mock_site_info(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_webservice_get_site_info",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_SITE_INFO))
        .mount(server)
        .await;
}

#[tokio::test]
async fn test_tools_list_returns_five_tools_with_exact_annotations() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    let tools = client.list_all_tools().await.expect("list_all_tools");
    assert!(tools.len() >= 5, "Debe exponer al menos 5 herramientas");

    let tool_map: HashMap<String, _> = tools.into_iter().map(|t| (t.name.to_string(), t)).collect();

    // 1. moodle_whoami: read_only_hint = true
    let whoami = tool_map
        .get("moodle_whoami")
        .expect("moodle_whoami tool must exist");
    let ann = whoami.annotations.as_ref().expect("annotations");
    assert_eq!(ann.read_only_hint, Some(true));
    assert_ne!(ann.destructive_hint, Some(true));
    assert_ne!(ann.open_world_hint, Some(true));

    // 2. moodle_list_functions: read_only_hint = true
    let list_fn = tool_map
        .get("moodle_list_functions")
        .expect("moodle_list_functions tool must exist");
    let ann = list_fn.annotations.as_ref().expect("annotations");
    assert_eq!(ann.read_only_hint, Some(true));
    assert_ne!(ann.destructive_hint, Some(true));

    // 3. moodle_describe_function: read_only_hint = true
    let desc_fn = tool_map
        .get("moodle_describe_function")
        .expect("moodle_describe_function tool must exist");
    let ann = desc_fn.annotations.as_ref().expect("annotations");
    assert_eq!(ann.read_only_hint, Some(true));
    assert_ne!(ann.destructive_hint, Some(true));

    // 4. moodle_read: read_only_hint = true, open_world_hint = true
    let read_fn = tool_map
        .get("moodle_read")
        .expect("moodle_read tool must exist");
    let ann = read_fn.annotations.as_ref().expect("annotations");
    assert_eq!(ann.read_only_hint, Some(true));
    assert_eq!(ann.open_world_hint, Some(true));
    assert_ne!(ann.destructive_hint, Some(true));

    // 5. moodle_call: destructive_hint = true, open_world_hint = true
    let call_fn = tool_map
        .get("moodle_call")
        .expect("moodle_call tool must exist");
    let ann = call_fn.annotations.as_ref().expect("annotations");
    assert_eq!(ann.destructive_hint, Some(true));
    assert_eq!(ann.open_world_hint, Some(true));
}

#[tokio::test]
async fn test_moodle_call_success_with_fixture() {
    let mock = MockServer::start().await;
    mock_site_info(&mock).await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_enrol_get_users_courses",
        ))
        .and(body_string_contains("userid=10001"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_USER_COURSES))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let args = serde_json::json!({
        "function": "core_enrol_get_users_courses",
        "params": { "userid": 10001 }
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_call").with_arguments(args);
    let result = client.call_tool(params).await.expect("call_tool");

    assert!(!result.is_error.unwrap_or(false));
    assert!(!result.content.is_empty());
    let text = result.content[0].as_text().unwrap();
    assert!(text.text.contains("ESTADISTICA I"));
}

#[tokio::test]
async fn test_moodle_read_rejects_write_function_with_invalid_arguments() {
    let mock = MockServer::start().await;
    mock_site_info(&mock).await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_save_submission",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(0)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let args = serde_json::json!({
        "function": "mod_assign_save_submission",
        "params": { "assignmentid": 1 }
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_read").with_arguments(args);
    let result = client.call_tool(params).await.expect("call_tool");

    assert_eq!(result.is_error, Some(true));
    let text = result.content[0].as_text().unwrap();
    let env: ToolErrorEnvelope = serde_json::from_str(&text.text).unwrap();
    assert_eq!(env.error.kind, "invalid_arguments");
    assert!(env.error.message.contains("no es de solo lectura"));
    assert!(env.error.hint.is_some());
}

#[tokio::test]
async fn test_function_not_in_list_returns_not_enabled() {
    let mock = MockServer::start().await;
    mock_site_info(&mock).await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains("wsfunction=core_nonexistent_function"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(0)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let args = serde_json::json!({
        "function": "core_nonexistent_function"
    })
    .as_object()
    .unwrap()
    .clone();

    // Probar con moodle_read
    let params = CallToolRequestParams::new("moodle_read").with_arguments(args.clone());
    let result = client.call_tool(params).await.expect("call_tool");
    assert_eq!(result.is_error, Some(true));
    let text = result.content[0].as_text().unwrap();
    let env: ToolErrorEnvelope = serde_json::from_str(&text.text).unwrap();
    assert_eq!(env.error.kind, "not_enabled");
    assert!(env.error.message.contains("no está habilitada"));

    // Probar con moodle_call
    let params = CallToolRequestParams::new("moodle_call").with_arguments(args);
    let result = client.call_tool(params).await.expect("call_tool");
    assert_eq!(result.is_error, Some(true));
    let text = result.content[0].as_text().unwrap();
    let env: ToolErrorEnvelope = serde_json::from_str(&text.text).unwrap();
    assert_eq!(env.error.kind, "not_enabled");
}

#[tokio::test]
async fn test_moodle_call_with_invalidparameter_includes_debuginfo() {
    let mock = MockServer::start().await;
    mock_site_info(&mock).await;

    let error_response = serde_json::json!({
        "exception": "invalid_parameter_exception",
        "errorcode": "invalidparameter",
        "message": "Detectado valor de parámetro no válido",
        "debuginfo": "Missing required key in courseids: 0"
    });

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_enrol_get_users_courses",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(error_response))
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let args = serde_json::json!({
        "function": "core_enrol_get_users_courses"
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_call").with_arguments(args);
    let result = client.call_tool(params).await.expect("call_tool");

    assert_eq!(result.is_error, Some(true));
    let text = result.content[0].as_text().unwrap();
    let env: ToolErrorEnvelope = serde_json::from_str(&text.text).unwrap();
    assert_eq!(env.error.kind, "api");
    assert_eq!(env.error.errorcode.as_deref(), Some("invalidparameter"));
    assert_eq!(
        env.error.debuginfo.as_deref(),
        Some("Missing required key in courseids: 0")
    );
    assert_eq!(env.error.message, "Detectado valor de parámetro no válido");
}

#[tokio::test]
async fn test_sanitization_scrubs_token_and_private_token() {
    let mock = MockServer::start().await;
    mock_site_info(&mock).await;

    let leaky_response = serde_json::json!({
        "status": "ok",
        "exposed_token": "tok123",
        "exposed_private": "priv123"
    });

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_enrol_get_users_courses",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(leaky_response))
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let args = serde_json::json!({
        "function": "core_enrol_get_users_courses"
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_call").with_arguments(args);
    let result = client.call_tool(params).await.expect("call_tool");

    let text = &result.content[0].as_text().unwrap().text;
    assert!(!text.contains("tok123"), "El wstoken no debe filtrarse");
    assert!(
        !text.contains("priv123"),
        "El privatetoken no debe filtrarse"
    );
    assert!(text.contains("<token>"));
}

#[tokio::test]
async fn test_response_exceeding_max_bytes_truncates_and_saves_to_file() {
    let mock = MockServer::start().await;
    mock_site_info(&mock).await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_enrol_get_users_courses",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_USER_COURSES))
        .mount(&mock)
        .await;

    let dir = tempdir().unwrap();
    let config = Config {
        base_url: mock.uri(),
        download_dir: dir.path().to_path_buf(),
        max_response_bytes: 100,
        max_inline_image_bytes: 1_000_000,
    };

    let client = start_server_with_options(&mock, Some("tok123"), Some(config)).await;

    let args = serde_json::json!({
        "function": "core_enrol_get_users_courses"
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_call").with_arguments(args);
    let result = client.call_tool(params).await.expect("call_tool");

    assert!(!result.is_error.unwrap_or(false));
    let text = &result.content[0].as_text().unwrap().text;
    let truncated: TruncatedResponse = serde_json::from_str(text).unwrap();
    assert!(truncated.truncated);
    assert!(truncated.preview.len() <= 100);

    let saved_path = std::path::Path::new(&truncated.saved_to);
    assert!(
        saved_path.exists(),
        "El archivo guardado debe existir en disco"
    );

    let saved_content = std::fs::read_to_string(saved_path).unwrap();
    let parsed_saved: Value = serde_json::from_str(&saved_content).unwrap();
    let expected: Value = serde_json::from_str(FIXTURE_USER_COURSES).unwrap();
    assert_eq!(parsed_saved, expected);
}

#[tokio::test]
async fn test_no_session_returns_no_session_error() {
    let mock = MockServer::start().await;
    let client = start_server_with_options(&mock, None, None).await;

    let tools = [
        ("moodle_whoami", serde_json::json!({})),
        (
            "moodle_list_functions",
            serde_json::json!({ "filter": "course" }),
        ),
        (
            "moodle_describe_function",
            serde_json::json!({ "name": "core_course_get_courses" }),
        ),
        (
            "moodle_read",
            serde_json::json!({ "function": "core_course_get_courses" }),
        ),
        (
            "moodle_call",
            serde_json::json!({ "function": "core_course_get_courses" }),
        ),
    ];

    for (tool_name, args_val) in tools {
        let args = args_val.as_object().unwrap().clone();
        let params = CallToolRequestParams::new(tool_name).with_arguments(args);
        let result = client.call_tool(params).await.expect("call_tool");

        assert_eq!(
            result.is_error,
            Some(true),
            "Herramienta {} debe fallar sin sesión",
            tool_name
        );
        let text = result.content[0].as_text().unwrap();
        let env: ToolErrorEnvelope = serde_json::from_str(&text.text).unwrap();
        assert_eq!(env.error.kind, "no_session");
        assert_eq!(
            env.error.hint.as_deref(),
            Some("Ejecuta 'moodle-mcp login' en una terminal; no hace falta reiniciar el servidor")
        );
    }
}

#[tokio::test]
async fn test_write_function_with_503_performs_exactly_one_request() {
    let mock = MockServer::start().await;
    mock_site_info(&mock).await;

    // mod_assign_save_submission es función de escritura
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_save_submission",
        ))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let args = serde_json::json!({
        "function": "mod_assign_save_submission",
        "params": { "assignmentid": 10 }
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_call").with_arguments(args);
    let result = client.call_tool(params).await.expect("call_tool");

    assert_eq!(result.is_error, Some(true));
    let text = result.content[0].as_text().unwrap();
    let env: ToolErrorEnvelope = serde_json::from_str(&text.text).unwrap();
    assert_eq!(env.error.kind, "maintenance");
}

#[tokio::test]
async fn test_whoami_returns_site_and_user_info_with_function_count() {
    let mock = MockServer::start().await;
    mock_site_info(&mock).await;

    let client = start_server(&mock).await;

    let params = CallToolRequestParams::new("moodle_whoami");
    let result = client.call_tool(params).await.expect("moodle_whoami");

    assert!(!result.is_error.unwrap_or(false));
    let text = result.content[0].as_text().unwrap();
    let val: Value = serde_json::from_str(&text.text).unwrap();

    assert_eq!(val["userid"], 10001);
    assert_eq!(val["fullname"], "ESTUDIANTE PRUEBA");
    assert_eq!(val["sitename"], "UAM Virtual - Grado");
    assert!(val.get("functions").is_none());
    assert!(val["function_count"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn test_list_functions_filtering() {
    let mock = MockServer::start().await;
    mock_site_info(&mock).await;

    let client = start_server(&mock).await;

    let args = serde_json::json!({
        "filter": "users_courses"
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_list_functions").with_arguments(args);
    let result = client.call_tool(params).await.expect("list_functions");

    assert!(!result.is_error.unwrap_or(false));
    let text = result.content[0].as_text().unwrap();
    let items: Vec<Value> = serde_json::from_str(&text.text).unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], "core_enrol_get_users_courses");
    assert_eq!(items[0]["read_only"], true);
}

#[tokio::test]
async fn test_describe_function_output() {
    let mock = MockServer::start().await;
    mock_site_info(&mock).await;

    let client = start_server(&mock).await;

    let args = serde_json::json!({
        "name": "core_enrol_get_users_courses"
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_describe_function").with_arguments(args);
    let result = client.call_tool(params).await.expect("describe_function");

    assert!(!result.is_error.unwrap_or(false));
    let text = result.content[0].as_text().unwrap();
    let val: Value = serde_json::from_str(&text.text).unwrap();

    assert_eq!(val["name"], "core_enrol_get_users_courses");
    assert_eq!(val["enabled"], true);
    assert_eq!(val["read_only"], true);
    assert!(val["signature"].is_null());
    assert!(val["hint"].is_string());
}

#[tokio::test]
async fn test_server_starts_without_session_and_reloads_after_login_without_restart() {
    let mock = MockServer::start().await;
    mock_site_info(&mock).await;

    let tctx = common::TestContext::with_options(&mock, None, None).await;

    // 1. Llamada inicial sin sesión retorna error no_session
    let params = CallToolRequestParams::new("moodle_whoami");
    let result = tctx
        .client
        .call_tool(params.clone())
        .await
        .expect("call_tool");
    assert_eq!(result.is_error, Some(true));
    let text = result.content[0].as_text().unwrap();
    let env: ToolErrorEnvelope = serde_json::from_str(&text.text).unwrap();
    assert_eq!(env.error.kind, "no_session");

    // 2. Simular login guardando credenciales en el archivo del almacén
    let creds = moodle_mcp::credentials::Credentials {
        base_url: mock.uri(),
        token: "tok_after_login".to_string(),
        private_token: None,
        userid: 10001,
        username: "user_login".to_string(),
        fullname: "ESTUDIANTE PRUEBA".to_string(),
    };
    tctx.store.save(&creds).await.expect("save credentials");

    // 3. Siguiente llamada tiene éxito sin reiniciar el servidor
    let result2 = tctx.client.call_tool(params).await.expect("call_tool");
    assert!(!result2.is_error.unwrap_or(false));
    let text2 = result2.content[0].as_text().unwrap();
    let val: Value = serde_json::from_str(&text2.text).unwrap();
    assert_eq!(val["userid"], 10001);
}

#[tokio::test]
async fn test_invalid_token_invalidates_session_and_reloads_on_next_call() {
    let mock = MockServer::start().await;

    // Mock que rechaza tok_bad con invalidtoken
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(wiremock::matchers::body_string_contains("wstoken=tok_bad"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "exception": "moodle_exception",
            "errorcode": "invalidtoken",
            "message": "Token no válido"
        })))
        .mount(&mock)
        .await;

    // Mock que acepta tok_renewed con 200 OK
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(wiremock::matchers::body_string_contains(
            "wstoken=tok_renewed",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_SITE_INFO))
        .mount(&mock)
        .await;

    let tctx = common::TestContext::with_options(&mock, Some("tok_bad"), None).await;

    // 1. Llamada falla con invalid_token e invalida la sesión
    let params = CallToolRequestParams::new("moodle_whoami");
    let result = tctx
        .client
        .call_tool(params.clone())
        .await
        .expect("call_tool");
    assert_eq!(result.is_error, Some(true));
    let text = result.content[0].as_text().unwrap();
    let env: ToolErrorEnvelope = serde_json::from_str(&text.text).unwrap();
    assert_eq!(env.error.kind, "invalid_token");

    // 2. Simular login guardando nuevo token en almacén
    let creds = moodle_mcp::credentials::Credentials {
        base_url: mock.uri(),
        token: "tok_renewed".to_string(),
        private_token: None,
        userid: 10001,
        username: "user_login".to_string(),
        fullname: "ESTUDIANTE PRUEBA".to_string(),
    };
    tctx.store.save(&creds).await.expect("save credentials");

    // 3. Siguiente llamada carga el nuevo token y tiene éxito
    let result2 = tctx.client.call_tool(params).await.expect("call_tool");
    assert!(!result2.is_error.unwrap_or(false));
    let text2 = result2.content[0].as_text().unwrap();
    let val: Value = serde_json::from_str(&text2.text).unwrap();
    assert_eq!(val["userid"], 10001);
}

#[tokio::test]
async fn test_whoami_twice_generates_two_http_requests() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(wiremock::matchers::body_string_contains(
            "wsfunction=core_webservice_get_site_info",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_SITE_INFO))
        .expect(2)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let params1 = CallToolRequestParams::new("moodle_whoami");
    let r1 = client.call_tool(params1).await.expect("moodle_whoami 1");
    assert!(!r1.is_error.unwrap_or(false));

    let params2 = CallToolRequestParams::new("moodle_whoami");
    let r2 = client.call_tool(params2).await.expect("moodle_whoami 2");
    assert!(!r2.is_error.unwrap_or(false));
}

#[tokio::test]
async fn test_corrupt_file_store_returns_io_error_and_server_stays_alive() {
    let mock = MockServer::start().await;
    mock_site_info(&mock).await;

    let tctx = common::TestContext::with_options(&mock, None, None).await;

    // Escribir contenido corrupto (no JSON) en el archivo de credenciales
    std::fs::write(&tctx.store_file, "INVALID_CORRUPTED_JSON_CONTENT{{{")
        .expect("write corrupt file");

    let params = CallToolRequestParams::new("moodle_whoami");
    let result = tctx
        .client
        .call_tool(params.clone())
        .await
        .expect("call_tool");
    assert_eq!(result.is_error, Some(true));
    let text = result.content[0].as_text().unwrap();
    let env: ToolErrorEnvelope = serde_json::from_str(&text.text).unwrap();
    assert_eq!(env.error.kind, "io", "Debe retornar error tipado io");

    // Reparar el archivo con credenciales válidas; el servidor sigue vivo y responde
    let creds = moodle_mcp::credentials::Credentials {
        base_url: mock.uri(),
        token: "tok123".to_string(),
        private_token: None,
        userid: 10001,
        username: "user".to_string(),
        fullname: "ESTUDIANTE PRUEBA".to_string(),
    };
    tctx.store.save(&creds).await.expect("save credentials");

    let result2 = tctx.client.call_tool(params).await.expect("call_tool");
    assert!(!result2.is_error.unwrap_or(false));
}

#[tokio::test]
async fn test_truncation_with_tokens_scrubs_both_preview_and_saved_file() {
    let mock = MockServer::start().await;
    mock_site_info(&mock).await;

    let secret_tok = "tok_super_secret_12345";
    let secret_priv = "priv_super_secret_67890";

    let leaky_large_response = serde_json::json!({
        "status": "ok",
        "auth_token": secret_tok,
        "private_token": secret_priv,
        "items": vec![format!("item_data_{secret_tok}_{secret_priv}"); 20]
    });

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(wiremock::matchers::body_string_contains(
            "wsfunction=core_enrol_get_users_courses",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(leaky_large_response))
        .mount(&mock)
        .await;

    let dir = tempdir().unwrap();
    let config = Config {
        base_url: mock.uri(),
        download_dir: dir.path().to_path_buf(),
        max_response_bytes: 80,
        max_inline_image_bytes: 1_000_000,
    };

    let creds = moodle_mcp::credentials::Credentials {
        base_url: mock.uri(),
        token: secret_tok.to_string(),
        private_token: Some(secret_priv.to_string()),
        userid: 10001,
        username: "test_user".to_string(),
        fullname: "ESTUDIANTE PRUEBA".to_string(),
    };

    let store_file = dir.path().join("creds.json");
    let store = moodle_mcp::credentials::CredentialStore::with_file(store_file);
    store.save(&creds).await.unwrap();

    let gate = moodle_mcp::moodle::gate::MoodleGate::new(4, 3).without_jitter();
    let client = moodle_mcp::moodle::client::MoodleClient::from_builder(
        moodle_mcp::moodle::client::MoodleClientBuilder::new()
            .base_url(mock.uri())
            .gate(gate),
    );

    let ctx = std::sync::Arc::new(AppContext::new(config, client, store));
    let server = moodle_mcp::mcp::MoodleMcp::new(ctx);

    let (client_io, server_io) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });

    let mcp_client = ().serve(client_io).await.unwrap();

    let args = serde_json::json!({
        "function": "core_enrol_get_users_courses"
    })
    .as_object()
    .unwrap()
    .clone();

    let params = CallToolRequestParams::new("moodle_call").with_arguments(args);
    let result = mcp_client.call_tool(params).await.expect("call_tool");

    assert!(!result.is_error.unwrap_or(false));
    let text = &result.content[0].as_text().unwrap().text;
    let truncated: TruncatedResponse = serde_json::from_str(text).unwrap();
    assert!(truncated.truncated);

    // Ni el preview ni el archivo guardado deben contener ninguno de los secretos
    assert!(!truncated.preview.contains(secret_tok));
    assert!(!truncated.preview.contains(secret_priv));
    assert!(truncated.preview.contains("<token>"));

    let saved_content = std::fs::read_to_string(&truncated.saved_to).unwrap();
    assert!(!saved_content.contains(secret_tok));
    assert!(!saved_content.contains(secret_priv));
    assert!(saved_content.contains("<token>"));
}
