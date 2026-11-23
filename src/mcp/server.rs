use crate::mcp::context::AppContext;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::tool_handler;
use std::sync::Arc;

/// Servidor MCP para Moodle sobre transporte stdio.
#[derive(Clone)]
pub struct MoodleMcp {
    pub(crate) ctx: Arc<AppContext>,
    tool_router: ToolRouter<Self>,
}

impl MoodleMcp {
    /// Construye una nueva instancia del servidor MCP con su contexto y combina los routers de herramientas.
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self {
            ctx,
            tool_router: Self::generic_router() + Self::files_router() + Self::sync_router(),
        }
    }

    /// Retorna una referencia al contexto de la aplicación.
    pub fn context(&self) -> &Arc<AppContext> {
        &self.ctx
    }

    /// Inicia el servicio MCP sobre un transporte dado.
    #[allow(clippy::result_large_err)]
    pub async fn serve<T, E, A>(
        self,
        transport: T,
    ) -> Result<
        rmcp::service::RunningService<rmcp::service::RoleServer, Self>,
        rmcp::service::ServerInitializeError,
    >
    where
        T: rmcp::transport::IntoTransport<rmcp::service::RoleServer, E, A>,
        E: std::error::Error + Send + Sync + 'static,
    {
        use rmcp::ServiceExt;
        ServiceExt::serve(self, transport).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MoodleMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("moodle-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(include_str!("instructions.md"))
    }
}
