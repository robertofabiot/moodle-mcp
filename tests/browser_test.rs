mod common;

use common::{start_server, start_server_with_options};
use moodle_mcp::config::Config;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{RoleClient, RunningService};
use serde_json::{json, Value};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn browser_link(client: &RunningService<RoleClient, ()>, args: Value) -> (bool, Value) {
    let params = CallToolRequestParams::new("moodle_open_in_browser")
        .with_arguments(args.as_object().unwrap().clone());
    let result = client.call_tool(params).await.unwrap();
    let text = result.content[0].as_text().unwrap();
    (
        result.is_error.unwrap_or(false),
        serde_json::from_str(&text.text).unwrap(),
    )
}

#[tokio::test]
async fn test_browser_link_requires_only_a_url_within_the_configured_site() {
    let mock = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let base_url = format!("{}/grado", mock.uri());
    let client = start_server_with_options(
        &mock,
        None,
        Some(Config {
            base_url: base_url.clone(),
            download_dir: dir.path().to_owned(),
            max_response_bytes: 60_000,
            max_inline_image_bytes: 1_000_000,
        }),
    )
    .await;

    let relative_url = "mod/lti/view.php?id=7#actividad";
    let full_url = format!("{base_url}/{relative_url}");
    for url in [&full_url, relative_url] {
        let (is_error, value) = browser_link(&client, json!({"url": url})).await;
        assert!(!is_error);
        assert_eq!(value["url"], full_url);
        assert!(value["login_hint"]
            .as_str()
            .unwrap()
            .contains("inicia sesión"));
    }

    let external = MockServer::start().await;
    for url in [
        "javascript:alert(1)".into(),
        format!("{}/grado/mod/lti/view.php?id=7", external.uri()),
        format!("{}/grado-externo/view.php?id=7", mock.uri()),
        format!("{base_url}/mod/lti/view.php?wstoken=secreto"),
        format!("{base_url}/admin/tool/mobile/autologin.php?key=secreto"),
        format!("{base_url}/tokenpluginfile.php/secreto/course/intro.pdf"),
        format!("http://usuario:secreto@{}/grado/view.php", mock.address()),
    ] {
        let (is_error, value) = browser_link(&client, json!({"url": url})).await;
        assert!(is_error);
        assert_eq!(value["error"]["kind"], "invalid_arguments");
        assert!(!value.to_string().contains("secreto"));
    }

    for args in [
        json!({}),
        json!({"url": " "}),
        json!({"cmid": 0}),
        json!({"url": full_url, "cmid": 7}),
    ] {
        let (is_error, value) = browser_link(&client, args).await;
        assert!(is_error);
        assert_eq!(value["error"]["kind"], "invalid_arguments");
    }

    let (is_error, value) = browser_link(&client, json!({"cmid": 7})).await;
    assert!(is_error);
    assert_eq!(value["error"]["kind"], "no_session");
    assert!(mock.received_requests().await.unwrap().is_empty());
    assert!(external.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_browser_link_resolves_cmid_without_requesting_a_browser_session() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_course_get_course_module",
        ))
        .and(body_string_contains("cmid=7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "cm": {"id": 7, "instance": 20, "course": 3, "modname": "lti", "name": "Actividad"}
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;
    let tools = client.list_all_tools().await.unwrap();
    let tool = tools
        .iter()
        .find(|tool| tool.name == "moodle_open_in_browser")
        .unwrap();
    assert_eq!(
        tool.annotations.as_ref().unwrap().read_only_hint,
        Some(true)
    );

    let (is_error, value) = browser_link(&client, json!({"cmid": 7})).await;
    assert!(!is_error);
    assert_eq!(
        value["url"],
        format!("{}/mod/lti/view.php?id=7", mock.uri())
    );
    assert_eq!(mock.received_requests().await.unwrap().len(), 1);
}
