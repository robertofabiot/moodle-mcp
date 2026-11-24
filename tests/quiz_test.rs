mod common;

use common::start_server;
use moodle_mcp::config::Config;
use moodle_mcp::credentials::{CredentialStore, Credentials};
use moodle_mcp::mcp::{AppContext, MoodleMcp};
use moodle_mcp::moodle::client::{MoodleClient, MoodleClientBuilder};
use moodle_mcp::moodle::gate::MoodleGate;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{RoleClient, RunningService};
use rmcp::ServiceExt;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURE_QUIZ_LIST: &str = include_str!("fixtures/quiz_list.json");
const FIXTURE_QUIZ_ACCESS_INFO: &str = include_str!("fixtures/quiz_access_info.json");
const FIXTURE_QUIZ_ATTEMPTS: &str = include_str!("fixtures/quiz_attempts.json");
const FIXTURE_QUIZ_REVIEW: &str = include_str!("fixtures/quiz_review.json");

fn cmid_response(cmid: i64, instance: i64, course: i64, modname: &str, name: &str) -> String {
    json!({
        "cm": {
            "id": cmid,
            "instance": instance,
            "course": course,
            "modname": modname,
            "name": name,
        }
    })
    .to_string()
}

async fn start_server_and_ctx(
    mock: &MockServer,
) -> (RunningService<RoleClient, ()>, Arc<AppContext>) {
    let base_url = mock.uri();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let download_dir = temp_dir.path().join("downloads");
    let store_file = temp_dir.path().join("credentials.json");

    let config = Config {
        base_url: base_url.clone(),
        download_dir,
        max_response_bytes: 60_000,
        max_inline_image_bytes: 1_000_000,
    };

    let credentials = Some(Credentials {
        base_url: base_url.clone(),
        token: "tok123".to_string(),
        private_token: Some("priv123".to_string()),
        userid: 10001,
        username: "24000000".to_string(),
        fullname: "Estudiante Prueba".to_string(),
    });

    let gate = MoodleGate::new(4, 3).without_jitter();
    let client =
        MoodleClient::from_builder(MoodleClientBuilder::new().base_url(base_url).gate(gate));

    let store = CredentialStore::with_file(store_file);
    let ctx = Arc::new(
        AppContext::new_with_session(config, client, store, credentials)
            .with_env_token_reader(Arc::new(|| (None, None))),
    );
    let server = MoodleMcp::new(ctx.clone());

    let (client_io, server_io) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move {
        let _keep_alive = temp_dir;
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });

    let client = ().serve(client_io).await.expect("Fallo al conectar cliente duplex a MoodleMcp");

    (client, ctx)
}

#[tokio::test]
async fn test_quiz_tools_annotations() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    let tools = client.list_all_tools().await.expect("list_all_tools");
    let tool_map: HashMap<String, _> = tools.into_iter().map(|t| (t.name.to_string(), t)).collect();

    // 1. quiz_info: read_only_hint = true
    let info = tool_map
        .get("quiz_info")
        .expect("quiz_info debe estar registrada");
    let info_ann = info.annotations.as_ref().expect("annotations de quiz_info");
    assert_eq!(info_ann.read_only_hint, Some(true));

    // 2. quiz_review: read_only_hint = true
    let review = tool_map
        .get("quiz_review")
        .expect("quiz_review debe estar registrada");
    let review_ann = review
        .annotations
        .as_ref()
        .expect("annotations de quiz_review");
    assert_eq!(review_ann.read_only_hint, Some(true));

    // 3. quiz_start: destructive_hint = true
    let start = tool_map
        .get("quiz_start")
        .expect("quiz_start debe estar registrada");
    let start_ann = start
        .annotations
        .as_ref()
        .expect("annotations de quiz_start");
    assert_eq!(start_ann.destructive_hint, Some(true));

    // 4. quiz_get_page: read_only_hint = true
    let get_page = tool_map
        .get("quiz_get_page")
        .expect("quiz_get_page debe estar registrada");
    let get_page_ann = get_page
        .annotations
        .as_ref()
        .expect("annotations de quiz_get_page");
    assert_eq!(get_page_ann.read_only_hint, Some(true));

    // 5. quiz_save: destructive_hint = false
    let save = tool_map
        .get("quiz_save")
        .expect("quiz_save debe estar registrada");
    let save_ann = save.annotations.as_ref().expect("annotations de quiz_save");
    assert_eq!(save_ann.destructive_hint, Some(false));

    // 6. quiz_finish: destructive_hint = true
    let finish = tool_map
        .get("quiz_finish")
        .expect("quiz_finish debe estar registrada");
    let finish_ann = finish
        .annotations
        .as_ref()
        .expect("annotations de quiz_finish");
    assert_eq!(finish_ann.destructive_hint, Some(true));
}

