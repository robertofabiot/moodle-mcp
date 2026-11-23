use crate::content::html_to_markdown;
use crate::mcp::server::MoodleMcp;
use crate::moodle::courses::MoodleCourse;
use crate::moodle::error::MoodleError;
use crate::moodle::gate::Priority;
use chrono::{DateTime, Utc};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListCoursesParams {
    /// Si es true, incluye cursos archivados o pasados (por defecto false)
    #[serde(default)]
    pub include_past: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetCourseParams {
    /// ID numérico del curso en Moodle (courseid)
    pub courseid: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CourseUpdatesSinceParams {
    /// ID numérico del curso en Moodle (courseid)
    pub courseid: i64,
    /// Timestamp Unix epoch a partir del cual buscar actualizaciones
    pub since: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetGradesParams {
    /// ID opcional del curso (si se omite, devuelve calificaciones globales del estudiante)
    pub courseid: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpcomingEventsParams {
    /// Número de días hacia adelante para buscar eventos (por defecto 14)
    #[serde(default)]
    pub days: Option<i64>,
}

// Modelos de salida

#[derive(Debug, Serialize)]
pub struct CourseListItem {
    pub id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortname: Option<String>,
    pub fullname: String,
    pub active: bool,
}

#[derive(Debug, Serialize)]
pub struct CourseDetailSection {
    pub id: i64,
    pub section: i64,
    pub name: String,
    pub summary_markdown: String,
    pub modules: Vec<CourseDetailModule>,
}

#[derive(Debug, Serialize)]
pub struct CourseDetailModule {
    pub cmid: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<i64>,
    pub modname: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description_markdown: Option<String>,
    pub files: Vec<CourseDetailFile>,
    pub external_links: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CourseDetailFile {
    pub filename: String,
    pub size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fileurl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mimetype: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GradeItemOutput {
    pub item: String,
    pub grade: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percentage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback_markdown: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpcomingEventOutput {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub course: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmid: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<i64>,
    pub timesort: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

// Modelos internos para deserializar respuestas de Moodle

#[derive(Deserialize)]
struct OverviewGradesContainer {
    #[serde(default)]
    grades: Vec<OverviewGradeEntry>,
}

#[derive(Deserialize)]
struct OverviewGradeEntry {
    courseid: i64,
    #[serde(default)]
    grade: Option<String>,
    #[serde(default)]
    rawgrade: Option<String>,
}

#[derive(Deserialize)]
struct UserGradesContainer {
    #[serde(default)]
    usergrades: Vec<UserGradeReport>,
}

#[derive(Deserialize)]
struct UserGradeReport {
    #[serde(default)]
    gradeitems: Vec<UserGradeItemRaw>,
}

#[derive(Deserialize)]
struct UserGradeItemRaw {
    #[serde(default)]
    itemname: Option<String>,
    #[serde(default)]
    itemmodule: Option<String>,
    #[serde(default)]
    gradeformatted: Option<String>,
    #[serde(default)]
    graderaw: Option<serde_json::Value>,
    #[serde(default)]
    grademin: Option<f64>,
    #[serde(default)]
    grademax: Option<f64>,
    #[serde(default)]
    percentageformatted: Option<String>,
    #[serde(default)]
    feedback: Option<String>,
}

#[derive(Deserialize)]
struct CalendarEventsContainer {
    #[serde(default)]
    events: Vec<CalendarEventRaw>,
}

#[derive(Deserialize)]
struct CalendarEventRaw {
    name: String,
    #[serde(default)]
    course: Option<CalendarCourseRaw>,
    #[serde(default)]
    modulename: Option<String>,
    #[serde(default)]
    instance: Option<i64>,
    #[serde(default)]
    timesort: i64,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    action: Option<CalendarActionRaw>,
}

#[derive(Deserialize)]
struct CalendarCourseRaw {
    #[serde(default)]
    fullname: Option<String>,
}

#[derive(Deserialize)]
struct CalendarActionRaw {
    #[serde(default)]
    name: Option<String>,
}

#[tool_router(router = courses_router, vis = "pub(crate)")]
impl MoodleMcp {
    /// Lista los cursos matriculados del estudiante, con filtro de cursos activos.
    #[rmcp::tool(
        name = "list_courses",
        description = "Lista los cursos matriculados del estudiante, indicando si están activos o archivados",
        annotations(read_only_hint = true)
    )]
    pub async fn list_courses(&self, params: Parameters<ListCoursesParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let userid_str = creds.userid.to_string();
        let query = [
            ("wstoken", creds.token.as_str()),
            ("wsfunction", "core_enrol_get_users_courses"),
            ("moodlewsrestformat", "json"),
            ("userid", &userid_str),
        ];

        let courses: Vec<MoodleCourse> = match self
            .ctx
            .client
            .post_webservice(&query, Priority::Interactive)
            .await
        {
            Ok(c) => c,
            Err(MoodleError::InvalidToken) => {
                self.ctx.invalidate_session().await;
                return self.ctx.invalid_token_error().await;
            }
            Err(e) => return self.ctx.tool_error_from_moodle(&e),
        };

        let now = Utc::now().timestamp();
        let items: Vec<CourseListItem> = courses
            .into_iter()
            .filter_map(|c| {
                let active = c.is_active(now);
                if !params.0.include_past && !active {
                    None
                } else {
                    Some(CourseListItem {
                        id: c.id,
                        shortname: c.shortname,
                        fullname: c.fullname,
                        active,
                    })
                }
            })
            .collect();

        self.ctx.ok_json(&items, "list_courses")
    }

    /// Obtiene el árbol detallado de secciones y módulos de un curso con resúmenes convertidos a Markdown.
    #[rmcp::tool(
        name = "get_course",
        description = "Obtiene el árbol estructurado de secciones, actividades y recursos de un curso en Markdown",
        annotations(read_only_hint = true)
    )]
    pub async fn get_course(&self, params: Parameters<GetCourseParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let sections = match self
            .ctx
            .client
            .course_tree(&creds.token, params.0.courseid)
            .await
        {
            Ok(s) => s,
            Err(MoodleError::InvalidToken) => {
                self.ctx.invalidate_session().await;
                return self.ctx.invalid_token_error().await;
            }
            Err(e) => return self.ctx.tool_error_from_moodle(&e),
        };

        let base_url = Url::parse(self.ctx.client.base_url()).ok();

        let detail_sections: Vec<CourseDetailSection> = sections
            .into_iter()
            .map(|s| {
                let summary_markdown = html_to_markdown(&s.summary);
                let modules = s
                    .modules
                    .into_iter()
                    .map(|m| {
                        let description_markdown = m.description.as_deref().map(html_to_markdown);
                        let mut files = Vec::new();
                        let mut external_links = Vec::new();

                        for c in m.contents {
                            if c.r#type == "url" {
                                if let Some(u) = c.fileurl {
                                    external_links.push(u);
                                }
                                continue;
                            }

                            if let Some(ref furl) = c.fileurl {
                                if let Some(ref bu) = base_url {
                                    if let Ok(u) = Url::parse(furl) {
                                        if u.host_str() != bu.host_str() {
                                            external_links.push(furl.clone());
                                            continue;
                                        }
                                    }
                                }
                            }

                            files.push(CourseDetailFile {
                                filename: c.filename,
                                size: c.filesize.unwrap_or(0),
                                fileurl: c.fileurl,
                                mimetype: c.mimetype,
                            });
                        }

                        CourseDetailModule {
                            cmid: m.id,
                            instance: m.instance,
                            modname: m.modname,
                            name: m.name,
                            url: m.url,
                            description_markdown,
                            files,
                            external_links,
                        }
                    })
                    .collect();

                CourseDetailSection {
                    id: s.id,
                    section: s.section,
                    name: s.name,
                    summary_markdown,
                    modules,
                }
            })
            .collect();

        self.ctx.ok_json(&detail_sections, "get_course")
    }

    /// Comprueba si ha habido cambios o actualizaciones en los módulos de un curso desde un momento específico.
    #[rmcp::tool(
        name = "course_updates_since",
        description = "Comprueba si ha habido cambios o actualizaciones en un curso desde un timestamp específico",
        annotations(read_only_hint = true)
    )]
    pub async fn course_updates_since(
        &self,
        params: Parameters<CourseUpdatesSinceParams>,
    ) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let courseid_str = params.0.courseid.to_string();
        let since_str = params.0.since.to_string();
        let query = [
            ("wstoken", creds.token.as_str()),
            ("wsfunction", "core_course_get_updates_since"),
            ("moodlewsrestformat", "json"),
            ("courseid", &courseid_str),
            ("since", &since_str),
        ];

        let updates: serde_json::Value = match self
            .ctx
            .client
            .post_webservice(&query, Priority::Interactive)
            .await
        {
            Ok(u) => u,
            Err(MoodleError::InvalidToken) => {
                self.ctx.invalidate_session().await;
                return self.ctx.invalid_token_error().await;
            }
            Err(e) => return self.ctx.tool_error_from_moodle(&e),
        };

        self.ctx.ok_json(&updates, "course_updates_since")
    }

    /// Obtiene las calificaciones globales del estudiante o el desglose detallado de un curso.
    #[rmcp::tool(
        name = "get_grades",
        description = "Obtiene las calificaciones del estudiante: globales entre cursos o desglosadas por actividades",
        annotations(read_only_hint = true)
    )]
    pub async fn get_grades(&self, params: Parameters<GetGradesParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let userid_str = creds.userid.to_string();

        match params.0.courseid {
            None => {
                let query = [
                    ("wstoken", creds.token.as_str()),
                    ("wsfunction", "gradereport_overview_get_course_grades"),
                    ("moodlewsrestformat", "json"),
                    ("userid", &userid_str),
                ];

                let resp: OverviewGradesContainer = match self
                    .ctx
                    .client
                    .post_webservice(&query, Priority::Interactive)
                    .await
                {
                    Ok(r) => r,
                    Err(MoodleError::InvalidToken) => {
                        self.ctx.invalidate_session().await;
                        return self.ctx.invalid_token_error().await;
                    }
                    Err(e) => return self.ctx.tool_error_from_moodle(&e),
                };

                let items: Vec<GradeItemOutput> = resp
                    .grades
                    .into_iter()
                    .map(|g| {
                        let grade_str = g.grade.or(g.rawgrade).unwrap_or_else(|| "-".to_string());
                        GradeItemOutput {
                            item: format!("Curso {}", g.courseid),
                            grade: grade_str,
                            range: None,
                            percentage: None,
                            feedback_markdown: None,
                        }
                    })
                    .collect();

                self.ctx.ok_json(&items, "get_grades")
            }
            Some(courseid) => {
                let courseid_str = courseid.to_string();
                let query = [
                    ("wstoken", creds.token.as_str()),
                    ("wsfunction", "gradereport_user_get_grade_items"),
                    ("moodlewsrestformat", "json"),
                    ("courseid", &courseid_str),
                    ("userid", &userid_str),
                ];

                let resp: UserGradesContainer = match self
                    .ctx
                    .client
                    .post_webservice(&query, Priority::Interactive)
                    .await
                {
                    Ok(r) => r,
                    Err(MoodleError::InvalidToken) => {
                        self.ctx.invalidate_session().await;
                        return self.ctx.invalid_token_error().await;
                    }
                    Err(e) => return self.ctx.tool_error_from_moodle(&e),
                };

                let mut items = Vec::new();
                for report in resp.usergrades {
                    for gi in report.gradeitems {
                        let item_title = gi
                            .itemname
                            .or(gi.itemmodule)
                            .unwrap_or_else(|| "Calificación".to_string());

                        let grade_str = gi
                            .gradeformatted
                            .or_else(|| gi.graderaw.map(|v| v.to_string()))
                            .unwrap_or_else(|| "-".to_string());

                        let range = match (gi.grademin, gi.grademax) {
                            (Some(min), Some(max)) => Some(format!("{min} - {max}")),
                            _ => None,
                        };

                        let feedback_markdown = gi
                            .feedback
                            .as_deref()
                            .filter(|f| !f.trim().is_empty())
                            .map(html_to_markdown);

                        items.push(GradeItemOutput {
                            item: item_title,
                            grade: grade_str,
                            range,
                            percentage: gi.percentageformatted,
                            feedback_markdown,
                        });
                    }
                }

                self.ctx.ok_json(&items, "get_grades")
            }
        }
    }

    /// Consulta las fechas de entregas y eventos futuros del calendario del estudiante.
    #[rmcp::tool(
        name = "upcoming_events",
        description = "Consulta las fechas límite y eventos futuros del calendario del estudiante",
        annotations(read_only_hint = true)
    )]
    pub async fn upcoming_events(
        &self,
        params: Parameters<UpcomingEventsParams>,
    ) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let days = params.0.days.unwrap_or(14);
        let now = Utc::now().timestamp();
        let timesortfrom = now;
        let timesortto = now + days * 86400;

        let from_str = timesortfrom.to_string();
        let to_str = timesortto.to_string();
        let query = [
            ("wstoken", creds.token.as_str()),
            ("wsfunction", "core_calendar_get_action_events_by_timesort"),
            ("moodlewsrestformat", "json"),
            ("timesortfrom", &from_str),
            ("timesortto", &to_str),
            ("limitnum", "50"),
        ];

        let resp: CalendarEventsContainer = match self
            .ctx
            .client
            .post_webservice(&query, Priority::Interactive)
            .await
        {
            Ok(r) => r,
            Err(MoodleError::InvalidToken) => {
                self.ctx.invalidate_session().await;
                return self.ctx.invalid_token_error().await;
            }
            Err(e) => return self.ctx.tool_error_from_moodle(&e),
        };

        let items: Vec<UpcomingEventOutput> = resp
            .events
            .into_iter()
            .map(|e| {
                let timesort_iso = DateTime::from_timestamp(e.timesort, 0)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| e.timesort.to_string());

                UpcomingEventOutput {
                    name: e.name,
                    course: e.course.and_then(|c| c.fullname),
                    modname: e.modulename,
                    cmid: None,
                    instance: e.instance,
                    timesort: timesort_iso,
                    url: e.url,
                    action: e.action.and_then(|a| a.name),
                }
            })
            .collect();

        self.ctx.ok_json(&items, "upcoming_events")
    }
}
