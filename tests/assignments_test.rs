mod common;

use common::start_server;
use rmcp::model::CallToolRequestParams;
use serde_json::json;
use std::collections::HashMap;
use std::io::Write;
use tempfile::NamedTempFile;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURE_ASSIGNMENTS: &str = include_str!("fixtures/assignments.json");
const FIXTURE_SUBMISSION_STATUS: &str = include_str!("fixtures/submission_status.json");

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

#[tokio::test]
async fn test_assignments_tools_annotations() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    let tools = client.list_all_tools().await.expect("list_all_tools");
    let tool_map: HashMap<String, _> = tools.into_iter().map(|t| (t.name.to_string(), t)).collect();

    // 1. get_assignment: read_only_hint = true
    let get_assign = tool_map
        .get("get_assignment")
        .expect("get_assignment debe estar registrada");
    let get_ann = get_assign.annotations.as_ref().expect("annotations");
    assert_eq!(get_ann.read_only_hint, Some(true));

    // 2. submit_assignment: destructive_hint = true
    let submit_assign = tool_map
        .get("submit_assignment")
        .expect("submit_assignment debe estar registrada");
    let submit_ann = submit_assign.annotations.as_ref().expect("annotations");
    assert_eq!(submit_ann.destructive_hint, Some(true));
}

