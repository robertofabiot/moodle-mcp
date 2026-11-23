use crate::mcp::server::MoodleMcp;
use crate::moodle::error::MoodleError;
use crate::sync::sync_course;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::PathBuf;

/// Parámetros de la herramienta `moodle_sync_course`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SyncCourseParams {
    /// ID numérico del curso en Moodle (courseid)
    pub courseid: i64,
    /// Directorio de destino opcional (por defecto ~/Downloads/moodle-mcp/<curso>)
    pub dest: Option<String>,
}

#[tool_router(router = sync_router, vis = "pub(crate)")]
impl MoodleMcp {
    /// Revisa y replica incrementalmente todos los contenidos y archivos de un curso a disco local.
    #[rmcp::tool(
        name = "moodle_sync_course",
        description = "Revisa y replica incrementalmente todos los contenidos y archivos de un curso a disco local",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    pub async fn moodle_sync_course(&self, params: Parameters<SyncCourseParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let dest_path = params.0.dest.map(PathBuf::from);

        let result = match sync_course(
            &self.ctx.client,
            &self.ctx.file_client,
            &creds.token,
            params.0.courseid,
            None,
            dest_path,
            &self.ctx.config.download_dir,
        )
        .await
        {
            Ok(r) => r,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        self.ctx.ok_json(&result, "moodle_sync_course", Some(&creds))
    }
}
