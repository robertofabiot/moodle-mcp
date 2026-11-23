mod common;

use common::start_server;
use moodle_mcp::moodle::client::{MoodleClient, MoodleClientBuilder};
use moodle_mcp::moodle::gate::MoodleGate;
use moodle_mcp::moodle::ids::{resolve_cmid, resolve_instance, CourseModuleRef};
use rmcp::model::CallToolRequestParams;
use serde_json::json;
use std::collections::HashMap;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURE_SITE_INFO: &str = include_str!("fixtures/site_info.json");

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

#[test]
fn test_catalog_functions_schema_and_site_info_presence() {
    let catalog_raw = include_str!("../catalog/functions.json");
    let catalog: serde_json::Value =
        serde_json::from_str(catalog_raw).expect("catalog/functions.json debe ser un JSON válido");

    let catalog_obj = catalog
        .as_object()
        .expect("El catálogo debe ser un objeto JSON");
    assert!(!catalog_obj.is_empty(), "El catálogo no debe estar vacío");

    let site_info_raw = include_str!("fixtures/site_info.json");
    let site_info: serde_json::Value =
        serde_json::from_str(site_info_raw).expect("site_info.json debe ser un JSON válido");

    let site_functions: std::collections::HashSet<String> = site_info
        .get("functions")
        .and_then(|f| f.as_array())
        .expect("site_info.functions debe ser un array")
        .iter()
        .filter_map(|func| func.get("name").and_then(|n| n.as_str()).map(String::from))
        .collect();

    for (fn_name, fn_meta) in catalog_obj {
        // 1. Debe existir en site_info.json
        assert!(
            site_functions.contains(fn_name),
            "La función '{fn_name}' del catálogo no se encuentra en site_info.json"
        );

        // 2. Debe contener description no vacía
        let desc = fn_meta
            .get("description")
            .and_then(|d| d.as_str())
            .expect("description debe ser string");
        assert!(
            !desc.trim().is_empty(),
            "La función '{fn_name}' debe tener description no vacía"
        );

        // 3. Debe contener params como objeto
        assert!(
            fn_meta.get("params").and_then(|p| p.as_object()).is_some(),
            "La función '{fn_name}' debe tener params como objeto"
        );
    }
}

#[tokio::test]
async fn test_courses_tools_annotations() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    let tools = client.list_all_tools().await.expect("list_all_tools");
    let tool_map: HashMap<String, _> = tools.into_iter().map(|t| (t.name.to_string(), t)).collect();

    let courses_tools = [
        "list_courses",
        "get_course",
        "course_updates_since",
        "get_grades",
        "upcoming_events",
    ];

    for name in courses_tools {
        let tool = tool_map
            .get(name)
            .unwrap_or_else(|| panic!("Herramienta '{name}' debe estar registrada"));
        let ann = tool
            .annotations
            .as_ref()
            .unwrap_or_else(|| panic!("Anotaciones de '{name}' deben existir"));
        assert_eq!(
            ann.read_only_hint,
            Some(true),
            "'{name}' debe tener read_only_hint = true"
        );
    }
}

#[tokio::test]
async fn test_moodle_describe_function_with_catalog_and_fallback() {
    let mock = MockServer::start().await;
    mock_site_info(&mock).await;
    let client = start_server(&mock).await;

    // 1. Función en catálogo
    let args1 = json!({ "name": "core_course_get_contents" })
        .as_object()
        .unwrap()
        .clone();
    let res1 = client
        .call_tool(CallToolRequestParams::new("moodle_describe_function").with_arguments(args1))
        .await
        .expect("describe core_course_get_contents");
    let text1 = res1.content[0].as_text().expect("text").text.clone();
    let parsed1: serde_json::Value = serde_json::from_str(&text1).expect("json");
    assert_eq!(parsed1["name"], "core_course_get_contents");
    assert_eq!(parsed1["hint"], "Firma disponible en el catálogo");
    assert!(parsed1["signature"]["description"].is_string());

    // 2. Función fuera de catálogo pero habilitada
    let args2 = json!({ "name": "core_webservice_get_site_info" })
        .as_object()
        .unwrap()
        .clone();
    let res2 = client
        .call_tool(CallToolRequestParams::new("moodle_describe_function").with_arguments(args2))
        .await
        .expect("describe core_webservice_get_site_info");
    let text2 = res2.content[0].as_text().expect("text").text.clone();
    let parsed2: serde_json::Value = serde_json::from_str(&text2).expect("json");
    assert_eq!(parsed2["hint"], "Firma disponible en el catálogo");
    assert!(parsed2["signature"]["params"].is_object());
}

