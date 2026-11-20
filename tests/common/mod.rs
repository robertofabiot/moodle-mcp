use moodle_mcp::config::Config;
use moodle_mcp::credentials::{CredentialStore, Credentials};
use moodle_mcp::mcp::{AppContext, MoodleMcp};
use moodle_mcp::moodle::client::{MoodleClient, MoodleClientBuilder};
use moodle_mcp::moodle::gate::MoodleGate;
use rmcp::service::{RoleClient, RunningService};
use rmcp::ServiceExt;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use wiremock::MockServer;

/// Contexto de prueba que mantiene vivo el TempDir durante toda la ejecución del test.
#[allow(dead_code)]
pub struct TestContext {
    pub client: RunningService<RoleClient, ()>,
    pub temp_dir: TempDir,
    pub store_file: PathBuf,
    pub store: CredentialStore,
    pub config: Config,
}

impl TestContext {
    pub async fn with_options(
        mock: &MockServer,
        initial_token: Option<&str>,
        max_response_bytes: Option<usize>,
    ) -> Self {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let store_file = temp_dir.path().join("credentials.json");
        let download_dir = temp_dir.path().join("downloads");
        let store = CredentialStore::with_file(store_file.clone());

        let base_url = mock.uri();
        let config = Config {
            base_url: base_url.clone(),
            download_dir,
            max_response_bytes: max_response_bytes.unwrap_or(60_000),
            max_inline_image_bytes: 1_000_000,
        };

        if let Some(token) = initial_token {
            let creds = Credentials {
                base_url: base_url.clone(),
                token: token.to_string(),
                private_token: Some("priv123".to_string()),
                userid: 10001,
                username: "24000000".to_string(),
                fullname: "Estudiante Prueba".to_string(),
            };
            store
                .save(&creds)
                .await
                .expect("guardar credenciales iniciales");
        }

        let gate = MoodleGate::new(4, 3).without_jitter();
        let client =
            MoodleClient::from_builder(MoodleClientBuilder::new().base_url(base_url).gate(gate));

        let ctx = Arc::new(AppContext::new(config.clone(), client, store.clone()));
        let server = MoodleMcp::new(ctx);

        let (client_io, server_io) = tokio::io::duplex(1 << 16);
        tokio::spawn(async move {
            if let Ok(running) = server.serve(server_io).await {
                let _ = running.waiting().await;
            }
        });

        let client =
            ().serve(client_io)
                .await
                .expect("Fallo al conectar cliente duplex a MoodleMcp");

        Self {
            client,
            temp_dir,
            store_file,
            store,
            config,
        }
    }
}

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
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let download_dir = temp_dir.path().join("downloads");
    // Filtrar caída prematura del directorio temporal en pruebas
    let _ = Box::leak(Box::new(temp_dir));

    let config = custom_config.unwrap_or_else(|| Config {
        base_url: base_url.clone(),
        download_dir,
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

    let store =
        moodle_mcp::credentials::CredentialStore::new(moodle_mcp::credentials::StoreKind::Auto);
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
