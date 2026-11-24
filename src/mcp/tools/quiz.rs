use crate::content::html_to_markdown;
use crate::mcp::server::MoodleMcp;
use crate::moodle::error::MoodleError;
use crate::moodle::gate::Priority;
use crate::moodle::ids::resolve_module;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QuizInfoParams {
    /// ID del módulo de curso (cmid). Mutuamente excluyente con quizid.
    pub cmid: Option<i64>,
    /// ID de la instancia del cuestionario (quizid). Mutuamente excluyente con cmid.
    pub quizid: Option<i64>,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct QuizAttemptHistory {
    pub attemptid: i64,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sumgrades: Option<f64>,
    pub timestart: i64,
    pub timefinish: i64,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct QuizInfoResponse {
    pub cmid: i64,
    pub instance: i64,
    pub courseid: i64,
    pub name: String,
    pub intro_markdown: String,
    pub timeopen: i64,
    pub timeclose: i64,
    pub timelimit: i64,
    pub attempts: i64,
    pub grademethod: i64,
    pub accessrules: Vec<String>,
    pub preventaccessreasons: Vec<String>,
    pub requires_password: bool,
    pub requires_seb: bool,
    pub attempts_history: Vec<QuizAttemptHistory>,
}

#[derive(Deserialize)]
struct QuizListResponse {
    #[serde(default)]
    quizzes: Vec<QuizItemRaw>,
}

#[derive(Deserialize)]
struct QuizItemRaw {
    id: i64,
    #[allow(dead_code)]
    #[serde(default)]
    course: Option<i64>,
    #[allow(dead_code)]
    #[serde(default)]
    coursemodule: Option<i64>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    intro: Option<String>,
    #[serde(default)]
    timeopen: Option<i64>,
    #[serde(default)]
    timeclose: Option<i64>,
    #[serde(default)]
    timelimit: Option<i64>,
    #[serde(default)]
    attempts: Option<i64>,
    #[serde(default)]
    grademethod: Option<i64>,
}

#[derive(Deserialize)]
struct QuizAccessInfoResponse {
    #[serde(default)]
    accessrules: Vec<String>,
    #[serde(default)]
    preventaccessreasons: Vec<String>,
    #[serde(default)]
    activerulenames: Vec<String>,
}

#[derive(Deserialize)]
struct QuizUserAttemptsResponse {
    #[serde(default)]
    attempts: Vec<QuizAttemptRaw>,
}

#[derive(Deserialize)]
struct QuizAttemptRaw {
    id: i64,
    #[serde(default)]
    state: String,
    #[serde(default)]
    sumgrades: Option<f64>,
    #[serde(default)]
    timestart: Option<i64>,
    #[serde(default)]
    timefinish: Option<i64>,
}

#[tool_router(router = quiz_router, vis = "pub(crate)")]
impl MoodleMcp {
    /// Consulta la información detallada de un cuestionario en Moodle.
    #[rmcp::tool(
        name = "quiz_info",
        description = "Consulta la información detallada de un cuestionario, incluyendo reglas de acceso, restricciones de contraseña o SEB, fechas y el historial de intentos realizados",
        annotations(read_only_hint = true)
    )]
    pub async fn quiz_info(&self, params: Parameters<QuizInfoParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let module_ref = match resolve_module(
            &self.ctx.client,
            &creds.token,
            "quiz",
            params.0.cmid,
            params.0.quizid,
        )
        .await
        {
            Ok(m) => m,
            Err(MoodleError::InvalidArguments(msg)) => {
                return self.ctx.tool_error(
                    "invalid_arguments",
                    msg,
                    None,
                    None,
                    None,
                    Some(&creds),
                );
            }
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let course_str = module_ref.course.to_string();
        let query_quizzes = [
            ("wstoken", creds.token.as_str()),
            ("wsfunction", "mod_quiz_get_quizzes_by_courses"),
            ("moodlewsrestformat", "json"),
            ("courseids[0]", course_str.as_str()),
        ];

        let quiz_list: QuizListResponse = match self
            .ctx
            .client
            .post_webservice(&query_quizzes, Priority::Interactive)
            .await
        {
            Ok(r) => r,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let quiz_item = match quiz_list
            .quizzes
            .into_iter()
            .find(|q| q.id == module_ref.instance)
        {
            Some(q) => q,
            None => {
                return self.ctx.tool_error(
                    "invalid_arguments",
                    format!(
                        "No se encontró el cuestionario con instancia {} en el curso {}",
                        module_ref.instance, module_ref.course
                    ),
                    None,
                    None,
                    None,
                    Some(&creds),
                );
            }
        };

        let instance_str = module_ref.instance.to_string();
        let query_access = [
            ("wstoken", creds.token.as_str()),
            ("wsfunction", "mod_quiz_get_quiz_access_information"),
            ("moodlewsrestformat", "json"),
            ("quizid", instance_str.as_str()),
        ];

        let access_info: QuizAccessInfoResponse = match self
            .ctx
            .client
            .post_webservice(&query_access, Priority::Interactive)
            .await
        {
            Ok(r) => r,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let query_attempts = [
            ("wstoken", creds.token.as_str()),
            ("wsfunction", "mod_quiz_get_user_attempts"),
            ("moodlewsrestformat", "json"),
            ("quizid", instance_str.as_str()),
            ("status", "all"),
        ];

        let attempts_resp: QuizUserAttemptsResponse = match self
            .ctx
            .client
            .post_webservice(&query_attempts, Priority::Interactive)
            .await
        {
            Ok(r) => r,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let requires_seb = access_info
            .activerulenames
            .iter()
            .any(|r| r == "quizaccess_seb");
        let requires_password = access_info
            .activerulenames
            .iter()
            .any(|r| r == "quizaccess_password");

        let attempts_history = attempts_resp
            .attempts
            .into_iter()
            .map(|a| QuizAttemptHistory {
                attemptid: a.id,
                state: a.state,
                sumgrades: a.sumgrades,
                timestart: a.timestart.unwrap_or(0),
                timefinish: a.timefinish.unwrap_or(0),
            })
            .collect();

        let intro_markdown = html_to_markdown(quiz_item.intro.as_deref().unwrap_or(""))
            .trim()
            .to_string();

        let response = QuizInfoResponse {
            cmid: module_ref.cmid,
            instance: module_ref.instance,
            courseid: module_ref.course,
            name: quiz_item.name,
            intro_markdown,
            timeopen: quiz_item.timeopen.unwrap_or(0),
            timeclose: quiz_item.timeclose.unwrap_or(0),
            timelimit: quiz_item.timelimit.unwrap_or(0),
            attempts: quiz_item.attempts.unwrap_or(0),
            grademethod: quiz_item.grademethod.unwrap_or(1),
            accessrules: access_info.accessrules,
            preventaccessreasons: access_info.preventaccessreasons,
            requires_password,
            requires_seb,
            attempts_history,
        };

        self.ctx.ok_json(&response, "quiz_info", Some(&creds))
    }
}