#[tokio::test]
async fn test_quiz_info_by_cmid_and_by_quizid_identical() {
    let mock = MockServer::start().await;

    // 1. Resolver por cmid
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_course_get_course_module",
        ))
        .and(body_string_contains("cmid=979333"))
        .respond_with(ResponseTemplate::new(200).set_body_string(cmid_response(
            979333,
            68822,
            8233,
            "quiz",
            "Cuestionario de Evaluación 1",
        )))
        .mount(&mock)
        .await;

    // 2. Resolver por quizid (instance)
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_course_get_course_module_by_instance",
        ))
        .and(body_string_contains("module=quiz"))
        .and(body_string_contains("instance=68822"))
        .respond_with(ResponseTemplate::new(200).set_body_string(cmid_response(
            979333,
            68822,
            8233,
            "quiz",
            "Cuestionario de Evaluación 1",
        )))
        .mount(&mock)
        .await;

    // 3. mod_quiz_get_quizzes_by_courses
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_quiz_get_quizzes_by_courses",
        ))
        .and(body_string_contains("8233"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_QUIZ_LIST))
        .mount(&mock)
        .await;

    // 4. mod_quiz_get_quiz_access_information
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_quiz_get_quiz_access_information",
        ))
        .and(body_string_contains("quizid=68822"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_QUIZ_ACCESS_INFO))
        .mount(&mock)
        .await;

    // 5. mod_quiz_get_user_attempts
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_quiz_get_user_attempts",
        ))
        .and(body_string_contains("quizid=68822"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_QUIZ_ATTEMPTS))
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    // Llamada por cmid
    let res_cmid = client
        .call_tool(
            CallToolRequestParams::new("quiz_info")
                .with_arguments(json!({ "cmid": 979333 }).as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool por cmid");
    assert!(!res_cmid.is_error.unwrap_or(false));
    let text_cmid = res_cmid.content[0].as_text().unwrap();
    let val_cmid: serde_json::Value = serde_json::from_str(&text_cmid.text).unwrap();

    // Llamada por quizid
    let res_quizid = client
        .call_tool(
            CallToolRequestParams::new("quiz_info")
                .with_arguments(json!({ "quizid": 68822 }).as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool por quizid");
    assert!(!res_quizid.is_error.unwrap_or(false));
    let text_quizid = res_quizid.content[0].as_text().unwrap();
    let val_quizid: serde_json::Value = serde_json::from_str(&text_quizid.text).unwrap();

    // Validar identidad entre ambas respuestas
    assert_eq!(val_cmid, val_quizid);

    // Validar campos retornados
    assert_eq!(val_cmid["cmid"], 979333);
    assert_eq!(val_cmid["instance"], 68822);
    assert_eq!(val_cmid["courseid"], 8233);
    assert_eq!(val_cmid["name"], "Cuestionario de Evaluación 1");
    assert!(val_cmid["intro_markdown"]
        .as_str()
        .unwrap()
        .contains("Bienvenido al cuestionario"));
    assert_eq!(val_cmid["timeopen"], 1700000000);
    assert_eq!(val_cmid["timeclose"], 1700003600);
    assert_eq!(val_cmid["timelimit"], 3600);
    assert_eq!(val_cmid["attempts"], 2);
    assert_eq!(val_cmid["grademethod"], 1);
    assert_eq!(val_cmid["requires_seb"], true);
    assert_eq!(val_cmid["requires_password"], true);

    let attempts = val_cmid["attempts_history"].as_array().unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0]["attemptid"], 101);
    assert_eq!(attempts[0]["state"], "finished");
    assert_eq!(attempts[0]["sumgrades"], 9.5);
    assert_eq!(attempts[0]["timestart"], 1700000100);
    assert_eq!(attempts[0]["timefinish"], 1700001800);
}

#[tokio::test]
async fn test_quiz_info_both_or_neither_cmid_quizid_returns_invalid_arguments() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    // Caso 1: Ambos suministrados
    let res_both = client
        .call_tool(
            CallToolRequestParams::new("quiz_info").with_arguments(
                json!({ "cmid": 979333, "quizid": 68822 })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("call_tool con ambos");
    assert!(res_both.is_error.unwrap_or(false));
    let err_both: serde_json::Value =
        serde_json::from_str(&res_both.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(err_both["error"]["kind"], "invalid_arguments");

    // Caso 2: Ninguno suministrado
    let res_none = client
        .call_tool(
            CallToolRequestParams::new("quiz_info")
                .with_arguments(json!({}).as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool sin argumentos");
    assert!(res_none.is_error.unwrap_or(false));
    let err_none: serde_json::Value =
        serde_json::from_str(&res_none.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(err_none["error"]["kind"], "invalid_arguments");
}

#[tokio::test]
async fn test_quiz_info_seb_and_password_false_when_absent() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_course_get_course_module",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(cmid_response(
            979333,
            68822,
            8233,
            "quiz",
            "Cuestionario Abierto",
        )))
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_quiz_get_quizzes_by_courses",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_QUIZ_LIST))
        .mount(&mock)
        .await;

    // Reglas de acceso sin SEB ni contraseña
    let access_info_no_seb = json!({
        "accessrules": [],
        "preventaccessreasons": [],
        "activerulenames": ["quizaccess_numattempts"],
        "warnings": []
    });

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_quiz_get_quiz_access_information",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(access_info_no_seb.to_string()))
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_quiz_get_user_attempts",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_QUIZ_ATTEMPTS))
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("quiz_info")
                .with_arguments(json!({ "cmid": 979333 }).as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool quiz_info");

    assert!(!res.is_error.unwrap_or(false));
    let val: serde_json::Value =
        serde_json::from_str(&res.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(val["requires_seb"], false);
    assert_eq!(val["requires_password"], false);
}

#[tokio::test]
async fn test_quiz_review_success() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_quiz_get_attempt_review",
        ))
        .and(body_string_contains("attemptid=101"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_QUIZ_REVIEW))
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("quiz_review")
                .with_arguments(json!({ "attemptid": 101 }).as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool quiz_review");

    assert!(!res.is_error.unwrap_or(false));
    let val: serde_json::Value =
        serde_json::from_str(&res.content[0].as_text().unwrap().text).unwrap();

    assert_eq!(val["attemptid"], 101);
    assert_eq!(val["state"], "finished");
    assert_eq!(val["grade"], 9.5);

    let questions = val["questions"].as_array().unwrap();
    assert_eq!(questions.len(), 1);
    let q = &questions[0];
    assert_eq!(q["slot"], 1);
    assert_eq!(q["number"], "1");
    assert_eq!(q["type"], "multichoice");
    assert_eq!(q["status"], "Correcta");
    assert_eq!(q["state"], "gradedright");
    assert_eq!(q["mark"], "1.00");
    assert_eq!(q["maxmark"], "1.00000");
    assert!(q["text_markdown"]
        .as_str()
        .unwrap()
        .contains("capital de Nicaragua"));

    let inputs = q["inputs"].as_array().unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0]["name"], "q101:1_answer");
    assert_eq!(inputs[0]["kind"], "single");
    assert_eq!(inputs[0]["current"], "1");
    let opts = inputs[0]["options"].as_array().unwrap();
    assert_eq!(opts.len(), 2);
    assert_eq!(opts[0]["value"], "0");
    assert_eq!(opts[0]["label"], "a. San José");
    assert_eq!(opts[1]["value"], "1");
    assert_eq!(opts[1]["label"], "b. Managua");

    assert_eq!(q["feedback_markdown"], "Respuesta correcta");
    assert_eq!(q["hidden_fields"]["q101:1_:sequencecheck"], "1");
}