#[tokio::test]
async fn test_resolve_cmid_and_instance() {
    let mock = MockServer::start().await;
    let base_url = mock.uri();
    let gate = MoodleGate::new(4, 3).without_jitter();
    let client =
        MoodleClient::from_builder(MoodleClientBuilder::new().base_url(&base_url).gate(gate));

    // Mock para core_course_get_course_module
    let cm_response = json!({
        "cm": {
            "id": 893829,
            "instance": 9696,
            "course": 101,
            "modname": "assign",
            "name": "Tarea 1: Algoritmos"
        }
    });

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&cm_response))
        .mount(&mock)
        .await;

    let res_cmid = resolve_cmid(&client, "tok123", 893829)
        .await
        .expect("resolve_cmid");
    assert_eq!(
        res_cmid,
        CourseModuleRef {
            cmid: 893829,
            instance: 9696,
            course: 101,
            modname: "assign".to_string(),
            name: "Tarea 1: Algoritmos".to_string(),
        }
    );

    let res_inst = resolve_instance(&client, "tok123", "assign", 9696)
        .await
        .expect("resolve_instance");
    assert_eq!(res_inst, res_cmid);
}

#[tokio::test]
async fn test_list_courses_filtering_active() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    let now = chrono::Utc::now().timestamp();
    let courses_payload = json!([
        {
            "id": 10,
            "shortname": "ACT101",
            "fullname": "Curso Activo",
            "visible": 1,
            "enddate": now + 86400 * 30
        },
        {
            "id": 20,
            "shortname": "PAS201",
            "fullname": "Curso Pasado",
            "visible": 1,
            "enddate": now - 86400 * 10
        }
    ]);

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&courses_payload))
        .mount(&mock)
        .await;

    // 1. Por defecto (include_past = false): solo devuelve el curso activo
    let args1 = json!({ "include_past": false })
        .as_object()
        .unwrap()
        .clone();
    let res1 = client
        .call_tool(CallToolRequestParams::new("list_courses").with_arguments(args1))
        .await
        .expect("list_courses activos");
    let text1 = res1.content[0].as_text().expect("text").text.clone();
    let list1: Vec<serde_json::Value> = serde_json::from_str(&text1).expect("json list");
    assert_eq!(list1.len(), 1);
    assert_eq!(list1[0]["id"], 10);
    assert_eq!(list1[0]["active"], true);

    // 2. Con include_past = true: devuelve ambos
    let args2 = json!({ "include_past": true }).as_object().unwrap().clone();
    let res2 = client
        .call_tool(CallToolRequestParams::new("list_courses").with_arguments(args2))
        .await
        .expect("list_courses todos");
    let text2 = res2.content[0].as_text().expect("text").text.clone();
    let list2: Vec<serde_json::Value> = serde_json::from_str(&text2).expect("json list");
    assert_eq!(list2.len(), 2);
}

