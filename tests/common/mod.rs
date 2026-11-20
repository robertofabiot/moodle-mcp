use moodle_mcp::config::Config;
use moodle_mcp::credentials::Credentials;
use moodle_mcp::mcp::{AppContext, MoodleMcp};
use moodle_mcp::moodle::client::{MoodleClient, MoodleClientBuilder};
use moodle_mcp::moodle::gate::MoodleGate;
use rmcp::service::{RoleClient, RunningService};
use rmcp::ServiceExt;
use std::sync::Arc;
use wiremock::MockServer;

/// Arranca un servidor MoodleMcp sobre transporte duplex en memoria conectado a un MockServer,
/// retornando el cliente MCP conectado.
pub async fn start_server(mock: &MockServer) -> RunningService<RoleClient, ()> {
    start_server_with_options(mock, Some("tok123"), None).await
}

/// Arranca un servidor MoodleMcp con opciones avanzadas de token y configuración personalizada.
pub async fn start_server_with_options(
    mock: &MockServer,
    token: Option<&str>,
    custom_config: Option<Config>,
) -> RunningService<RoleClient, ()> {
    let base_url = mock.uri();
    let config = custom_config.unwrap_or_else(|| Config {
        base_url: base_url.clone(),
        download_dir: tempfile::tempdir().unwrap().path().to_path_buf(),
        max_response_bytes: 60_000,
        max_inline_image_bytes: 1_000_000,
    });

    let credentials = token.map(|t| Credentials {
        base_url: base_url.clone(),
        token: t.to_string(),
        private_token: Some("priv123".to_string()),
        userid: 10001,
        username: "24000000".to_string(),
        fullname: "Estudiante Prueba".to_string(),
    });

    let gate = MoodleGate::new(4, 3).without_jitter();
    let client =
        MoodleClient::from_builder(MoodleClientBuilder::new().base_url(base_url).gate(gate));

    let store = moodle_mcp::credentials::CredentialStore::new(
        moodle_mcp::credentials::StoreKind::Auto,
    );
    let ctx = Arc::new(AppContext::new_with_session(
        config,
        client,
        store,
        credentials,
    ));
    let server = MoodleMcp::new(ctx);

    let (client_io, server_io) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });

    ().serve(client_io)
        .await
        .expect("Fallo al conectar cliente duplex a MoodleMcp")
}