#[tokio::test]
async fn test_quiz_scrubs_token_in_error() {
    let mock = MockServer::start().await;

    let error_response = json!({
        "exception": "moodle_exception",
        "errorcode": "invalidparameter",
        "message": "Parámetro inválido",
        "debuginfo": "Falló validación con token tok123 y sesión activa"
    });

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_quiz_get_attempt_review",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(error_response.to_string()))
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("quiz_review")
                .with_arguments(json!({ "attemptid": 101 }).as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool quiz_review con error");

    assert!(res.is_error.unwrap_or(false));
    let raw_text = &res.content[0].as_text().unwrap().text;

    // El token no debe aparecer en claro
    assert!(!raw_text.contains("tok123"));
    // Debe haber sido sanitizado a <token>
    assert!(raw_text.contains("<token>"));
}

#[tokio::test]
async fn test_quiz_start_resumes_inprogress_attempt_without_start_attempt() {
    let mock = MockServer::start().await;

    // 1. Resolver cmid
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_course_get_course_module",
        ))
        .and(body_string_contains("cmid=979333"))
        .respond_with(ResponseTemplate::new(200).set_body_string(cmid_response(
            979333,
            68822,
            8233,
            "quiz",
            "Cuestionario de Evaluación 1",
        )))
        .mount(&mock)
        .await;

    // 2. mod_quiz_get_quiz_access_information (sin SEB)
    let access_info = json!({
        "accessrules": [],
        "preventaccessreasons": [],
        "activerulenames": ["quizaccess_numattempts"],
        "warnings": []
    });
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_quiz_get_quiz_access_information",
        ))
        .and(body_string_contains("quizid=68822"))
        .respond_with(ResponseTemplate::new(200).set_body_string(access_info.to_string()))
        .mount(&mock)
        .await;

    // 3. mod_quiz_get_user_attempts con intento inprogress
    let user_attempts = json!({
        "attempts": [
            {
                "id": 101,
                "quiz": 68822,
                "state": "inprogress",
                "timestart": 1700000000,
                "timelimit": 3600,
                "endtime": 1700003600,
                "layout": "1,2,0,3,4,0"
            }
        ],
        "warnings": []
    });
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_quiz_get_user_attempts",
        ))
        .and(body_string_contains("quizid=68822"))
        .respond_with(ResponseTemplate::new(200).set_body_string(user_attempts.to_string()))
        .mount(&mock)
        .await;

    // 4. mod_quiz_start_attempt NUNCA debe ser llamado
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains("wsfunction=mod_quiz_start_attempt"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("quiz_start")
                .with_arguments(json!({ "cmid": 979333 }).as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool quiz_start");

    assert!(!res.is_error.unwrap_or(false));
    let val: serde_json::Value =
        serde_json::from_str(&res.content[0].as_text().unwrap().text).unwrap();

    assert_eq!(val["attemptid"], 101);
    assert_eq!(val["state"], "inprogress");
    assert_eq!(val["timestart"], 1700000000);
    assert_eq!(val["timelimit"], 3600);
    assert_eq!(val["endtime"], 1700003600);
    assert_eq!(val["pages"], json!([0, 1]));
}

#[tokio::test]
async fn test_quiz_start_rejects_seb_with_impossible() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_course_get_course_module",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(cmid_response(
            979333,
            68822,
            8233,
            "quiz",
            "Cuestionario SEB",
        )))
        .mount(&mock)
        .await;

    // Regla SEB presente
    let access_info_seb = json!({
        "accessrules": ["Safe Exam Browser requerido"],
        "preventaccessreasons": [],
        "activerulenames": ["quizaccess_seb"],
        "warnings": []
    });
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_quiz_get_quiz_access_information",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(access_info_seb.to_string()))
        .mount(&mock)
        .await;

    // start_attempt nunca debe llamarse
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains("wsfunction=mod_quiz_start_attempt"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("quiz_start")
                .with_arguments(json!({ "cmid": 979333 }).as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool quiz_start con SEB");

    assert!(res.is_error.unwrap_or(false));
    let val: serde_json::Value =
        serde_json::from_str(&res.content[0].as_text().unwrap().text).unwrap();

    assert_eq!(val["error"]["kind"], "impossible");
    assert!(val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Safe Exam Browser"));
}

#[tokio::test]
async fn test_quiz_start_new_attempt_with_password() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_course_get_course_module_by_instance",
        ))
        .and(body_string_contains("instance=68822"))
        .respond_with(ResponseTemplate::new(200).set_body_string(cmid_response(
            979333,
            68822,
            8233,
            "quiz",
            "Cuestionario con Clave",
        )))
        .mount(&mock)
        .await;

    let access_info = json!({
        "accessrules": ["Se requiere contraseña"],
        "preventaccessreasons": [],
        "activerulenames": ["quizaccess_password"],
        "warnings": []
    });
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_quiz_get_quiz_access_information",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(access_info.to_string()))
        .mount(&mock)
        .await;

    // Sin intentos previos
    let empty_attempts = json!({
        "attempts": [],
        "warnings": []
    });
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_quiz_get_user_attempts",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(empty_attempts.to_string()))
        .mount(&mock)
        .await;

    // start_attempt con password en preflightdata
    let start_response = json!({
        "attempt": {
            "id": 105,
            "quiz": 68822,
            "state": "inprogress",
            "timestart": 1700000000,
            "timelimit": 1800,
            "endtime": 1700001800,
            "layout": "1,0"
        },
        "warnings": []
    });
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains("wsfunction=mod_quiz_start_attempt"))
        .and(body_string_contains("quizid=68822"))
        .and(body_string_contains(
            "preflightdata%5B0%5D%5Bname%5D=quizpassword",
        ))
        .and(body_string_contains(
            "preflightdata%5B0%5D%5Bvalue%5D=clave123",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(start_response.to_string()))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("quiz_start").with_arguments(
                json!({ "quizid": 68822, "password": "clave123" })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("call_tool quiz_start con password");

    assert!(!res.is_error.unwrap_or(false));
    let val: serde_json::Value =
        serde_json::from_str(&res.content[0].as_text().unwrap().text).unwrap();

    assert_eq!(val["attemptid"], 105);
    assert_eq!(val["state"], "inprogress");
    assert_eq!(val["timestart"], 1700000000);
    assert_eq!(val["timelimit"], 1800);
    assert_eq!(val["endtime"], 1700001800);
    assert_eq!(val["pages"], json!([0]));
}

#[tokio::test]
async fn test_quiz_get_page_stores_hidden_fields() {
    let mock = MockServer::start().await;

    let page_data = json!({
        "attempt": {
            "id": 101,
            "quiz": 68822,
            "state": "inprogress",
            "timestart": 1700000000,
            "timefinish": 0,
            "timeleft": 3540
        },
        "nextpage": 1,
        "timeleft": 3540,
        "questions": [
            {
                "slot": 1,
                "type": "multichoice",
                "number": 1,
                "status": "Sin responder aún",
                "state": "todo",
                "html": "<div class=\"que multichoice deferredfeedback notyetanswered\"><div class=\"qtext\"><p>¿Cuál es la capital de Nicaragua?</p></div><div class=\"ablock\"><div class=\"answer\"><div class=\"r0\"><input type=\"radio\" name=\"q101:1_answer\" value=\"0\" id=\"q101_1_answer0\" /><label for=\"q101_1_answer0\">San José</label></div><div class=\"r1\"><input type=\"radio\" name=\"q101:1_answer\" value=\"1\" id=\"q101_1_answer1\" /><label for=\"q101_1_answer1\">Managua</label></div></div></div><input type=\"hidden\" name=\"q101:1_:sequencecheck\" value=\"1\" /><input type=\"hidden\" name=\"q101:1_:flagged\" value=\"0\" /></div>"
            }
        ],
        "messages": [],
        "warnings": []
    });

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains("wsfunction=mod_quiz_get_attempt_data"))
        .and(body_string_contains("attemptid=101"))
        .and(body_string_contains("page=0"))
        .respond_with(ResponseTemplate::new(200).set_body_string(page_data.to_string()))
        .expect(1)
        .mount(&mock)
        .await;

    let (client, ctx) = start_server_and_ctx(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("quiz_get_page").with_arguments(
                json!({ "attemptid": 101, "page": 0 })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("call_tool quiz_get_page");

    assert!(!res.is_error.unwrap_or(false));
    let val: serde_json::Value =
        serde_json::from_str(&res.content[0].as_text().unwrap().text).unwrap();

    assert_eq!(val["attemptid"], 101);
    assert_eq!(val["page"], 0);
    assert_eq!(val["nextpage"], 1);
    assert_eq!(val["timeleft"], 3540);

    let questions = val["questions"].as_array().unwrap();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0]["slot"], 1);
    assert_eq!(questions[0]["hidden_fields"]["q101:1_:sequencecheck"], "1");
    assert_eq!(questions[0]["hidden_fields"]["q101:1_:flagged"], "0");

    // Validar que se almacenó en AppContext::quiz_hidden
    let hidden_map = ctx.quiz_hidden.lock().unwrap();
    let attempt_hidden = hidden_map
        .get(&101)
        .expect("debe existir attempt 101 en quiz_hidden");
    assert_eq!(
        attempt_hidden.get("q101:1_:sequencecheck"),
        Some(&"1".to_string())
    );
    assert_eq!(
        attempt_hidden.get("q101:1_:flagged"),
        Some(&"0".to_string())
    );
}

