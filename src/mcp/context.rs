use crate::config::Config;
use crate::credentials::Credentials;
use crate::moodle::auth::{SiteInfo, SiteInfoFunction};
use crate::moodle::client::MoodleClient;
use crate::moodle::error::{MoodleError, Result};
use crate::moodle::gate::Priority;
use tokio::sync::OnceCell;

/// Contexto global compartido del servidor MCP.
pub struct AppContext {
    /// Configuración operativa del servidor.
    pub config: Config,
    /// Cliente HTTP para operaciones de lectura (con reintentos configurados).
    pub client: MoodleClient,
    /// Cliente HTTP para operaciones de escritura (con 0 reintentos para evitar duplicación).
    pub write_client: MoodleClient,
    /// Credenciales del usuario autenticado si hay sesión activa.
    pub credentials: Option<Credentials>,
    /// Información del sitio y usuario cacheada en memoria.
    pub site_info: OnceCell<SiteInfo>,
}

impl AppContext {
    /// Inicializa un nuevo contexto con la configuración, credenciales y cliente HTTP.
    ///  se inicializa con .
    pub fn new(config: Config, credentials: Option<Credentials>, client: MoodleClient) -> Self {
        let write_client = client.with_max_retries(0);
        Self {
            config,
            client,
            write_client,
            credentials,
            site_info: OnceCell::new(),
        }
    }

    /// Obtiene la información del sitio y usuario cacheada en .
    /// Si aún no ha sido cargada, la solicita al servidor Moodle con prioridad interactiva.
    pub async fn get_site_info(&self) -> Result<&SiteInfo> {
        let creds = self
            .credentials
            .as_ref()
            .ok_or_else(|| MoodleError::InvalidArguments("No hay sesión activa".to_string()))?;

        self.site_info
            .get_or_try_init(|| async {
                self.client
                    .get_site_info(&creds.token, Priority::Interactive)
                    .await
            })
            .await
    }

    /// Comprueba si una función específica está en la lista de funciones habilitadas del usuario.
    pub async fn is_function_enabled(&self, function: &str) -> Result<bool> {
        let info = self.get_site_info().await?;
        if let Some(ref funcs) = info.functions {
            Ok(funcs.iter().any(|f| f.name == function))
        } else {
            Ok(false)
        }
    }

    /// Retorna la lista de funciones habilitadas devueltas por .
    pub async fn get_enabled_functions(&self) -> Result<&[SiteInfoFunction]> {
        let info = self.get_site_info().await?;
        Ok(info.functions.as_deref().unwrap_or(&[]))
    }

    /// Retorna una referencia al wstoken si existe sesión activa.
    pub fn token(&self) -> Option<&str> {
        self.credentials.as_ref().map(|c| c.token.as_str())
    }

    /// Retorna una referencia al privatetoken si existe sesión activa y fue proporcionado.
    pub fn private_token(&self) -> Option<&str> {
        self.credentials
            .as_ref()
            .and_then(|c| c.private_token.as_deref())
    }
}
