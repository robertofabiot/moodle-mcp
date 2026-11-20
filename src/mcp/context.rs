use crate::config::Config;
use crate::credentials::{CredentialStore, Credentials};
use crate::moodle::auth::{SiteInfo, SiteInfoFunction};
use crate::moodle::client::MoodleClient;
use crate::moodle::error::MoodleError;
use crate::moodle::gate::Priority;
use rmcp::model::CallToolResult;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Origen de las credenciales activas en la sesión.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSource {
    Env,
    Store,
}

/// Contexto global compartido del servidor MCP.
pub struct AppContext {
    /// Configuración operativa del servidor.
    pub config: Config,
    /// Cliente HTTP para operaciones de lectura (con reintentos configurados).
    pub client: MoodleClient,
    /// Cliente HTTP para operaciones de escritura (con 0 reintentos para evitar duplicación).
    pub write_client: MoodleClient,
    /// Almacén de credenciales para resolución bajo demanda.
    pub store: CredentialStore,
    /// Credenciales del usuario autenticado si hay sesión activa.
    pub session: RwLock<Option<Arc<Credentials>>>,
    /// Información del sitio y usuario cacheada en memoria.
    pub site_info: RwLock<Option<Arc<SiteInfo>>>,
    /// Procedencia de la sesión actual (variables de entorno o almacén).
    pub session_source: RwLock<Option<SessionSource>>,
}

impl AppContext {
    /// Inicializa un nuevo contexto con la configuración, cliente HTTP y almacén de credenciales.
    /// `write_client` se inicializa con `client.with_max_retries(0)`.
    pub fn new(config: Config, client: MoodleClient, store: CredentialStore) -> Self {
        let write_client = client.with_max_retries(0);
        Self {
            config,
            client,
            write_client,
            store,
            session: RwLock::new(None),
            site_info: RwLock::new(None),
            session_source: RwLock::new(None),
        }
    }

    /// Inicializa un nuevo contexto precargando una sesión opcional (útil para pruebas).
    pub fn new_with_session(
        config: Config,
        client: MoodleClient,
        store: CredentialStore,
        credentials: Option<Credentials>,
    ) -> Self {
        let write_client = client.with_max_retries(0);
        let has_creds = credentials.is_some();
        Self {
            config,
            client,
            write_client,
            store,
            session: RwLock::new(credentials.map(Arc::new)),
            site_info: RwLock::new(None),
            session_source: RwLock::new(if has_creds {
                Some(SessionSource::Store)
            } else {
                None
            }),
        }
    }

    /// Resuelve las credenciales activas bajo demanda respetando el orden de precedencia:
    /// 1. Sesión ya cargada en caché.
    /// 2. Variables de entorno (`MOODLE_TOKEN` y opcional `MOODLE_PRIVATE_TOKEN`).
    /// 3. Almacén de credenciales (`CredentialStore`).
    ///
    /// NO mantiene locks tomados durante llamadas de red.
    pub async fn session(&self) -> Result<Arc<Credentials>, CallToolResult> {
        // 1. Verificar si ya existe sesión activa en caché
        {
            let guard = self.session.read().await;
            if let Some(ref creds) = *guard {
                return Ok(Arc::clone(creds));
            }
        }

        // 2. Comprobar si hay token en variable de entorno
        if let Ok(token) = std::env::var("MOODLE_TOKEN") {
            let token = token.trim().to_string();
            if !token.is_empty() {
                let private_token = std::env::var("MOODLE_PRIVATE_TOKEN")
                    .ok()
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty());

                // Llamada de red sin locks retenidos
                let site_info = match self
                    .client
                    .get_site_info(&token, Priority::Interactive)
                    .await
                {
                    Ok(info) => info,
                    Err(MoodleError::InvalidToken) => {
                        *self.session_source.write().await = Some(SessionSource::Env);
                        tracing::warn!("Token proporcionado en MOODLE_TOKEN no es válido");
                        return Err(self.invalid_token_error().await);
                    }
                    Err(e) => {
                        tracing::warn!("Error de red al validar MOODLE_TOKEN: {e}");
                        return Err(self.tool_error_from_moodle(&e));
                    }
                };

                let creds = Arc::new(Credentials {
                    base_url: self.config.base_url.clone(),
                    token,
                    private_token,
                    userid: site_info.userid,
                    username: site_info
                        .username
                        .clone()
                        .unwrap_or_else(|| "env_user".to_string()),
                    fullname: site_info.fullname.clone(),
                });

                *self.session.write().await = Some(Arc::clone(&creds));
                *self.site_info.write().await = Some(Arc::new(site_info));
                *self.session_source.write().await = Some(SessionSource::Env);

                return Ok(creds);
            }
        }

