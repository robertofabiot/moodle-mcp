mod common;

use common::start_server;
use rmcp::model::CallToolRequestParams;
use serde_json::json;
use std::collections::HashMap;
use std::io::Write;
use tempfile::NamedTempFile;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURE_FORUMS: &str = include_str!("fixtures/forums_by_courses.json");
const FIXTURE_DISCUSSIONS: &str = include_str!("fixtures/forum_discussions.json");
const FIXTURE_POSTS: &str = include_str!("fixtures/forum_discussion_posts.json");
const FIXTURE_CONVERSATIONS: &str = include_str!("fixtures/message_conversations.json");
const FIXTURE_CONV_MESSAGES: &str = include_str!("fixtures/message_conversation_messages.json");
const FIXTURE_INVALID_TOKEN: &str = include_str!("fixtures/error_invalidtoken.json");

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

fn parse_tool_json(res: &rmcp::model::CallToolResult) -> serde_json::Value {
    let text = res.content[0].as_text().expect("debe ser texto");
    serde_json::from_str(&text.text).expect("debe ser JSON válido")
}

#[tokio::test]
async fn test_forums_and_messaging_tools_annotations() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    let tools = client.list_all_tools().await.expect("list_all_tools");
    let tool_map: HashMap<String, _> = tools.into_iter().map(|t| (t.name.to_string(), t)).collect();

    let read_only_tools = [
        "forum_list",
        "forum_list_discussions",
        "forum_get_discussion",
        "list_conversations",
        "get_conversation",
    ];
    for name in read_only_tools {
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
        assert_ne!(
            ann.destructive_hint,
            Some(true),
            "'{name}' no debe tener destructive_hint = true"
        );
    }

    let destructive_tools = ["forum_reply", "forum_new_discussion", "send_message"];
    for name in destructive_tools {
        let tool = tool_map
            .get(name)
            .unwrap_or_else(|| panic!("Herramienta '{name}' debe estar registrada"));
        let ann = tool
            .annotations
            .as_ref()
            .unwrap_or_else(|| panic!("Anotaciones de '{name}' deben existir"));
        assert_eq!(
            ann.destructive_hint,
            Some(true),
            "'{name}' debe tener destructive_hint = true"
        );
    }
}

#[tokio::test]
async fn test_forum_list_maps_fields_and_markdown() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_forums_by_courses",
        ))
        .and(body_string_contains("courseids%5B0%5D=8233"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_FORUMS))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("forum_list")
                .with_arguments(json!({ "courseid": 8233 }).as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool forum_list");

    assert!(!res.is_error.unwrap_or(false));
    let val = parse_tool_json(&res);

    assert_eq!(val["courseid"], 8233);
    let forums = val["forums"].as_array().unwrap();
    assert_eq!(forums.len(), 2);

    assert_eq!(forums[0]["id"], 88);
    assert_eq!(forums[0]["cmid"], 980100);
    assert_eq!(forums[0]["course"], 8233);
    assert_eq!(forums[0]["name"], "Avisos Generales");
    assert_eq!(forums[0]["type"], "news");
    assert!(forums[0]["intro_markdown"]
        .as_str()
        .unwrap()
        .contains("**avisos**"));
    assert_eq!(forums[0]["numdiscussions"], 2);
    assert_eq!(forums[0]["duedate"], 0);
    assert_eq!(forums[0]["cutoffdate"], 0);

    assert_eq!(forums[1]["id"], 89);
    assert_eq!(forums[1]["duedate"], 1700003600);
    assert_eq!(forums[1]["cutoffdate"], 1700007200);
}

