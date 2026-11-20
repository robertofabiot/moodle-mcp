mod common;

use common::{start_server, start_server_with_options};
use moodle_mcp::config::Config;
use moodle_mcp::mcp::output::{ToolErrorEnvelope, TruncatedResponse};
use rmcp::model::CallToolRequestParams;
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
    assert_eq!(tools.len(), 5, "Debe exponer exactamente 5 herramientas");

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
            Some("Ejecuta 'moodle-mcp login' en una terminal")
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