        // 3. Consultar almacén de credenciales
        match self.store.load(&self.config.base_url).await {
            Ok(Some(creds)) => {
                let arc_creds = Arc::new(creds);
                *self.session.write().await = Some(Arc::clone(&arc_creds));
                *self.session_source.write().await = Some(SessionSource::Store);
                Ok(arc_creds)
            }
            Ok(None) => Err(self.no_session_error()),
            Err(e) => {
                tracing::warn!("Error al leer credenciales del almacén: {e}");
                Err(self.tool_error(
                    "io",
                    format!("Error al leer credenciales del almacén: {e}"),
                    None,
                    None,
                    None,
                ))
            }
        }
    }

    /// Invalida la sesión actual vaciando la caché de credenciales y site_info.
    pub async fn invalidate_session(&self) {
        *self.session.write().await = None;
        *self.site_info.write().await = None;
        *self.session_source.write().await = None;
    }

    /// Retorna una respuesta de error para token inválido con sugerencia contextual.
    pub async fn invalid_token_error(&self) -> CallToolResult {
        let source = *self.session_source.read().await;
        let hint = match source {
            Some(SessionSource::Env) => {
                "Actualiza la variable de entorno MOODLE_TOKEN y reinicia el cliente MCP"
                    .to_string()
            }
            _ => "Ejecuta 'moodle-mcp login' para renovar el token".to_string(),
        };

        self.tool_error(
            "invalid_token",
            "Token de Moodle inválido o expirado",
            Some("invalidtoken".to_string()),
            None,
            Some(hint),
        )
    }

    /// Obtiene la información del sitio y usuario cacheada en `site_info`.
    /// Si aún no ha sido cargada, la solicita al servidor Moodle sin retener locks durante la llamada de red.
    pub async fn get_site_info(&self) -> Result<Arc<SiteInfo>, CallToolResult> {
        let creds = self.session().await?;
        {
            let guard = self.site_info.read().await;
            if let Some(ref info) = *guard {
                return Ok(Arc::clone(info));
            }
        }

        // Consulta de red sin locks retenidos
        let info = match self
            .client
            .get_site_info(&creds.token, Priority::Interactive)
            .await
        {
            Ok(info) => Arc::new(info),
            Err(MoodleError::InvalidToken) => {
                self.invalidate_session().await;
                return Err(self.invalid_token_error().await);
            }
            Err(e) => return Err(self.tool_error_from_moodle(&e)),
        };

        *self.site_info.write().await = Some(Arc::clone(&info));
        Ok(info)
    }

    /// Comprueba si una función específica está en la lista de funciones habilitadas del usuario.
    pub async fn is_function_enabled(&self, function: &str) -> Result<bool, CallToolResult> {
        let info = self.get_site_info().await?;
        if let Some(ref funcs) = info.functions {
            Ok(funcs.iter().any(|f| f.name == function))
        } else {
            Ok(false)
        }
    }

    /// Retorna la lista de funciones habilitadas devueltas por `get_site_info`.
    pub async fn get_enabled_functions(&self) -> Result<Vec<SiteInfoFunction>, CallToolResult> {
        let info = self.get_site_info().await?;
        Ok(info.functions.clone().unwrap_or_default())
    }

    /// Retorna el wstoken si existe sesión activa en caché.
    pub fn token(&self) -> Option<String> {
        self.session
            .try_read()
            .ok()
            .and_then(|guard| guard.as_ref().map(|c| c.token.clone()))
    }

    /// Retorna el privatetoken si existe sesión activa y fue proporcionado.
    pub fn private_token(&self) -> Option<String> {
        self.session
            .try_read()
            .ok()
            .and_then(|guard| guard.as_ref().and_then(|c| c.private_token.clone()))
    }

    /// Formatea una respuesta de éxito con truncado a archivo si excede `max_response_bytes`
    /// y sanitización de tokens.
    pub fn ok_json<T: Serialize>(&self, value: &T, function: &str) -> CallToolResult {
        let tok = self.token();
        let priv_tok = self.private_token();
        match crate::mcp::output::ok_json(
            value,
            self.config.max_response_bytes,
            &self.config.download_dir,
            function,
            tok.as_deref(),
            priv_tok.as_deref(),
        ) {
            Ok(json_text) => {
                crate::mcp::output::tool_success(&json_text, tok.as_deref(), priv_tok.as_deref())
            }
            Err(err) => err,
        }
    }

    /// Genera una respuesta de error MCP tipada y sanitizada.
    pub fn tool_error(
        &self,
        kind: impl Into<String>,
        message: impl Into<String>,
        errorcode: Option<String>,
        debuginfo: Option<String>,
        hint: Option<String>,
    ) -> CallToolResult {
        let tok = self.token();
        let priv_tok = self.private_token();
        crate::mcp::output::tool_error(
            kind,
            message,
            errorcode,
            debuginfo,
            hint,
            tok.as_deref(),
            priv_tok.as_deref(),
        )
    }

    /// Convierte un `MoodleError` a respuesta de error MCP sanitizada.
    pub fn tool_error_from_moodle(&self, err: &MoodleError) -> CallToolResult {
        let tok = self.token();
        let priv_tok = self.private_token();
        crate::mcp::output::tool_error_from_moodle(err, tok.as_deref(), priv_tok.as_deref())
    }

    /// Genera la respuesta estándar para cuando no hay sesión activa.
    pub fn no_session_error(&self) -> CallToolResult {
        self.tool_error(
            "no_session",
            "No hay sesión activa de Moodle",
            None,
            None,
            Some(
                "Ejecuta 'moodle-mcp login' en una terminal; no hace falta reiniciar el servidor"
                    .to_string(),
            ),
        )
    }
}