#[tokio::test]
async fn test_forum_list_discussions_by_cmid_and_forumid_identical() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_course_get_course_module",
        ))
        .and(body_string_contains("cmid=980101"))
        .respond_with(ResponseTemplate::new(200).set_body_string(cmid_response(
            980101,
            89,
            8233,
            "forum",
            "Foro de Consultas",
        )))
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_course_get_course_module_by_instance",
        ))
        .and(body_string_contains("module=forum"))
        .and(body_string_contains("instance=89"))
        .respond_with(ResponseTemplate::new(200).set_body_string(cmid_response(
            980101,
            89,
            8233,
            "forum",
            "Foro de Consultas",
        )))
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_forum_discussions",
        ))
        .and(body_string_contains("forumid=89"))
        .and(body_string_contains("page=0"))
        .and(body_string_contains("perpage=20"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_DISCUSSIONS))
        .expect(2)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_forum_discussions",
        ))
        .and(body_string_contains("forumid=89"))
        .and(body_string_contains("page=2"))
        .and(body_string_contains("perpage=5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "discussions": [],
            "warnings": []
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res_cmid = client
        .call_tool(
            CallToolRequestParams::new("forum_list_discussions")
                .with_arguments(json!({ "cmid": 980101 }).as_object().unwrap().clone()),
        )
        .await
        .expect("forum_list_discussions cmid");
    assert!(!res_cmid.is_error.unwrap_or(false));
    let val_cmid = parse_tool_json(&res_cmid);

    let res_forumid = client
        .call_tool(
            CallToolRequestParams::new("forum_list_discussions")
                .with_arguments(json!({ "forumid": 89 }).as_object().unwrap().clone()),
        )
        .await
        .expect("forum_list_discussions forumid");
    assert!(!res_forumid.is_error.unwrap_or(false));
    let val_forumid = parse_tool_json(&res_forumid);

    assert_eq!(val_cmid, val_forumid);
    assert_eq!(val_cmid["cmid"], 980101);
    assert_eq!(val_cmid["instance"], 89);
    assert_eq!(val_cmid["courseid"], 8233);
    assert_eq!(val_cmid["warnings"].as_array().unwrap().len(), 0);

    let discussions = val_cmid["discussions"].as_array().unwrap();
    assert_eq!(discussions.len(), 2);
    assert_eq!(discussions[0]["id"], 456);
    assert_eq!(discussions[0]["discussion"], 105);
    assert_eq!(discussions[0]["name"], "Duda sobre el proyecto final");
    assert_eq!(discussions[0]["subject"], "Duda sobre el proyecto final");
    assert!(discussions[0]["message_markdown"]
        .as_str()
        .unwrap()
        .contains("**parte práctica**"));
    assert_eq!(discussions[0]["userfullname"], "Docente Ejemplo");
    assert_eq!(discussions[0]["timemodified"], 1700000500);
    assert!(discussions[0]["timemodified_iso"]
        .as_str()
        .unwrap()
        .contains('T'));
    assert_eq!(discussions[0]["numreplies"], 2);
    assert_eq!(discussions[0]["pinned"], true);
    assert_eq!(discussions[0]["locked"], false);

    // Paginación explícita
    let res_paged = client
        .call_tool(
            CallToolRequestParams::new("forum_list_discussions").with_arguments(
                json!({ "forumid": 89, "page": 2, "perpage": 5 })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("forum_list_discussions paged");
    assert!(!res_paged.is_error.unwrap_or(false));
    assert_eq!(
        parse_tool_json(&res_paged)["discussions"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn test_forum_list_discussions_invalid_arguments() {
    let mock = MockServer::start().await;

    // cmid de otro tipo de módulo (assign en vez de forum)
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
            "assign",
            "Laboratorio JavaFX",
        )))
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    // 1. Ambos presentes
    let res_both = client
        .call_tool(
            CallToolRequestParams::new("forum_list_discussions").with_arguments(
                json!({ "cmid": 980101, "forumid": 89 })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("both");
    assert!(res_both.is_error.unwrap_or(false));
    assert_eq!(
        parse_tool_json(&res_both)["error"]["kind"],
        "invalid_arguments"
    );

    // 2. Ninguno presente
    let res_neither = client
        .call_tool(
            CallToolRequestParams::new("forum_list_discussions")
                .with_arguments(json!({}).as_object().unwrap().clone()),
        )
        .await
        .expect("neither");
    assert!(res_neither.is_error.unwrap_or(false));
    assert_eq!(
        parse_tool_json(&res_neither)["error"]["kind"],
        "invalid_arguments"
    );

    // 3. Módulo de tipo incorrecto
    let res_wrong = client
        .call_tool(
            CallToolRequestParams::new("forum_list_discussions")
                .with_arguments(json!({ "cmid": 979333 }).as_object().unwrap().clone()),
        )
        .await
        .expect("wrong modname");
    assert!(res_wrong.is_error.unwrap_or(false));
    assert_eq!(
        parse_tool_json(&res_wrong)["error"]["kind"],
        "invalid_arguments"
    );
}

#[tokio::test]
async fn test_forum_get_discussion_sort_markdown_and_attachments() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_discussion_posts",
        ))
        .and(body_string_contains("discussionid=105"))
        .and(body_string_contains("sortby=created"))
        .and(body_string_contains("sortdirection=ASC"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_POSTS))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("forum_get_discussion")
                .with_arguments(json!({ "discussionid": 105 }).as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool forum_get_discussion");

    assert!(!res.is_error.unwrap_or(false));
    let val = parse_tool_json(&res);

    assert_eq!(val["discussionid"], 105);
    let posts = val["posts"].as_array().unwrap();
    assert_eq!(posts.len(), 2);

    assert_eq!(posts[0]["postid"], 204);
    assert_eq!(posts[0]["parentid"], 0);
    assert_eq!(posts[0]["author"], "Estudiante Prueba");
    assert_eq!(posts[0]["subject"], "Duda sobre el proyecto final");
    assert!(posts[0]["message_markdown"]
        .as_str()
        .unwrap()
        .contains("borrador inicial"));
    assert_eq!(posts[0]["timecreated"], 1700000100);
    assert!(posts[0]["timecreated_iso"].as_str().unwrap().contains('T'));

    let att = &posts[0]["attachments"][0];
    assert_eq!(att["filename"], "esquema.pdf");
    assert_eq!(att["filesize"], 20480);
    assert_eq!(
        att["fileurl"],
        "https://moodle.example/pluginfile.php/123/mod_forum/attachment/204/esquema.pdf"
    );
    assert_eq!(att["mimetype"], "application/pdf");

    assert_eq!(posts[1]["postid"], 205);
    assert_eq!(posts[1]["parentid"], 204);
    assert_eq!(posts[1]["author"], "Docente Ejemplo");
    assert!(posts[1]["message_markdown"]
        .as_str()
        .unwrap()
        .contains("**implementación**"));
}

#[tokio::test]
async fn test_forum_reply_with_attachment_and_auto_subject() {
    let mock = MockServer::start().await;

    // 1. Consulta del post padre para resolver asunto automático
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_discussion_post",
        ))
        .and(body_string_contains("postid=204"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "post": {
                "id": 204,
                "subject": "Duda sobre el proyecto final"
            },
            "warnings": []
        })))
        .expect(1)
        .mount(&mock)
        .await;

    // 2. Subida del archivo adjunto
    Mock::given(method("POST"))
        .and(path("/webservice/upload.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "itemid": 777888,
                "filename": "anexo.txt"
            }
        ])))
        .expect(1)
        .mount(&mock)
        .await;

    // 3. Publicación de la respuesta con attachmentsid y messageformat=1
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_add_discussion_post",
        ))
        .and(body_string_contains("postid=204"))
        .and(body_string_contains(
            "subject=Re%3A+Duda+sobre+el+proyecto+final",
        ))
        .and(body_string_contains("messageformat=1"))
        .and(body_string_contains(
            "options%5B0%5D%5Bname%5D=attachmentsid",
        ))
        .and(body_string_contains("options%5B0%5D%5Bvalue%5D=777888"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "postid": 501,
            "warnings": []
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"Contenido de prueba para el foro").unwrap();
    tmp.flush().unwrap();
    let tmp_path = tmp.path().to_string_lossy().to_string();

    let res = client
        .call_tool(
            CallToolRequestParams::new("forum_reply").with_arguments(
                json!({
                    "postid": 204,
                    "message": "Aquí está el **anexo** solicitado.",
                    "attachments": [tmp_path],
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("call_tool forum_reply");

    assert!(!res.is_error.unwrap_or(false));
    let val = parse_tool_json(&res);
    assert_eq!(val["postid"], 501);
    assert_eq!(val["subject"], "Re: Duda sobre el proyecto final");

    // Verificar que message se convirtió a HTML en la petición a Moodle
    let requests = mock.received_requests().await.unwrap();
    let add_post_req = requests
        .iter()
        .find(|r| {
            String::from_utf8_lossy(&r.body).contains("wsfunction=mod_forum_add_discussion_post")
        })
        .expect("debe existir petición mod_forum_add_discussion_post");
    let body_str = String::from_utf8_lossy(&add_post_req.body);
    let parsed_form: HashMap<String, String> = url::form_urlencoded::parse(body_str.as_bytes())
        .into_owned()
        .collect();
    assert!(parsed_form["message"].contains("<strong>anexo</strong>"));
}

#[tokio::test]
async fn test_forum_reply_preserves_existing_re_prefix() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_discussion_post",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "post": {
                "id": 205,
                "subject": "re: Consulta previa"
            },
            "warnings": []
        })))
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_add_discussion_post",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "postid": 502,
            "warnings": []
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("forum_reply").with_arguments(
                json!({
                    "postid": 205,
                    "subject": "   ",
                    "message": "Enterado, gracias.",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("call_tool forum_reply whitespace subject");

    assert!(!res.is_error.unwrap_or(false));
    let val = parse_tool_json(&res);
    assert_eq!(val["postid"], 502);
    assert_eq!(val["subject"], "re: Consulta previa");
}

#[tokio::test]
async fn test_forum_reply_with_explicit_subject_and_missing_attachment() {
    let mock = MockServer::start().await;

    // Cuando se envía subject explícito, NO debe consultarse mod_forum_get_discussion_post
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_discussion_post",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(0)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_add_discussion_post",
        ))
        .and(body_string_contains("subject=Asunto+expl%C3%ADcito"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "postid": 503,
            "warnings": []
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    // 1. Con asunto explícito y attachments vacío []
    let res_ok = client
        .call_tool(
            CallToolRequestParams::new("forum_reply").with_arguments(
                json!({
                    "postid": 204,
                    "subject": "Asunto explícito",
                    "message": "Respuesta sin adjuntos.",
                    "attachments": [],
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("explicit subject");
    assert!(!res_ok.is_error.unwrap_or(false));
    let val_ok = parse_tool_json(&res_ok);
    assert_eq!(val_ok["postid"], 503);
    assert_eq!(val_ok["subject"], "Asunto explícito");

    // 2. Archivo adjunto inexistente -> invalid_arguments
    let res_missing = client
        .call_tool(
            CallToolRequestParams::new("forum_reply").with_arguments(
                json!({
                    "postid": 204,
                    "subject": "Asunto explícito",
                    "message": "Respuesta con archivo inexistente.",
                    "attachments": ["/ruta/inexistente/no_existe_12345.pdf"],
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("missing attachment");
    assert!(res_missing.is_error.unwrap_or(false));
    assert_eq!(
        parse_tool_json(&res_missing)["error"]["kind"],
        "invalid_arguments"
    );
}

#[tokio::test]
async fn test_forum_reply_validation_warnings_and_no_retry_on_503() {
    let mock = MockServer::start().await;

    // 1. Primera llamada con warnings, segunda con 503
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_add_discussion_post",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "postid": 0,
            "warnings": [{
                "item": "post",
                "itemid": 204,
                "warningcode": "cannotreply",
                "message": "El hilo está cerrado"
            }]
        })))
        .up_to_n_times(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_add_discussion_post",
        ))
        .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable"))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    // Mensaje vacío -> invalid_arguments
    let res_empty = client
        .call_tool(
            CallToolRequestParams::new("forum_reply").with_arguments(
                json!({
                    "postid": 204,
                    "subject": "Asunto",
                    "message": "   ",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("empty message");
    assert!(res_empty.is_error.unwrap_or(false));
    assert_eq!(
        parse_tool_json(&res_empty)["error"]["kind"],
        "invalid_arguments"
    );

    // Warnings -> api error
    let res_warn = client
        .call_tool(
            CallToolRequestParams::new("forum_reply").with_arguments(
                json!({
                    "postid": 204,
                    "subject": "Asunto",
                    "message": "Respuesta",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("warnings");
    assert!(res_warn.is_error.unwrap_or(false));
    let val_warn = parse_tool_json(&res_warn);
    assert_eq!(val_warn["error"]["kind"], "api");
    assert!(val_warn["error"]["message"]
        .as_str()
        .unwrap()
        .contains("El hilo está cerrado"));

    // 503 -> maintenance con exactamente 1 intento
    let res_503 = client
        .call_tool(
            CallToolRequestParams::new("forum_reply").with_arguments(
                json!({
                    "postid": 204,
                    "subject": "Asunto",
                    "message": "Respuesta",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("503");
    assert!(res_503.is_error.unwrap_or(false));
    assert_eq!(parse_tool_json(&res_503)["error"]["kind"], "maintenance");
}

#[tokio::test]
async fn test_forum_new_discussion_without_messageformat_cmid_and_forumid() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_course_get_course_module",
        ))
        .and(body_string_contains("cmid=980101"))
        .respond_with(ResponseTemplate::new(200).set_body_string(cmid_response(
            980101,
            89,
            8233,
            "forum",
            "Foro de Consultas",
        )))
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_course_get_course_module_by_instance",
        ))
        .and(body_string_contains("module=forum"))
        .and(body_string_contains("instance=89"))
        .respond_with(ResponseTemplate::new(200).set_body_string(cmid_response(
            980101,
            89,
            8233,
            "forum",
            "Foro de Consultas",
        )))
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/upload.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "itemid": 445566,
                "filename": "guia.md"
            }
        ])))
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains("wsfunction=mod_forum_add_discussion"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "discussionid": 310,
            "warnings": []
        })))
        .expect(2)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    // 1. Por cmid, sin groupid (usa 0) y sin adjuntos
    let res1 = client
        .call_tool(
            CallToolRequestParams::new("forum_new_discussion").with_arguments(
                json!({
                    "cmid": 980101,
                    "subject": "Nueva consulta general",
                    "message": "Hola, tengo una **consulta**.",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("forum_new_discussion cmid");
    assert!(!res1.is_error.unwrap_or(false));
    let val1 = parse_tool_json(&res1);
    assert_eq!(val1["cmid"], 980101);
    assert_eq!(val1["instance"], 89);
    assert_eq!(val1["courseid"], 8233);
    assert_eq!(val1["discussionid"], 310);

    // 2. Por forumid, con groupid explícito y con adjunto
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"# Guia\nDetalle").unwrap();
    tmp.flush().unwrap();
    let tmp_path = tmp.path().to_string_lossy().to_string();

    let res2 = client
        .call_tool(
            CallToolRequestParams::new("forum_new_discussion").with_arguments(
                json!({
                    "forumid": 89,
                    "subject": "Consulta con archivo",
                    "message": "Adjunto guía.",
                    "groupid": 3,
                    "attachments": [tmp_path],
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("forum_new_discussion forumid");
    assert!(!res2.is_error.unwrap_or(false));

    // Inspeccionar ambas peticiones recibidas a mod_forum_add_discussion
    let requests = mock.received_requests().await.unwrap();
    let add_reqs: Vec<_> = requests
        .iter()
        .filter(|r| {
            let b = String::from_utf8_lossy(&r.body);
            b.contains("wsfunction=mod_forum_add_discussion&")
                || b.ends_with("wsfunction=mod_forum_add_discussion")
        })
        .collect();
    assert_eq!(add_reqs.len(), 2);

    for (idx, req) in add_reqs.iter().enumerate() {
        let body_str = String::from_utf8_lossy(&req.body);
        let parsed: HashMap<String, String> = url::form_urlencoded::parse(body_str.as_bytes())
            .into_owned()
            .collect();

        // CRÍTICO: Moodle 4.5 no acepta messageformat en mod_forum_add_discussion
        assert!(
            !parsed.contains_key("messageformat"),
            "mod_forum_add_discussion no debe enviar messageformat"
        );
        assert_eq!(parsed["forumid"], "89");
        if idx == 0 {
            assert_eq!(parsed["groupid"], "0");
            assert!(parsed["message"].contains("<strong>consulta</strong>"));
            assert!(!parsed.contains_key("options[0][name]"));
        } else {
            assert_eq!(parsed["groupid"], "3");
            assert_eq!(parsed["options[0][name]"], "attachmentsid");
            assert_eq!(parsed["options[0][value]"], "445566");
        }
    }
}

#[tokio::test]
async fn test_forum_new_discussion_validation_warnings_and_503() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_course_get_course_module",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(cmid_response(
            980101,
            89,
            8233,
            "forum",
            "Foro de Consultas",
        )))
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains("wsfunction=mod_forum_add_discussion"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "discussionid": 0,
            "warnings": [{
                "item": "forum",
                "itemid": 89,
                "warningcode": "cannotcreatediscussion",
                "message": "No tienes permisos para abrir discusiones en este foro"
            }]
        })))
        .up_to_n_times(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains("wsfunction=mod_forum_add_discussion"))
        .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable"))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    // 1. Asunto vacío
    let res_subj = client
        .call_tool(
            CallToolRequestParams::new("forum_new_discussion").with_arguments(
                json!({
                    "cmid": 980101,
                    "subject": "   ",
                    "message": "Contenido válido",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("empty subject");
    assert_eq!(
        parse_tool_json(&res_subj)["error"]["kind"],
        "invalid_arguments"
    );

    // 2. Mensaje vacío
    let res_msg = client
        .call_tool(
            CallToolRequestParams::new("forum_new_discussion").with_arguments(
                json!({
                    "cmid": 980101,
                    "subject": "Asunto válido",
                    "message": "",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("empty message");
    assert_eq!(
        parse_tool_json(&res_msg)["error"]["kind"],
        "invalid_arguments"
    );

    // 3. Faltan cmid y forumid
    let res_none = client
        .call_tool(
            CallToolRequestParams::new("forum_new_discussion").with_arguments(
                json!({
                    "subject": "Asunto válido",
                    "message": "Contenido válido",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("missing module ids");
    assert_eq!(
        parse_tool_json(&res_none)["error"]["kind"],
        "invalid_arguments"
    );

    // 4. Warnings -> api error
    let res_warn = client
        .call_tool(
            CallToolRequestParams::new("forum_new_discussion").with_arguments(
                json!({
                    "cmid": 980101,
                    "subject": "Asunto válido",
                    "message": "Contenido válido",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("warnings");
    assert!(res_warn.is_error.unwrap_or(false));
    let val_warn = parse_tool_json(&res_warn);
    assert_eq!(val_warn["error"]["kind"], "api");
    assert!(val_warn["error"]["message"]
        .as_str()
        .unwrap()
        .contains("No tienes permisos para abrir discusiones"));

    // 5. 503 sin reintentos
    let res_503 = client
        .call_tool(
            CallToolRequestParams::new("forum_new_discussion").with_arguments(
                json!({
                    "cmid": 980101,
                    "subject": "Asunto válido",
                    "message": "Contenido válido",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("503");
    assert!(res_503.is_error.unwrap_or(false));
    assert_eq!(parse_tool_json(&res_503)["error"]["kind"], "maintenance");
}

#[tokio::test]
async fn test_list_conversations_sends_userid_and_converts_markdown() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_message_get_conversations",
        ))
        .and(body_string_contains("userid=10001"))
        .and(body_string_contains("limitnum=20"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_CONVERSATIONS))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("list_conversations")
                .with_arguments(json!({}).as_object().unwrap().clone()),
        )
        .await
        .expect("list_conversations");

    assert!(!res.is_error.unwrap_or(false));
    let val = parse_tool_json(&res);

    assert_eq!(val["userid"], 10001);
    let convs = val["conversations"].as_array().unwrap();
    assert_eq!(convs.len(), 1);
    assert_eq!(convs[0]["id"], 305);
    assert_eq!(convs[0]["unreadcount"], 1);
    assert_eq!(convs[0]["members"][0]["fullname"], "Docente Ejemplo");
    assert!(convs[0]["messages"][0]["text_markdown"]
        .as_str()
        .unwrap()
        .contains("**informe**"));
    assert_eq!(convs[0]["messages"][0]["timecreated"], 1700000500);
    assert!(convs[0]["messages"][0]["timecreated_iso"]
        .as_str()
        .unwrap()
        .contains('T'));
}

#[tokio::test]
async fn test_get_conversation_sends_params_and_newest_one() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_message_get_conversation_messages",
        ))
        .and(body_string_contains("currentuserid=10001"))
        .and(body_string_contains("convid=305"))
        .and(body_string_contains("limitnum=50"))
        .and(body_string_contains("newest=1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_CONV_MESSAGES))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("get_conversation").with_arguments(
                json!({ "conversationid": 305 })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("get_conversation");

    assert!(!res.is_error.unwrap_or(false));
    let val = parse_tool_json(&res);

    assert_eq!(val["conversationid"], 305);
    assert_eq!(val["members"].as_array().unwrap().len(), 2);
    let msgs = val["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["id"], 9002);
    assert_eq!(msgs[0]["useridfrom"], 10001);
    assert!(msgs[0]["text_markdown"]
        .as_str()
        .unwrap()
        .contains("hoy por la tarde"));
    assert_eq!(msgs[0]["timecreated"], 1700000600);
    assert!(msgs[0]["timecreated_iso"].as_str().unwrap().contains('T'));
    assert!(msgs[1]["text_markdown"]
        .as_str()
        .unwrap()
        .contains("**informe**"));
}

#[tokio::test]
async fn test_send_message_by_conversation_and_touserid_and_validations() {
    let mock = MockServer::start().await;

    // 1. Por conversationid
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_message_send_messages_to_conversation",
        ))
        .and(body_string_contains("conversationid=305"))
        .and(body_string_contains("messages%5B0%5D%5Btextformat%5D=1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": 9100,
                "useridfrom": 10001,
                "text": "<p>Hola en <strong>conversación</strong></p>",
                "timecreated": 1700001000,
                "conversationid": 305
            }
        ])))
        .expect(1)
        .mount(&mock)
        .await;

    // 2. Por touserid (primero éxito, luego error lógico errormessage)
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_message_send_instant_messages",
        ))
        .and(body_string_contains("messages%5B0%5D%5Btouserid%5D=10002"))
        .and(body_string_contains("messages%5B0%5D%5Btextformat%5D=1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "msgid": 9101,
                "errormessage": null,
                "text": "<p>Mensaje <em>directo</em></p>",
                "timecreated": 1700001010,
                "conversationid": 306,
                "useridfrom": 10001
            }
        ])))
        .up_to_n_times(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_message_send_instant_messages",
        ))
        .and(body_string_contains("messages%5B0%5D%5Btouserid%5D=10003"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "msgid": -1,
                "errormessage": "No tienes permiso para contactar a este usuario"
            }
        ])))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    // 1. Por conversationid
    let res_conv = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({
                    "conversationid": 305,
                    "text": "Hola en **conversación**",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("send_message conversationid");
    assert!(!res_conv.is_error.unwrap_or(false));
    let val_conv = parse_tool_json(&res_conv);
    assert_eq!(val_conv["sent"], true);
    assert_eq!(val_conv["conversationid"], 305);
    assert_eq!(val_conv["messageid"], 9100);
    assert!(val_conv["text_markdown"]
        .as_str()
        .unwrap()
        .contains("**conversación**"));

    // 2. Por touserid (éxito)
    let res_user = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({
                    "touserid": 10002,
                    "text": "Mensaje *directo*",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("send_message touserid");
    assert!(!res_user.is_error.unwrap_or(false));
    let val_user = parse_tool_json(&res_user);
    assert_eq!(val_user["sent"], true);
    assert_eq!(val_user["touserid"], 10002);
    assert_eq!(val_user["messageid"], 9101);
    assert_eq!(val_user["conversationid"], 306);

    // 3. Por touserid con errormessage -> error api
    let res_err = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({
                    "touserid": 10003,
                    "text": "Hola",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("send_message errormessage");
    assert!(res_err.is_error.unwrap_or(false));
    let val_err = parse_tool_json(&res_err);
    assert_eq!(val_err["error"]["kind"], "api");
    assert!(val_err["error"]["message"]
        .as_str()
        .unwrap()
        .contains("No tienes permiso para contactar a este usuario"));

    // 4. Ambos conversationid y touserid -> invalid_arguments
    let res_both = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({
                    "conversationid": 305,
                    "touserid": 10002,
                    "text": "Hola",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("send_message both");
    assert!(res_both.is_error.unwrap_or(false));
    assert_eq!(
        parse_tool_json(&res_both)["error"]["kind"],
        "invalid_arguments"
    );

    // 5. Ninguno -> invalid_arguments
    let res_none = client
        .call_tool(
            CallToolRequestParams::new("send_message")
                .with_arguments(json!({ "text": "Hola" }).as_object().unwrap().clone()),
        )
        .await
        .expect("send_message neither");
    assert!(res_none.is_error.unwrap_or(false));
    assert_eq!(
        parse_tool_json(&res_none)["error"]["kind"],
        "invalid_arguments"
    );

    // 6. Texto vacío -> invalid_arguments
    let res_empty = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({
                    "conversationid": 305,
                    "text": "   \n ",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("send_message empty");
    assert!(res_empty.is_error.unwrap_or(false));
    assert_eq!(
        parse_tool_json(&res_empty)["error"]["kind"],
        "invalid_arguments"
    );
}

#[tokio::test]
async fn test_send_message_no_retries_on_503() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_message_send_messages_to_conversation",
        ))
        .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable"))
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_message_send_instant_messages",
        ))
        .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable"))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res1 = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({ "conversationid": 305, "text": "Hola" })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("503 conversation");
    assert_eq!(parse_tool_json(&res1)["error"]["kind"], "maintenance");

    let res2 = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({ "touserid": 10002, "text": "Hola" })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("503 instant");
    assert_eq!(parse_tool_json(&res2)["error"]["kind"], "maintenance");
}

#[tokio::test]
async fn test_forums_and_messaging_scrub_tokens_and_handle_invalid_token() {
    let mock = MockServer::start().await;

    // Respuesta con tokens filtrados en URL/texto (deben sanearse a <token>)
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_discussion_posts",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "posts": [{
                "id": 999,
                "parentid": 0,
                "subject": "Prueba tok123",
                "message": "<p>Texto con priv123</p>",
                "author": { "fullname": "Autor" },
                "timecreated": 1700000100,
                "attachments": [{
                    "filename": "doc.pdf",
                    "filesize": 123,
                    "url": "https://moodle.example/pluginfile.php/1/doc.pdf?token=tok123",
                    "mimetype": "application/pdf"
                }]
            }]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    // Error de API conteniendo token (también debe sanearse)
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_message_send_instant_messages",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "msgid": -1,
            "errormessage": "Fallo interno con token tok123 y priv123"
        }])))
        .expect(1)
        .mount(&mock)
        .await;

    // InvalidToken en list_conversations
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_message_get_conversations",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_INVALID_TOKEN))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    // 1. Scrub en salida exitosa
    let res_ok = client
        .call_tool(
            CallToolRequestParams::new("forum_get_discussion")
                .with_arguments(json!({ "discussionid": 999 }).as_object().unwrap().clone()),
        )
        .await
        .expect("scrub ok");
    let raw_ok = &res_ok.content[0].as_text().unwrap().text;
    assert!(!raw_ok.contains("tok123"));
    assert!(!raw_ok.contains("priv123"));
    assert!(raw_ok.contains("<token>"));

    // 2. Scrub en salida de error
    let res_err = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({ "touserid": 10002, "text": "Hola" })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("scrub err");
    assert!(res_err.is_error.unwrap_or(false));
    let raw_err = &res_err.content[0].as_text().unwrap().text;
    assert!(!raw_err.contains("tok123"));
    assert!(!raw_err.contains("priv123"));
    assert!(raw_err.contains("<token>"));

    // 3. InvalidToken -> kind = invalid_token
    let res_inv = client
        .call_tool(
            CallToolRequestParams::new("list_conversations")
                .with_arguments(json!({}).as_object().unwrap().clone()),
        )
        .await
        .expect("invalid token");
    assert!(res_inv.is_error.unwrap_or(false));
    assert_eq!(parse_tool_json(&res_inv)["error"]["kind"], "invalid_token");
}

#[tokio::test]
async fn test_forum_list_edge_cases_empty_nulls_and_warnings() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_forums_by_courses",
        ))
        .and(body_string_contains("courseids%5B0%5D=1001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_forums_by_courses",
        ))
        .and(body_string_contains("courseids%5B0%5D=1002"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": 91,
                "coursemodule": 55501,
                "name": null,
                "type": null,
                "intro": null
            }
        ])))
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_forums_by_courses",
        ))
        .and(body_string_contains("courseids%5B0%5D=1003"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "warnings": [{ "warningcode": "noaccess", "message": "Sin acceso al curso" }]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res_empty = client
        .call_tool(
            CallToolRequestParams::new("forum_list")
                .with_arguments(json!({ "courseid": 1001 }).as_object().unwrap().clone()),
        )
        .await
        .expect("forum_list empty");
    assert!(!res_empty.is_error.unwrap_or(false));
    let val_empty = parse_tool_json(&res_empty);
    assert_eq!(val_empty["forums"].as_array().unwrap().len(), 0);

    let res_nulls = client
        .call_tool(
            CallToolRequestParams::new("forum_list")
                .with_arguments(json!({ "courseid": 1002 }).as_object().unwrap().clone()),
        )
        .await
        .expect("forum_list nulls");
    assert!(!res_nulls.is_error.unwrap_or(false));
    let val_nulls = parse_tool_json(&res_nulls);
    assert_eq!(val_nulls["forums"][0]["cmid"], 55501);
    assert_eq!(val_nulls["forums"][0]["course"], 1002);
    assert_eq!(val_nulls["forums"][0]["name"], "");
    assert_eq!(val_nulls["forums"][0]["type"], "");
    assert_eq!(val_nulls["forums"][0]["duedate"], 0);

    let res_warn = client
        .call_tool(
            CallToolRequestParams::new("forum_list")
                .with_arguments(json!({ "courseid": 1003 }).as_object().unwrap().clone()),
        )
        .await
        .expect("forum_list warnings");
    assert!(res_warn.is_error.unwrap_or(false));
    let val_warn = parse_tool_json(&res_warn);
    assert_eq!(val_warn["error"]["kind"], "api");
    assert!(val_warn["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Sin acceso al curso"));
}

#[tokio::test]
async fn test_forum_list_discussions_nulls_numeric_booleans_and_warnings() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_course_get_course_module_by_instance",
        ))
        .and(body_string_contains("instance=92"))
        .respond_with(ResponseTemplate::new(200).set_body_string(cmid_response(
            980200,
            92,
            8233,
            "forum",
            "Foro con variantes",
        )))
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_forum_discussions",
        ))
        .and(body_string_contains("forumid=92"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "discussions": [
                {
                    "id": 701,
                    "name": null,
                    "subject": "",
                    "modified": 1700001234,
                    "pinned": 1,
                    "locked": 0
                }
            ],
            "warnings": [{ "warningcode": "notice", "message": "Aviso parcial" }]
        })))
        .up_to_n_times(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_forum_discussions",
        ))
        .and(body_string_contains("forumid=92"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "discussions": null,
            "warnings": null
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res1 = client
        .call_tool(
            CallToolRequestParams::new("forum_list_discussions")
                .with_arguments(json!({ "forumid": 92 }).as_object().unwrap().clone()),
        )
        .await
        .expect("numeric booleans");
    assert!(!res1.is_error.unwrap_or(false));
    let val1 = parse_tool_json(&res1);
    assert_eq!(val1["discussions"][0]["pinned"], true);
    assert_eq!(val1["discussions"][0]["locked"], false);
    assert_eq!(val1["discussions"][0]["timemodified"], 1700001234);
    assert_eq!(val1["warnings"].as_array().unwrap().len(), 1);

    let res2 = client
        .call_tool(
            CallToolRequestParams::new("forum_list_discussions")
                .with_arguments(json!({ "forumid": 92 }).as_object().unwrap().clone()),
        )
        .await
        .expect("null discussions and warnings");
    assert!(!res2.is_error.unwrap_or(false));
    let val2 = parse_tool_json(&res2);
    assert_eq!(val2["discussions"].as_array().unwrap().len(), 0);
    assert_eq!(val2["warnings"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_forum_get_discussion_empty_posts_and_missing_mimetype() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_discussion_posts",
        ))
        .and(body_string_contains("discussionid=5001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "posts": []
        })))
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_discussion_posts",
        ))
        .and(body_string_contains("discussionid=5002"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "posts": [{
                "id": 801,
                "parentid": null,
                "author": null,
                "subject": null,
                "message": null,
                "timecreated": 0,
                "attachments": [{
                    "filename": "sin_mime.bin",
                    "filesize": 512,
                    "url": "https://moodle.example/pluginfile.php/10/sin_mime.bin"
                }]
            }]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res_empty = client
        .call_tool(
            CallToolRequestParams::new("forum_get_discussion")
                .with_arguments(json!({ "discussionid": 5001 }).as_object().unwrap().clone()),
        )
        .await
        .expect("empty posts");
    assert!(!res_empty.is_error.unwrap_or(false));
    assert_eq!(
        parse_tool_json(&res_empty)["posts"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let res_null_mime = client
        .call_tool(
            CallToolRequestParams::new("forum_get_discussion")
                .with_arguments(json!({ "discussionid": 5002 }).as_object().unwrap().clone()),
        )
        .await
        .expect("missing mimetype");
    assert!(!res_null_mime.is_error.unwrap_or(false));
    let val = parse_tool_json(&res_null_mime);
    let post = &val["posts"][0];
    assert_eq!(post["postid"], 801);
    assert_eq!(post["parentid"], 0);
    assert_eq!(post["author"], "");
    assert_eq!(post["subject"], "");
    assert_eq!(post["timecreated"], 0);
    assert!(post["timecreated_iso"].is_null());
    assert_eq!(
        post["attachments"][0]["fileurl"],
        "https://moodle.example/pluginfile.php/10/sin_mime.bin"
    );
    assert!(post["attachments"][0]["mimetype"].is_null());
}

#[tokio::test]
async fn test_forum_reply_validates_attachments_before_reading_parent_and_nested_post_id() {
    let mock = MockServer::start().await;

    // Cuando el archivo adjunto no existe, NO debe llamarse mod_forum_get_discussion_post
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_discussion_post",
        ))
        .and(body_string_contains("postid=777"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(0)
        .mount(&mock)
        .await;

    // Warnings al consultar postid 778 -> error api
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_get_discussion_post",
        ))
        .and(body_string_contains("postid=778"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "warnings": [{ "warningcode": "nopost", "message": "Mensaje padre no encontrado" }]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    // Fallback a resp.post.id cuando falta postid raíz
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_forum_add_discussion_post",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "post": { "id": 9901 },
            "warnings": []
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res_missing = client
        .call_tool(
            CallToolRequestParams::new("forum_reply").with_arguments(
                json!({
                    "postid": 777,
                    "message": "Respuesta con adjunto inválido",
                    "attachments": ["/ruta/inexistente/archivo_99999.pdf"],
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("missing attachment without subject");
    assert!(res_missing.is_error.unwrap_or(false));
    assert_eq!(
        parse_tool_json(&res_missing)["error"]["kind"],
        "invalid_arguments"
    );

    let res_warn = client
        .call_tool(
            CallToolRequestParams::new("forum_reply").with_arguments(
                json!({
                    "postid": 778,
                    "message": "Respuesta sin asunto",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("parent post warning");
    assert!(res_warn.is_error.unwrap_or(false));
    let val_warn = parse_tool_json(&res_warn);
    assert_eq!(val_warn["error"]["kind"], "api");
    assert!(val_warn["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Mensaje padre no encontrado"));

    let res_nested = client
        .call_tool(
            CallToolRequestParams::new("forum_reply").with_arguments(
                json!({
                    "postid": 779,
                    "subject": "Asunto directo",
                    "message": "Mensaje",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("nested post id");
    assert!(!res_nested.is_error.unwrap_or(false));
    assert_eq!(parse_tool_json(&res_nested)["postid"], 9901);
}

#[tokio::test]
async fn test_forum_new_discussion_validates_attachments_early_and_wrong_modname() {
    let mock = MockServer::start().await;

    // Si hay archivo inexistente, no debe llamarse resolve_module para cmid=980101
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains("cmid=980101"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(0)
        .mount(&mock)
        .await;

    // Módulo assign en lugar de forum -> invalid_arguments
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_course_get_course_module",
        ))
        .and(body_string_contains("cmid=979333"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(cmid_response(979333, 68822, 8233, "assign", "Tarea")),
        )
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/upload.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(0)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res_missing = client
        .call_tool(
            CallToolRequestParams::new("forum_new_discussion").with_arguments(
                json!({
                    "cmid": 980101,
                    "subject": "Tema",
                    "message": "Contenido",
                    "attachments": ["/ruta/inexistente/no_existe_new_disc.pdf"],
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("missing attachment new discussion");
    assert!(res_missing.is_error.unwrap_or(false));
    assert_eq!(
        parse_tool_json(&res_missing)["error"]["kind"],
        "invalid_arguments"
    );

    let res_wrong_mod = client
        .call_tool(
            CallToolRequestParams::new("forum_new_discussion").with_arguments(
                json!({
                    "cmid": 979333,
                    "subject": "Tema",
                    "message": "Contenido",
                    "attachments": [],
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("wrong modname new discussion");
    assert!(res_wrong_mod.is_error.unwrap_or(false));
    assert_eq!(
        parse_tool_json(&res_wrong_mod)["error"]["kind"],
        "invalid_arguments"
    );
}

#[tokio::test]
async fn test_conversations_edge_cases_empty_nulls_and_lastmessage_markdown() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_message_get_conversations",
        ))
        .and(body_string_contains("limitnum=5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "conversations": [{
                "id": 401,
                "name": null,
                "members": [{ "id": 10002, "fullname": null, "isonline": null, "isblocked": 0 }],
                "messages": null,
                "lastmessage": "<p>Último <strong>aviso</strong></p>"
            }]
        })))
        .up_to_n_times(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_message_get_conversations",
        ))
        .and(body_string_contains("limitnum=2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "conversations": []
        })))
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_message_get_conversation_messages",
        ))
        .and(body_string_contains("convid=401"))
        .and(body_string_contains("limitnum=10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "members": null,
            "messages": []
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res_list = client
        .call_tool(
            CallToolRequestParams::new("list_conversations")
                .with_arguments(json!({ "limit": 5 }).as_object().unwrap().clone()),
        )
        .await
        .expect("list_conversations custom limit");
    assert!(!res_list.is_error.unwrap_or(false));
    let val_list = parse_tool_json(&res_list);
    assert_eq!(val_list["conversations"][0]["id"], 401);
    assert!(val_list["conversations"][0]["name"].is_null());
    assert_eq!(val_list["conversations"][0]["members"][0]["fullname"], "");
    assert_eq!(
        val_list["conversations"][0]["members"][0]["isblocked"],
        false
    );
    assert!(val_list["conversations"][0]["lastmessage_markdown"]
        .as_str()
        .unwrap()
        .contains("**aviso**"));

    let res_empty_convs = client
        .call_tool(
            CallToolRequestParams::new("list_conversations")
                .with_arguments(json!({ "limit": 2 }).as_object().unwrap().clone()),
        )
        .await
        .expect("list_conversations empty");
    assert!(!res_empty_convs.is_error.unwrap_or(false));
    assert_eq!(
        parse_tool_json(&res_empty_convs)["conversations"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let res_get = client
        .call_tool(
            CallToolRequestParams::new("get_conversation").with_arguments(
                json!({ "conversationid": 401, "limit": 10 })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("get_conversation empty");
    assert!(!res_get.is_error.unwrap_or(false));
    let val_get = parse_tool_json(&res_get);
    assert_eq!(val_get["members"].as_array().unwrap().len(), 0);
    assert_eq!(val_get["messages"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_send_message_rejects_empty_array_and_negative_msgid_sentinel() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_message_send_messages_to_conversation",
        ))
        .and(body_string_contains("conversationid=305"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_message_send_messages_to_conversation",
        ))
        .and(body_string_contains("conversationid=306"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "warnings": [{ "warningcode": "notfound", "message": "Conversación no encontrada" }]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_message_send_instant_messages",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "msgid": -1, "errormessage": null }
        ])))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res_empty = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({ "conversationid": 305, "text": "Hola" })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("empty array send_message");
    assert!(res_empty.is_error.unwrap_or(false));
    assert_eq!(parse_tool_json(&res_empty)["error"]["kind"], "protocol");

    let res_warn = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({ "conversationid": 306, "text": "Hola" })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("warnings send_message");
    assert!(res_warn.is_error.unwrap_or(false));
    assert_eq!(parse_tool_json(&res_warn)["error"]["kind"], "api");

    let res_sentinel = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({ "touserid": 10004, "text": "Hola" })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("sentinel msgid -1");
    assert!(res_sentinel.is_error.unwrap_or(false));
    assert_eq!(parse_tool_json(&res_sentinel)["error"]["kind"], "api");
}