#[tokio::test]
async fn test_get_course_detail_and_markdown() {
    let mock = MockServer::start().await;
    let base_url = mock.uri();
    let client = start_server(&mock).await;

    let tree_payload = json!([
        {
            "id": 1,
            "section": 1,
            "name": "Unidad I",
            "summary": "<h3>Introducción</h3><p>Bienvenidos al <b>curso</b>.</p>",
            "visible": 1,
            "modules": [
                {
                    "id": 105,
                    "instance": 205,
                    "modname": "page",
                    "name": "Guía de bienvenida",
                    "description": "<p>Por favor <i>leer atentamente</i>.</p>",
                    "contents": [
                        {
                            "type": "file",
                            "filename": "syllabus.pdf",
                            "filesize": 1024,
                            "fileurl": format!("{base_url}/pluginfile.php/1/syllabus.pdf"),
                            "mimetype": "application/pdf"
                        },
                        {
                            "type": "url",
                            "filename": "meet_call",
                            "fileurl": "https://meet.google.com/xyz-123"
                        }
                    ]
                }
            ]
        }
    ]);

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&tree_payload))
        .mount(&mock)
        .await;

    let args = json!({ "courseid": 42 }).as_object().unwrap().clone();
    let res = client
        .call_tool(CallToolRequestParams::new("get_course").with_arguments(args))
        .await
        .expect("get_course");
    let text = res.content[0].as_text().expect("text").text.clone();
    let sections: Vec<serde_json::Value> = serde_json::from_str(&text).expect("json");

    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0]["name"], "Unidad I");
    let summary = sections[0]["summary_markdown"].as_str().unwrap();
    assert!(summary.contains("### Introducción"));
    assert!(summary.contains("curso"));

    let mod1 = &sections[0]["modules"][0];
    assert_eq!(mod1["cmid"], 105);
    assert_eq!(mod1["instance"], 205);
    let desc = mod1["description_markdown"].as_str().unwrap();
    assert!(desc.contains("leer atentamente"));

    // Archivo normal
    let files = mod1["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["filename"], "syllabus.pdf");

    // Enlace externo
    let ext = mod1["external_links"].as_array().unwrap();
    assert_eq!(ext.len(), 1);
    assert_eq!(ext[0], "https://meet.google.com/xyz-123");
}

#[tokio::test]
async fn test_course_updates_since() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    let updates_payload = json!({
        "instances": [
            { "id": 105, "updates": [{ "name": "configuration", "timeupdated": 1700000005 }] }
        ]
    });

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&updates_payload))
        .mount(&mock)
        .await;

    let args = json!({ "courseid": 55, "since": 1700000000 })
        .as_object()
        .unwrap()
        .clone();
    let res = client
        .call_tool(CallToolRequestParams::new("course_updates_since").with_arguments(args))
        .await
        .expect("course_updates_since");
    let text = res.content[0].as_text().expect("text").text.clone();
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(parsed["instances"][0]["id"], 105);
}

#[tokio::test]
async fn test_get_grades_overview_and_course_items() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    // 1. Overview (sin courseid)
    let overview_payload = json!({
        "grades": [
            { "courseid": 101, "grade": "85.5", "rawgrade": "85.5000" },
            { "courseid": 102, "grade": "92.0", "rawgrade": "92.0000" }
        ]
    });

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&overview_payload))
        .up_to_n_times(1)
        .mount(&mock)
        .await;

    let args1 = json!({}).as_object().unwrap().clone();
    let res1 = client
        .call_tool(CallToolRequestParams::new("get_grades").with_arguments(args1))
        .await
        .expect("get_grades overview");
    let text1 = res1.content[0].as_text().expect("text").text.clone();
    let grades1: Vec<serde_json::Value> = serde_json::from_str(&text1).expect("json");
    assert_eq!(grades1.len(), 2);
    assert_eq!(grades1[0]["item"], "Curso 101");
    assert_eq!(grades1[0]["grade"], "85.5");

    // 2. Por curso específico (con courseid)
    let usergrades_payload = json!({
        "usergrades": [
            {
                "courseid": 101,
                "userid": 10001,
                "gradeitems": [
                    {
                        "itemname": "Examen Parcial",
                        "itemmodule": "quiz",
                        "gradeformatted": "90.0",
                        "grademin": 0.0,
                        "grademax": 100.0,
                        "percentageformatted": "90 %",
                        "feedback": "<p>Excelente trabajo</p>"
                    }
                ]
            }
        ]
    });

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&usergrades_payload))
        .up_to_n_times(1)
        .mount(&mock)
        .await;

    let args2 = json!({ "courseid": 101 }).as_object().unwrap().clone();
    let res2 = client
        .call_tool(CallToolRequestParams::new("get_grades").with_arguments(args2))
        .await
        .expect("get_grades course");
    let text2 = res2.content[0].as_text().expect("text").text.clone();
    let grades2: Vec<serde_json::Value> = serde_json::from_str(&text2).expect("json");
    assert_eq!(grades2.len(), 1);
    assert_eq!(grades2[0]["item"], "Examen Parcial");
    assert_eq!(grades2[0]["grade"], "90.0");
    assert_eq!(grades2[0]["range"], "0 - 100");
    assert_eq!(grades2[0]["percentage"], "90 %");
    assert!(grades2[0]["feedback_markdown"]
        .as_str()
        .unwrap()
        .contains("Excelente trabajo"));
}

