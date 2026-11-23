use moodle_mcp::moodle::client::{MoodleClient, MoodleClientBuilder};
use moodle_mcp::moodle::error::MoodleError;
use moodle_mcp::moodle::gate::MoodleGate;
use moodle_mcp::moodle::ids::{resolve_module, CourseModuleRef};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_resolve_module_validation_rules() {
    let mock = MockServer::start().await;
    let gate = MoodleGate::new(4, 3).without_jitter();
    let client =
        MoodleClient::from_builder(MoodleClientBuilder::new().base_url(mock.uri()).gate(gate));

    // 1. Ambos None -> Error de argumentos sin llamada de red
    let err_both_none = resolve_module(&client, "tok", "assign", None, None).await;
    match err_both_none {
        Err(MoodleError::InvalidArguments(msg)) => {
            assert_eq!(
                msg,
                "Indica exactamente uno de cmid o el id de la actividad"
            );
        }
        _ => panic!("Esperaba InvalidArguments cuando ambos son None"),
    }

    // 2. Ambos Some -> Error de argumentos sin llamada de red
    let err_both_some = resolve_module(&client, "tok", "assign", Some(10), Some(20)).await;
    match err_both_some {
        Err(MoodleError::InvalidArguments(msg)) => {
            assert_eq!(
                msg,
                "Indica exactamente uno de cmid o el id de la actividad"
            );
        }
        _ => panic!("Esperaba InvalidArguments cuando ambos son Some"),
    }
}

#[tokio::test]
async fn test_resolve_module_by_cmid_success_and_mismatch() {
    let mock = MockServer::start().await;
    let gate = MoodleGate::new(4, 3).without_jitter();
    let client =
        MoodleClient::from_builder(MoodleClientBuilder::new().base_url(mock.uri()).gate(gate));

    let cm_payload = json!({
        "cm": {
            "id": 105,
            "course": 42,
            "module": 1,
            "name": "Entrega 1",
            "modname": "assign",
            "instance": 501
        }
    });

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&cm_payload))
        .mount(&mock)
        .await;

    // A. modname no coincide (se esperaba quiz pero es assign)
    let err_mismatch = resolve_module(&client, "tok", "quiz", Some(105), None).await;
    match err_mismatch {
        Err(MoodleError::InvalidArguments(msg)) => {
            assert_eq!(msg, "El módulo 105 es de tipo 'assign', no 'quiz'");
        }
        _ => panic!("Esperaba InvalidArguments por discrepancia de modname"),
    }

    // B. modname coincide
    let ok_ref = resolve_module(&client, "tok", "assign", Some(105), None)
        .await
        .expect("resolve_module exitoso");
    assert_eq!(
        ok_ref,
        CourseModuleRef {
            cmid: 105,
            instance: 501,
            course: 42,
            modname: "assign".to_string(),
            name: "Entrega 1".to_string(),
        }
    );
}

#[tokio::test]
async fn test_resolve_module_by_instance_success() {
    let mock = MockServer::start().await;
    let gate = MoodleGate::new(4, 3).without_jitter();
    let client =
        MoodleClient::from_builder(MoodleClientBuilder::new().base_url(mock.uri()).gate(gate));

    let cm_payload = json!({
        "cm": {
            "id": 204,
            "course": 42,
            "module": 2,
            "name": "Cuestionario 1",
            "modname": "quiz",
            "instance": 777
        }
    });

    Mock::given(method("POST"))
        .and(path("/webservice/rest/server.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&cm_payload))
        .mount(&mock)
        .await;

    let ok_ref = resolve_module(&client, "tok", "quiz", None, Some(777))
        .await
        .expect("resolve_module por instance exitoso");
    assert_eq!(
        ok_ref,
        CourseModuleRef {
            cmid: 204,
            instance: 777,
            course: 42,
            modname: "quiz".to_string(),
            name: "Cuestionario 1".to_string(),
        }
    );
}
