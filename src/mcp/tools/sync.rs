use crate::mcp::server::MoodleMcp;
use crate::moodle::courses::MoodleCourse;
use crate::moodle::error::MoodleError;
use crate::moodle::files::sanitize_filename;
use crate::moodle::gate::Priority;
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

#[derive(Deserialize)]
struct CoursesByFieldResp {
    #[serde(default)]
    courses: Vec<CourseByFieldItem>,
}

#[derive(Deserialize)]
struct CourseByFieldItem {
    id: i64,
    #[serde(default)]
    shortname: Option<String>,
    #[serde(default)]
    fullname: Option<String>,
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

        let courseid = params.0.courseid;
        let dest_path = params.0.dest.map(|d| {
            let p = PathBuf::from(d);
            if p.is_relative() {
                self.ctx.config.download_dir.join(p)
            } else {
                p
            }
        });

        // Intentar obtener el nombre del curso para la carpeta por defecto
        let courseid_str = courseid.to_string();
        let query_by_field = [
            ("wstoken", creds.token.as_str()),
            ("wsfunction", "core_course_get_courses_by_field"),
            ("moodlewsrestformat", "json"),
            ("field", "id"),
            ("value", &courseid_str),
        ];

        let course_name = match self
            .ctx
            .client
            .post_webservice::<CoursesByFieldResp>(&query_by_field, Priority::Interactive)
            .await
        {
            Ok(resp) => resp
                .courses
                .into_iter()
                .find(|c| c.id == courseid)
                .and_then(|c| c.shortname.filter(|s| !s.trim().is_empty()).or(c.fullname)),
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(_) => None,
        };

        let course_name = match course_name {
            Some(name) => name,
            None => {
                let userid_str = creds.userid.to_string();
                let query_enrol = [
                    ("wstoken", creds.token.as_str()),
                    ("wsfunction", "core_enrol_get_users_courses"),
                    ("moodlewsrestformat", "json"),
                    ("userid", &userid_str),
                ];
                let enrolled: Vec<MoodleCourse> = match self
                    .ctx
                    .client
                    .post_webservice(&query_enrol, Priority::Interactive)
                    .await
                {
                    Ok(list) => list,
                    Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
                    Err(_) => Vec::new(),
                };

                enrolled
                    .into_iter()
                    .find(|c| c.id == courseid)
                    .and_then(|c| {
                        c.shortname
                            .filter(|s| !s.trim().is_empty())
                            .or(Some(c.fullname))
                    })
                    .unwrap_or_else(|| format!("curso-{courseid}"))
            }
        };

        let sanitized_course_name = sanitize_filename(&course_name);

        let result = match sync_course(
            &self.ctx.client,
            &self.ctx.file_client,
            &creds.token,
            courseid,
            Some(&sanitized_course_name),
            dest_path,
            &self.ctx.config.download_dir,
        )
        .await
        {
            Ok(r) => r,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        self.ctx
            .ok_json(&result, "moodle_sync_course", Some(&creds))
    }
}