#[tokio::test]
async fn test_quiz_save_injects_sequencecheck_automatically() {
    let mock = MockServer::start().await;

    // 1. Mock de mod_quiz_save_attempt que espera la respuesta y el sequencecheck inyectado
    let save_response = json!({
        "status": true,
        "warnings": []
    });
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains("wsfunction=mod_quiz_save_attempt"))
        .and(body_string_contains("attemptid=101"))
        .and(body_string_contains("q101%3A1_answer"))
        .and(body_string_contains("q101%3A1_%3Asequencecheck"))
        .respond_with(ResponseTemplate::new(200).set_body_string(save_response.to_string()))
        .expect(1)
        .mount(&mock)
        .await;

    let (client, ctx) = start_server_and_ctx(&mock).await;

    // Precargar campo oculto en AppContext::quiz_hidden
    {
        let mut guard = ctx.quiz_hidden.lock().unwrap();
        let map = guard.entry(101).or_default();
        map.insert("q101:1_:sequencecheck".to_string(), "1".to_string());
        map.insert("q101:1_:flagged".to_string(), "0".to_string());
    }

    // Invocación SIN sequencecheck en answers
    let res = client
        .call_tool(
            CallToolRequestParams::new("quiz_save").with_arguments(
                json!({
                    "attemptid": 101,
                    "answers": {
                        "q101:1_answer": "1"
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("call_tool quiz_save");

    assert!(!res.is_error.unwrap_or(false));
    let val: serde_json::Value =
        serde_json::from_str(&res.content[0].as_text().unwrap().text).unwrap();

    assert_eq!(val["saved"], true);
    assert_eq!(val["attemptid"], 101);
}

#[tokio::test]
async fn test_quiz_save_without_cache_reads_page_first() {
    let mock = MockServer::start().await;

    // 1. mod_quiz_get_attempt_data (precarga automática de página 0)
    let page_data = json!({
        "attempt": {
            "id": 101,
            "quiz": 68822,
            "state": "inprogress",
            "timestart": 1700000000,
            "timefinish": 0,
            "timeleft": 3500
        },
        "nextpage": 1,
        "timeleft": 3500,
        "questions": [
            {
                "slot": 1,
                "type": "multichoice",
                "number": 1,
                "status": "Sin responder",
                "state": "todo",
                "html": "<div><input type=\"radio\" name=\"q101:1_answer\" value=\"1\" /><input type=\"hidden\" name=\"q101:1_:sequencecheck\" value=\"5\" /></div>"
            }
        ],
        "messages": [],
        "warnings": []
    });
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains("wsfunction=mod_quiz_get_attempt_data"))
        .and(body_string_contains("attemptid=101"))
        .and(body_string_contains("page=0"))
        .respond_with(ResponseTemplate::new(200).set_body_string(page_data.to_string()))
        .expect(1)
        .mount(&mock)
        .await;

    // 2. mod_quiz_save_attempt con sequencecheck = 5
    let save_response = json!({
        "status": true,
        "warnings": []
    });
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains("wsfunction=mod_quiz_save_attempt"))
        .and(body_string_contains("attemptid=101"))
        .and(body_string_contains("q101%3A1_%3Asequencecheck"))
        .and(body_string_contains("%5D=5"))
        .respond_with(ResponseTemplate::new(200).set_body_string(save_response.to_string()))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("quiz_save").with_arguments(
                json!({
                    "attemptid": 101,
                    "answers": {
                        "q101:1_answer": "1"
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("call_tool quiz_save sin caché previa");

    assert!(!res.is_error.unwrap_or(false));
    let val: serde_json::Value =
        serde_json::from_str(&res.content[0].as_text().unwrap().text).unwrap();

    assert_eq!(val["saved"], true);
    assert_eq!(val["attemptid"], 101);
}

#[tokio::test]
async fn test_quiz_save_warnings_fails_as_api_error() {
    let mock = MockServer::start().await;

    let save_response_warning = json!({
        "status": false,
        "warnings": [
            {
                "item": "quiz",
                "itemid": 101,
                "warningcode": "savefailed",
                "message": "No se pudo guardar la respuesta por límite de tiempo"
            }
        ]
    });
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains("wsfunction=mod_quiz_save_attempt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(save_response_warning.to_string()))
        .mount(&mock)
        .await;

    let (client, ctx) = start_server_and_ctx(&mock).await;
    {
        let mut guard = ctx.quiz_hidden.lock().unwrap();
        let map = guard.entry(101).or_default();
        map.insert("q101:1_:sequencecheck".to_string(), "1".to_string());
    }

    let res = client
        .call_tool(
            CallToolRequestParams::new("quiz_save").with_arguments(
                json!({
                    "attemptid": 101,
                    "answers": {
                        "q101:1_answer": "1"
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("call_tool quiz_save con warning");

    assert!(res.is_error.unwrap_or(false));
    let val: serde_json::Value =
        serde_json::from_str(&res.content[0].as_text().unwrap().text).unwrap();

    assert_eq!(val["error"]["kind"], "api");
    assert!(val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("No se pudo guardar la respuesta"));
}

#[tokio::test]
async fn test_quiz_finish_success_with_review() {
    let mock = MockServer::start().await;

    // 1. mod_quiz_process_attempt con finishattempt = 1
    let process_response = json!({
        "state": "finished",
        "warnings": []
    });
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains("wsfunction=mod_quiz_process_attempt"))
        .and(body_string_contains("attemptid=101"))
        .and(body_string_contains("finishattempt=1"))
        .and(body_string_contains("timeup=0"))
        .and(body_string_contains("q101%3A1_%3Asequencecheck"))
        .respond_with(ResponseTemplate::new(200).set_body_string(process_response.to_string()))
        .expect(1)
        .mount(&mock)
        .await;

    // 2. mod_quiz_get_attempt_review para obtener calificación final
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_quiz_get_attempt_review",
        ))
        .and(body_string_contains("attemptid=101"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_QUIZ_REVIEW))
        .expect(1)
        .mount(&mock)
        .await;

    let (client, ctx) = start_server_and_ctx(&mock).await;
    {
        let mut guard = ctx.quiz_hidden.lock().unwrap();
        let map = guard.entry(101).or_default();
        map.insert("q101:1_:sequencecheck".to_string(), "2".to_string());
    }

    let res = client
        .call_tool(
            CallToolRequestParams::new("quiz_finish")
                .with_arguments(json!({ "attemptid": 101 }).as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool quiz_finish");

    assert!(!res.is_error.unwrap_or(false));
    let val: serde_json::Value =
        serde_json::from_str(&res.content[0].as_text().unwrap().text).unwrap();

    assert_eq!(val["finished"], true);
    assert_eq!(val["attemptid"], 101);
    assert_eq!(val["state"], "finished");
    assert_eq!(val["grade"], 9.5);

    // Validar que se limpiaron los campos en memoria
    let hidden_guard = ctx.quiz_hidden.lock().unwrap();
    assert!(hidden_guard.get(&101).is_none());
}

#[tokio::test]
async fn test_quiz_finish_warnings_fails_as_api_error() {
    let mock = MockServer::start().await;

    let process_response_warning = json!({
        "state": "inprogress",
        "warnings": [
            {
                "item": "quiz",
                "itemid": 101,
                "warningcode": "finishfailed",
                "message": "No se puede cerrar el intento por validación pendiente"
            }
        ]
    });
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains("wsfunction=mod_quiz_process_attempt"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(process_response_warning.to_string()),
        )
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("quiz_finish")
                .with_arguments(json!({ "attemptid": 101 }).as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool quiz_finish con warning");

    assert!(res.is_error.unwrap_or(false));
    let val: serde_json::Value =
        serde_json::from_str(&res.content[0].as_text().unwrap().text).unwrap();

    assert_eq!(val["error"]["kind"], "api");
    assert!(val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("No se puede cerrar el intento"));
}