#[tokio::test]
async fn test_upcoming_events() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    let calendar_payload = json!({
        "events": [
            {
                "name": "Entrega de Proyecto",
                "course": { "fullname": "Ingeniería de Software" },
                "modulename": "assign",
                "instance": 55,
                "timesort": 1780000000,
                "url": "https://moodle.example/mod/assign/view.php?id=55",
                "action": { "name": "Entregar tarea" }
            }
        ]
    });

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&calendar_payload))
        .mount(&mock)
        .await;

    let args = json!({ "days": 7 }).as_object().unwrap().clone();
    let res = client
        .call_tool(CallToolRequestParams::new("upcoming_events").with_arguments(args))
        .await
        .expect("upcoming_events");
    let text = res.content[0].as_text().expect("text").text.clone();
    let events: Vec<serde_json::Value> = serde_json::from_str(&text).expect("json");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["name"], "Entrega de Proyecto");
    assert_eq!(events[0]["course"], "Ingeniería de Software");
    assert_eq!(events[0]["modname"], "assign");
    assert_eq!(events[0]["cmid"], 55);
    assert!(events[0].get("courseid").is_some());
    assert!(events[0]["courseid"].is_null());
    assert_eq!(events[0]["action"], "Entregar tarea");
    // Verificar formato ISO 8601
    let ts_str = events[0]["timesort"].as_str().unwrap();
    assert!(ts_str.contains('T'), "Debe ser formato ISO 8601 con 'T'");
}