#[tokio::test]
async fn test_get_assignment_by_cmid_and_by_assignid_identical() {
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
            "assign",
            "Laboratorio JavaFX",
        )))
        .mount(&mock)
        .await;

    // 2. Resolver por assignid
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_course_get_course_module_by_instance",
        ))
        .and(body_string_contains("module=assign"))
        .and(body_string_contains("instance=68822"))
        .respond_with(ResponseTemplate::new(200).set_body_string(cmid_response(
            979333,
            68822,
            8233,
            "assign",
            "Laboratorio JavaFX",
        )))
        .mount(&mock)
        .await;

    // 3. mod_assign_get_assignments
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_get_assignments",
        ))
        .and(body_string_contains("courseids%5B0%5D=8233"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_ASSIGNMENTS))
        .mount(&mock)
        .await;

    // 4. mod_assign_get_submission_status
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_get_submission_status",
        ))
        .and(body_string_contains("assignid=68822"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_SUBMISSION_STATUS))
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    // Llamada por cmid
    let res_cmid = client
        .call_tool(
            CallToolRequestParams::new("get_assignment")
                .with_arguments(json!({ "cmid": 979333 }).as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool por cmid");
    assert!(!res_cmid.is_error.unwrap_or(false));
    let text_cmid = res_cmid.content[0].as_text().unwrap();
    let val_cmid: serde_json::Value = serde_json::from_str(&text_cmid.text).unwrap();

    // Llamada por assignid
    let res_assignid = client
        .call_tool(
            CallToolRequestParams::new("get_assignment")
                .with_arguments(json!({ "assignid": 68822 }).as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool por assignid");
    assert!(!res_assignid.is_error.unwrap_or(false));
    let text_assignid = res_assignid.content[0].as_text().unwrap();
    let val_assignid: serde_json::Value = serde_json::from_str(&text_assignid.text).unwrap();

    // Verificar identidad completa de salida
    assert_eq!(val_cmid, val_assignid);

    // Verificar campos mapeados correctamente
    assert_eq!(val_cmid["cmid"], 979333);
    assert_eq!(val_cmid["instance"], 68822);
    assert_eq!(val_cmid["courseid"], 8233);
    assert_eq!(
        val_cmid["name"],
        "Laboratorio | Construcción de interfaz gráfica funcional"
    );
    assert!(val_cmid["intro_markdown"]
        .as_str()
        .unwrap()
        .contains("Evaluación sistemática"));
    assert_eq!(val_cmid["submission_types"]["onlinetext"]["enabled"], true);
    assert_eq!(val_cmid["status"]["status"], "submitted");
    assert_eq!(val_cmid["grade"]["grade"], "30 / 30");
    assert_eq!(val_cmid["grade"]["grader"], 20001);
}

#[tokio::test]
async fn test_get_assignment_both_or_neither_cmid_assignid_returns_invalid_arguments() {
    let mock = MockServer::start().await;
    let client = start_server(&mock).await;

    // 1. Ambos parámetros presentes
    let res_both = client
        .call_tool(
            CallToolRequestParams::new("get_assignment").with_arguments(
                json!({ "cmid": 979333, "assignid": 68822 })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("call_tool both");
    assert!(res_both.is_error.unwrap_or(false));
    let text_both = res_both.content[0].as_text().unwrap();
    let val_both: serde_json::Value = serde_json::from_str(&text_both.text).unwrap();
    assert_eq!(val_both["error"]["kind"], "invalid_arguments");

    // 2. Ningún parámetro presente
    let res_neither = client
        .call_tool(
            CallToolRequestParams::new("get_assignment")
                .with_arguments(json!({}).as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool neither");
    assert!(res_neither.is_error.unwrap_or(false));
    let text_neither = res_neither.content[0].as_text().unwrap();
    let val_neither: serde_json::Value = serde_json::from_str(&text_neither.text).unwrap();
    assert_eq!(val_neither["error"]["kind"], "invalid_arguments");
}

#[tokio::test]
async fn test_submit_assignment_file_only_saves_without_grading() {
    let mock = MockServer::start().await;

    // 1. Resolver módulo
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

    // 2. Subida de archivo a upload.php
    Mock::given(method("POST"))
        .and(path("/webservice/upload.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "itemid": 555666,
                "filename": "entrega.pdf"
            }
        ])))
        .expect(1)
        .mount(&mock)
        .await;

    // 3. mod_assign_save_submission (sin submit_for_grading)
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_save_submission",
        ))
        .and(body_string_contains("assignmentid=68822"))
        .and(body_string_contains(
            "plugindata%5Bfiles_filemanager%5D=555666",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&mock)
        .await;

    // 4. mod_assign_submit_for_grading NO debe ser invocada
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_submit_for_grading",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(0)
        .mount(&mock)
        .await;

    // 5. Consulta final de estado
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_get_submission_status",
        ))
        .and(body_string_contains("assignid=68822"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_SUBMISSION_STATUS))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"PDF bytes de prueba").unwrap();
    tmp.flush().unwrap();
    let tmp_path = tmp.path().to_string_lossy().to_string();

    let res = client
        .call_tool(
            CallToolRequestParams::new("submit_assignment").with_arguments(
                json!({
                    "cmid": 979333,
                    "files": [tmp_path],
                    "final": false,
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("call_tool submit_assignment");

    assert!(!res.is_error.unwrap_or(false));
    let text = res.content[0].as_text().unwrap();
    let val: serde_json::Value = serde_json::from_str(&text.text).unwrap();
    assert_eq!(val["status"], "submitted");
}

#[tokio::test]
async fn test_submit_assignment_final_with_submissiondrafts_invokes_submit_for_grading() {
    let mock = MockServer::start().await;

    // 1. Resolver módulo
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

    // 2. core_files_get_unused_draft_itemid para texto en línea
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=core_files_get_unused_draft_itemid",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": 112233 })))
        .expect(1)
        .mount(&mock)
        .await;

    // 3. mod_assign_save_submission
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_save_submission",
        ))
        .and(body_string_contains("assignmentid=68822"))
        .and(body_string_contains(
            "plugindata%5Bonlinetext_editor%5D%5Bitemid%5D=112233",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&mock)
        .await;

    // 4. mod_assign_get_assignments con submissiondrafts = 1
    let assignments_with_drafts = json!({
        "courses": [{
            "id": 8233,
            "assignments": [{
                "id": 68822,
                "cmid": 979333,
                "course": 8233,
                "name": "Laboratorio JavaFX",
                "submissiondrafts": 1,
                "configs": []
            }]
        }]
    })
    .to_string();

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_get_assignments",
        ))
        .and(body_string_contains("courseids%5B0%5D=8233"))
        .respond_with(ResponseTemplate::new(200).set_body_string(assignments_with_drafts))
        .expect(1)
        .mount(&mock)
        .await;

    // 5. mod_assign_submit_for_grading con acceptsubmissionstatement = 1
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_submit_for_grading",
        ))
        .and(body_string_contains("assignmentid=68822"))
        .and(body_string_contains("acceptsubmissionstatement=1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&mock)
        .await;

    // 6. Consulta final de estado
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_get_submission_status",
        ))
        .and(body_string_contains("assignid=68822"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE_SUBMISSION_STATUS))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("submit_assignment").with_arguments(
                json!({
                    "cmid": 979333,
                    "onlinetext": "# Mi entrega\nSolución del ejercicio.",
                    "final": true,
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("call_tool submit_assignment final");

    assert!(!res.is_error.unwrap_or(false));
}

#[tokio::test]
async fn test_submit_assignment_save_submission_warnings_halts_and_returns_api_error() {
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
            "assign",
            "Laboratorio JavaFX",
        )))
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/upload.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "itemid": 555666, "filename": "entrega.pdf" }
        ])))
        .mount(&mock)
        .await;

    // mod_assign_save_submission devuelve warnings
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_save_submission",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "item": "submission",
                "itemid": 68822,
                "warningcode": "duedate_passed",
                "message": "La fecha de entrega de esta tarea ha vencido"
            }
        ])))
        .expect(1)
        .mount(&mock)
        .await;

    // submit_for_grading NO debe llamarse
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_submit_for_grading",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(0)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"PDF prueba").unwrap();
    tmp.flush().unwrap();
    let tmp_path = tmp.path().to_string_lossy().to_string();

    let res = client
        .call_tool(
            CallToolRequestParams::new("submit_assignment").with_arguments(
                json!({
                    "cmid": 979333,
                    "files": [tmp_path],
                    "final": true,
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("call_tool con warning");

    assert!(res.is_error.unwrap_or(false));
    let text = res.content[0].as_text().unwrap();
    let val: serde_json::Value = serde_json::from_str(&text.text).unwrap();
    assert_eq!(val["error"]["kind"], "api");
    assert!(val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("La fecha de entrega de esta tarea ha vencido"));
}

#[tokio::test]
async fn test_submit_assignment_keep_existing_files_downloads_and_reuploads_single_itemid() {
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
            "assign",
            "Laboratorio JavaFX",
        )))
        .mount(&mock)
        .await;

    // 1. Estado previo con un archivo existente
    let prev_file_url = format!(
        "{}/pluginfile.php/123/mod_assign/submission_files/previo.pdf",
        mock.uri()
    );
    let status_with_file = json!({
        "lastattempt": {
            "submission": {
                "id": 1,
                "status": "draft",
                "plugins": [{
                    "type": "file",
                    "fileareas": [{
                        "area": "submission_files",
                        "files": [{
                            "filename": "previo.pdf",
                            "fileurl": prev_file_url,
                            "filesize": 100,
                            "mimetype": "application/pdf"
                        }]
                    }]
                }]
            },
            "canedit": true,
            "cansubmit": true
        }
    })
    .to_string();

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_get_submission_status",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(status_with_file))
        .expect(2) // Una para ver archivos previos y otra al final
        .mount(&mock)
        .await;

    // 2. Descarga del archivo previo
    Mock::given(method("GET"))
        .and(path(
            "/webservice/pluginfile.php/123/mod_assign/submission_files/previo.pdf",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(b"Contenido de previo.pdf".to_vec())
                .insert_header("content-type", "application/pdf")
                .insert_header("content-disposition", "attachment; filename=\"previo.pdf\""),
        )
        .expect(1)
        .mount(&mock)
        .await;

    // 3. Subida conjunta a upload.php (ambos archivos en una sola llamada)
    Mock::given(method("POST"))
        .and(path("/webservice/upload.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "itemid": 999888, "filename": "previo.pdf" },
            { "itemid": 999888, "filename": "nuevo.pdf" }
        ])))
        .expect(1)
        .mount(&mock)
        .await;

    // 4. Guardar entrega con itemid conjunto
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_save_submission",
        ))
        .and(body_string_contains(
            "plugindata%5Bfiles_filemanager%5D=999888",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"Contenido nuevo").unwrap();
    tmp.flush().unwrap();
    let tmp_path = tmp.path().to_string_lossy().to_string();

    let res = client
        .call_tool(
            CallToolRequestParams::new("submit_assignment").with_arguments(
                json!({
                    "cmid": 979333,
                    "files": [tmp_path],
                    "keep_existing_files": true,
                    "final": false,
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("call_tool keep_existing_files");

    assert!(!res.is_error.unwrap_or(false));
}

#[tokio::test]
async fn test_submit_assignment_save_uses_write_client_no_retries_on_503() {
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
            "assign",
            "Laboratorio JavaFX",
        )))
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/webservice/upload.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "itemid": 555666, "filename": "entrega.pdf" }
        ])))
        .mount(&mock)
        .await;

    // mod_assign_save_submission responde 503; write_client tiene max_retries = 0, por lo que espera exactamente 1 petición
    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .and(body_string_contains(
            "wsfunction=mod_assign_save_submission",
        ))
        .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable"))
        .expect(1)
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"PDF prueba").unwrap();
    tmp.flush().unwrap();
    let tmp_path = tmp.path().to_string_lossy().to_string();

    let res = client
        .call_tool(
            CallToolRequestParams::new("submit_assignment").with_arguments(
                json!({
                    "cmid": 979333,
                    "files": [tmp_path],
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("call_tool 503");

    assert!(res.is_error.unwrap_or(false));
    let text = res.content[0].as_text().unwrap();
    let val: serde_json::Value = serde_json::from_str(&text.text).unwrap();
    assert_eq!(val["error"]["kind"], "maintenance");
}

#[tokio::test]
async fn test_submit_assignment_missing_files_and_onlinetext_returns_invalid_arguments() {
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
            "assign",
            "Laboratorio JavaFX",
        )))
        .mount(&mock)
        .await;

    let client = start_server(&mock).await;

    let res = client
        .call_tool(
            CallToolRequestParams::new("submit_assignment").with_arguments(
                json!({
                    "cmid": 979333,
                    "files": [],
                    "onlinetext": "   ",
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await
        .expect("call_tool invalid arguments");

    assert!(res.is_error.unwrap_or(false));
    let text = res.content[0].as_text().unwrap();
    let val: serde_json::Value = serde_json::from_str(&text.text).unwrap();
    assert_eq!(val["error"]["kind"], "invalid_arguments");
    assert!(val["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Debes proporcionar files o onlinetext"));
}