#[test]
fn test_canonical_catalog_moodle_45_parameters() {
    let catalog_raw = include_str!("../catalog/functions.json");
    let catalog: serde_json::Value =
        serde_json::from_str(catalog_raw).expect("catalog/functions.json debe ser un JSON válido");

    // 1. core_calendar_get_action_events_by_timesort: searchvalue
    assert!(
        catalog["core_calendar_get_action_events_by_timesort"]["params"]["searchvalue"].is_object()
    );

    // 2. core_message_get_conversations: favourites, mergeself
    assert!(catalog["core_message_get_conversations"]["params"]["favourites"].is_object());
    assert!(catalog["core_message_get_conversations"]["params"]["mergeself"].is_object());

    // 3. core_message_get_conversation_messages: timefrom
    assert!(catalog["core_message_get_conversation_messages"]["params"]["timefrom"].is_object());

    // 4. core_course_get_contents: options[].name, options[].value
    assert!(catalog["core_course_get_contents"]["params"]["options[].name"].is_object());
    assert!(catalog["core_course_get_contents"]["params"]["options[].value"].is_object());

    // 5. mod_quiz_save_attempt: preflightdata[].name, preflightdata[].value, data[].name, data[].value
    assert!(catalog["mod_quiz_save_attempt"]["params"]["data[].name"].is_object());
    assert!(catalog["mod_quiz_save_attempt"]["params"]["data[].value"].is_object());
    assert!(catalog["mod_quiz_save_attempt"]["params"]["preflightdata[].name"].is_object());
    assert!(catalog["mod_quiz_save_attempt"]["params"]["preflightdata[].value"].is_object());

    // 6. mod_quiz_process_attempt: preflightdata
    assert!(catalog["mod_quiz_process_attempt"]["params"]["preflightdata"].is_object());
    assert!(catalog["mod_quiz_process_attempt"]["params"]["preflightdata[].name"].is_object());
    assert!(catalog["mod_quiz_process_attempt"]["params"]["preflightdata[].value"].is_object());

    // 7. mod_quiz_start_attempt, mod_quiz_get_attempt_data, mod_quiz_get_attempt_summary: preflightdata[].name
    for fn_name in [
        "mod_quiz_start_attempt",
        "mod_quiz_get_attempt_data",
        "mod_quiz_get_attempt_summary",
    ] {
        assert!(catalog[fn_name]["params"]["preflightdata[].name"].is_object());
        assert!(catalog[fn_name]["params"]["preflightdata[].value"].is_object());
    }

    // 8. mod_forum_add_discussion_post: messageformat, options[].name
    assert!(catalog["mod_forum_add_discussion_post"]["params"]["messageformat"].is_object());
    assert!(catalog["mod_forum_add_discussion_post"]["params"]["options[].name"].is_object());
    assert!(catalog["mod_forum_add_discussion_post"]["params"]["options[].value"].is_object());

    // 9. mod_forum_add_discussion: options[].name
    assert!(catalog["mod_forum_add_discussion"]["params"]["options[].name"].is_object());
    assert!(catalog["mod_forum_add_discussion"]["params"]["options[].value"].is_object());

    // 10. mod_forum_get_forum_discussions: groupid
    assert!(catalog["mod_forum_get_forum_discussions"]["params"]["groupid"].is_object());

    // 11. mod_forum_get_discussion_posts: includeinlineattachments
    assert!(
        catalog["mod_forum_get_discussion_posts"]["params"]["includeinlineattachments"].is_object()
    );

    // 12. core_message_send_instant_messages: messages[].touserid, text, textformat, clientmsgid
    assert!(
        catalog["core_message_send_instant_messages"]["params"]["messages[].touserid"].is_object()
    );
    assert!(catalog["core_message_send_instant_messages"]["params"]["messages[].text"].is_object());
    assert!(
        catalog["core_message_send_instant_messages"]["params"]["messages[].textformat"]
            .is_object()
    );
    assert!(
        catalog["core_message_send_instant_messages"]["params"]["messages[].clientmsgid"]
            .is_object()
    );

    // 13. core_message_send_messages_to_conversation: messages[].text, textformat
    assert!(
        catalog["core_message_send_messages_to_conversation"]["params"]["messages[].text"]
            .is_object()
    );
    assert!(
        catalog["core_message_send_messages_to_conversation"]["params"]["messages[].textformat"]
            .is_object()
    );
}

#[tokio::test]
async fn test_upcoming_events_with_cmid_and_courseid() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    let calendar_payload = json!({
        "events": [
            {
                "name": "Entrega de Proyecto",
                "course": { "id": 42, "fullname": "Ingeniería de Software" },
                "modulename": "assign",
                "instance": 55,
                "timesort": 1780000000,
                "url": "https://moodle.example/mod/assign/view.php?id=999",
                "action": { "name": "Entregar tarea" }
            },
            {
                "name": "Evento Sin Módulo",
                "timesort": 1780000000,
                "url": "https://moodle.example/calendar/view.php"
            }
        ]
    });

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&calendar_payload))
        .mount(&mock)
        .await;

    let args = json!({ "days": 7 }).as_object().unwrap().clone();
    let res = client
        .call_tool(CallToolRequestParams::new("upcoming_events").with_arguments(args))
        .await
        .expect("upcoming_events");
    let text = res.content[0].as_text().expect("text").text.clone();
    let events: Vec<serde_json::Value> = serde_json::from_str(&text).expect("json");

    assert_eq!(events.len(), 2);
    // Evento con id de curso y cmid extraído
    assert_eq!(events[0]["courseid"], 42);
    assert_eq!(events[0]["cmid"], 999);

    // Evento sin curso ni cmid (deben serializarse explícitamente como null)
    assert!(events[1].get("courseid").is_some());
    assert!(events[1]["courseid"].is_null());
    assert!(events[1].get("cmid").is_some());
    assert!(events[1]["cmid"].is_null());
}
