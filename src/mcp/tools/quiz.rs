use crate::content::{html_to_markdown, parse_question, ParsedQuestion};
use crate::mcp::server::MoodleMcp;
use crate::moodle::error::MoodleError;
use crate::moodle::gate::Priority;
use crate::moodle::ids::resolve_module;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QuizStartParams {
    /// ID del módulo de curso (cmid). Mutuamente excluyente con quizid.
    pub cmid: Option<i64>,
    /// ID de la instancia del cuestionario (quizid). Mutuamente excluyente con cmid.
    pub quizid: Option<i64>,
    /// Contraseña requerida por el cuestionario si aplica.
    pub password: Option<String>,
    /// Si es true, fuerza la creación de un nuevo intento incluso si existe uno en progreso.
    #[serde(default)]
    pub forcenew: Option<bool>,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct QuizStartResponse {
    pub attemptid: i64,
    pub state: String,
    pub timestart: i64,
    pub timelimit: i64,
    pub endtime: i64,
    pub pages: Vec<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QuizGetPageParams {
    /// ID del intento activo
    pub attemptid: i64,
    /// Número de página a consultar (inicia en 0)
    pub page: i64,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct QuizPageResponse {
    pub attemptid: i64,
    pub page: i64,
    pub nextpage: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeleft: Option<i64>,
    pub questions: Vec<ParsedQuestion>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QuizSaveParams {
    /// ID del intento activo
    pub attemptid: i64,
    /// Respuestas a guardar (pares nombre/valor)
    pub answers: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct QuizSaveResponse {
    pub saved: bool,
    pub attemptid: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QuizReviewParams {
    /// ID del intento a revisar
    pub attemptid: i64,
    /// Página específica a revisar (-1 para consultar todas las páginas disponibles)
    pub page: Option<i64>,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct QuizReviewResponse {
    pub attemptid: i64,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<f64>,
    pub questions: Vec<ParsedQuestion>,
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

#[derive(Deserialize, Clone)]
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
    #[serde(default)]
    layout: Option<String>,
    #[serde(default)]
    timelimit: Option<i64>,
    #[serde(default)]
    endtime: Option<i64>,
    #[serde(default)]
    pages: Option<Vec<i64>>,
}

#[derive(Deserialize)]
struct AttemptReviewRaw {
    attempt: AttemptMetaRaw,
    #[serde(default)]
    grade: Option<serde_json::Value>,
    #[serde(default)]
    questions: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct AttemptDataRaw {
    #[allow(dead_code)]
    #[serde(default)]
    attempt: Option<AttemptMetaRaw>,
    #[serde(default)]
    nextpage: Option<i64>,
    #[serde(default)]
    timeleft: Option<i64>,
    #[serde(default)]
    questions: Vec<serde_json::Value>,
    #[allow(dead_code)]
    #[serde(default)]
    warnings: Vec<serde_json::Value>,
}

#[derive(Deserialize, Clone)]
struct AttemptMetaRaw {
    id: i64,
    #[serde(default)]
    state: String,
    #[serde(default)]
    sumgrades: Option<f64>,
    #[serde(default)]
    timestart: Option<i64>,
    #[allow(dead_code)]
    #[serde(default)]
    timefinish: Option<i64>,
    #[serde(default)]
    layout: Option<String>,
    #[serde(default)]
    timelimit: Option<i64>,
    #[serde(default)]
    endtime: Option<i64>,
    #[serde(default)]
    timeleft: Option<i64>,
    #[serde(default)]
    pages: Option<Vec<i64>>,
}

/// Extrae mensajes o códigos de advertencia devueltos por Moodle en array o campo warnings.
fn extract_warnings(val: &serde_json::Value) -> Option<String> {
    let mut warnings = Vec::new();
    if let Some(arr) = val.as_array() {
        for item in arr {
            if let Some(msg) = item.get("message").and_then(|m| m.as_str()) {
                warnings.push(msg.to_string());
            } else if let Some(code) = item.get("warningcode").and_then(|c| c.as_str()) {
                warnings.push(code.to_string());
            }
        }
    } else if let Some(arr) = val.get("warnings").and_then(|w| w.as_array()) {
        for item in arr {
            if let Some(msg) = item.get("message").and_then(|m| m.as_str()) {
                warnings.push(msg.to_string());
            } else if let Some(code) = item.get("warningcode").and_then(|c| c.as_str()) {
                warnings.push(code.to_string());
            }
        }
    }
    if warnings.is_empty() {
        None
    } else {
        Some(warnings.join("; "))
    }
}

/// Convierte el layout de Moodle (números y ceros delimitados por comas) en una lista de páginas (0-indexed).
pub fn parse_layout_pages(layout: &str) -> Vec<i64> {
    let mut pages = Vec::new();
    let mut current_page = 0;
    let mut page_has_questions = false;

    for part in layout.split(',') {
        let part = part.trim();
        if part == "0" {
            pages.push(current_page);
            current_page += 1;
            page_has_questions = false;
        } else if !part.is_empty() {
            page_has_questions = true;
        }
    }

    if page_has_questions || pages.is_empty() {
        pages.push(current_page);
    }

    pages
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

    /// Inicia un nuevo intento o retoma uno en curso para un cuestionario.
    #[rmcp::tool(
        name = "quiz_start",
        description = "Inicia un nuevo intento de resolución o retoma uno en curso, soportando contraseña y verificando restricciones como Safe Exam Browser",
        annotations(destructive_hint = true)
    )]
    pub async fn quiz_start(&self, params: Parameters<QuizStartParams>) -> CallToolResult {
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

        if access_info
            .activerulenames
            .iter()
            .any(|r| r == "quizaccess_seb")
        {
            return self.ctx.tool_error(
                "impossible",
                "este cuestionario exige Safe Exam Browser; no puede hacerse por la API",
                None,
                None,
                None,
                Some(&creds),
            );
        }

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

        let forcenew = params.0.forcenew.unwrap_or(false);
        let existing_attempt = if !forcenew {
            attempts_resp
                .attempts
                .into_iter()
                .find(|a| a.state == "inprogress" || a.state == "overdue")
        } else {
            None
        };

        let (attemptid, state, timestart, layout, raw_timelimit, raw_endtime, explicit_pages) =
            if let Some(att) = existing_attempt {
                (
                    att.id,
                    att.state,
                    att.timestart.unwrap_or(0),
                    att.layout,
                    att.timelimit,
                    att.endtime,
                    att.pages,
                )
            } else {
                let mut start_params = serde_json::Map::new();
                start_params.insert("quizid".to_string(), serde_json::json!(module_ref.instance));
                if forcenew {
                    start_params.insert("forcenew".to_string(), serde_json::json!(1));
                }
                if let Some(pw) = params.0.password.as_deref() {
                    start_params.insert(
                        "preflightdata".to_string(),
                        serde_json::json!([
                            { "name": "quizpassword", "value": pw }
                        ]),
                    );
                }

                let start_val = match self
                    .ctx
                    .write_client
                    .call_raw(&creds.token, "mod_quiz_start_attempt", &start_params)
                    .await
                {
                    Ok(v) => v,
                    Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
                    Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
                };

                if let Some(warn_msg) = extract_warnings(&start_val) {
                    return self
                        .ctx
                        .tool_error("api", warn_msg, None, None, None, Some(&creds));
                }

                let started_attempt: AttemptMetaRaw = match start_val
                    .get("attempt")
                    .and_then(|a| serde_json::from_value(a.clone()).ok())
                {
                    Some(a) => a,
                    None => match serde_json::from_value(start_val) {
                        Ok(a) => a,
                        Err(e) => {
                            return self.ctx.tool_error(
                                "protocol",
                                format!("Respuesta de mod_quiz_start_attempt inválida: {e}"),
                                None,
                                None,
                                None,
                                Some(&creds),
                            );
                        }
                    },
                };

                (
                    started_attempt.id,
                    started_attempt.state,
                    started_attempt.timestart.unwrap_or(0),
                    started_attempt.layout,
                    started_attempt.timelimit,
                    started_attempt.endtime,
                    started_attempt.pages,
                )
            };

        let pages =
            explicit_pages.unwrap_or_else(|| parse_layout_pages(layout.as_deref().unwrap_or("")));

        let (timelimit, endtime) = if let (Some(tl), Some(et)) = (raw_timelimit, raw_endtime) {
            (tl, et)
        } else if let Some(tl) = raw_timelimit {
            let et = if tl > 0 { timestart + tl } else { 0 };
            (tl, et)
        } else if let Some(et) = raw_endtime {
            let tl = et.saturating_sub(timestart);
            (tl, et)
        } else {
            let course_str = module_ref.course.to_string();
            let query_quizzes = [
                ("wstoken", creds.token.as_str()),
                ("wsfunction", "mod_quiz_get_quizzes_by_courses"),
                ("moodlewsrestformat", "json"),
                ("courseids[0]", course_str.as_str()),
            ];
            if let Ok(quiz_list) = self
                .ctx
                .client
                .post_webservice::<QuizListResponse>(&query_quizzes, Priority::Interactive)
                .await
            {
                if let Some(q) = quiz_list
                    .quizzes
                    .into_iter()
                    .find(|q| q.id == module_ref.instance)
                {
                    let tl = q.timelimit.unwrap_or(0);
                    let et = if tl > 0 {
                        timestart + tl
                    } else {
                        q.timeclose.unwrap_or(0)
                    };
                    (tl, et)
                } else {
                    (0, 0)
                }
            } else {
                (0, 0)
            }
        };

        let response = QuizStartResponse {
            attemptid,
            state,
            timestart,
            timelimit,
            endtime,
            pages,
        };

        self.ctx.ok_json(&response, "quiz_start", Some(&creds))
    }

    /// Lee una página de un intento de cuestionario en curso y analiza sus preguntas.
    #[rmcp::tool(
        name = "quiz_get_page",
        description = "Lee una página de un intento de cuestionario, analizando las preguntas y guardando automáticamente sus campos ocultos en memoria",
        annotations(read_only_hint = true)
    )]
    pub async fn quiz_get_page(&self, params: Parameters<QuizGetPageParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let attempt_id_str = params.0.attemptid.to_string();
        let page_str = params.0.page.to_string();
        let query = [
            ("wstoken", creds.token.as_str()),
            ("wsfunction", "mod_quiz_get_attempt_data"),
            ("moodlewsrestformat", "json"),
            ("attemptid", attempt_id_str.as_str()),
            ("page", page_str.as_str()),
        ];

        let data_resp: AttemptDataRaw = match self
            .ctx
            .client
            .post_webservice(&query, Priority::Interactive)
            .await
        {
            Ok(r) => r,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let parsed_questions = data_resp
            .questions
            .iter()
            .map(parse_question)
            .collect::<Vec<ParsedQuestion>>();

        {
            let mut hidden_guard = self.ctx.quiz_hidden.lock().unwrap();
            let attempt_entry = hidden_guard.entry(params.0.attemptid).or_default();
            for q in &parsed_questions {
                for (k, v) in &q.hidden_fields {
                    attempt_entry.insert(k.clone(), v.clone());
                }
            }
        }

        let timeleft = data_resp
            .timeleft
            .or_else(|| data_resp.attempt.as_ref().and_then(|a| a.timeleft));

        let response = QuizPageResponse {
            attemptid: params.0.attemptid,
            page: params.0.page,
            nextpage: data_resp.nextpage.unwrap_or(-1),
            timeleft,
            questions: parsed_questions,
        };

        self.ctx.ok_json(&response, "quiz_get_page", Some(&creds))
    }

    /// Guarda las respuestas proporcionadas para las preguntas de un intento sin finalizarlo.
    #[rmcp::tool(
        name = "quiz_save",
        description = "Guarda respuestas intermedias en un intento de cuestionario inyectando automáticamente los campos ocultos y tokens de secuencia correspondientes",
        annotations(destructive_hint = false)
    )]
    pub async fn quiz_save(&self, params: Parameters<QuizSaveParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        // 1. Si no hay campos ocultos en memoria para el intento, intentar precargar la página 0
        let needs_fetch = {
            let guard = self.ctx.quiz_hidden.lock().unwrap();
            match guard.get(&params.0.attemptid) {
                None => true,
                Some(m) if m.is_empty() => true,
                Some(_) => false,
            }
        };

        if needs_fetch {
            let attempt_id_str = params.0.attemptid.to_string();
            let query = [
                ("wstoken", creds.token.as_str()),
                ("wsfunction", "mod_quiz_get_attempt_data"),
                ("moodlewsrestformat", "json"),
                ("attemptid", attempt_id_str.as_str()),
                ("page", "0"),
            ];
            if let Ok(data_resp) = self
                .ctx
                .client
                .post_webservice::<AttemptDataRaw>(&query, Priority::Interactive)
                .await
            {
                let mut guard = self.ctx.quiz_hidden.lock().unwrap();
                let attempt_entry = guard.entry(params.0.attemptid).or_default();
                for q_val in &data_resp.questions {
                    let parsed = parse_question(q_val);
                    for (k, v) in parsed.hidden_fields {
                        attempt_entry.insert(k, v);
                    }
                }
            }
        }

        // 2. Construir mapa de datos con respuestas y campos ocultos
        let mut data_map: HashMap<String, String> = HashMap::new();
        for (name, val) in &params.0.answers {
            let val_str = match val {
                serde_json::Value::Bool(b) => {
                    if *b {
                        "1".to_string()
                    } else {
                        "0".to_string()
                    }
                }
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Null => String::new(),
                other => other.to_string(),
            };
            data_map.insert(name.clone(), val_str);
        }

        let answer_prefixes: Vec<String> = params
            .0
            .answers
            .keys()
            .map(|k| {
                if let Some(idx) = k.find('_') {
                    k[..=idx].to_string()
                } else {
                    k.clone()
                }
            })
            .collect();

        let cached_hidden = {
            let guard = self.ctx.quiz_hidden.lock().unwrap();
            guard.get(&params.0.attemptid).cloned().unwrap_or_default()
        };

        for (hk, hv) in cached_hidden {
            let matches = if answer_prefixes.is_empty() {
                true
            } else {
                answer_prefixes.iter().any(|p| hk.starts_with(p))
            };
            if matches {
                data_map.entry(hk).or_insert(hv);
            }
        }

        let mut save_params = serde_json::Map::new();
        save_params.insert(
            "attemptid".to_string(),
            serde_json::json!(params.0.attemptid),
        );

        let mut data_arr = Vec::new();
        let mut sorted_keys: Vec<_> = data_map.keys().cloned().collect();
        sorted_keys.sort();
        for k in sorted_keys {
            let v = data_map.get(&k).unwrap();
            data_arr.push(serde_json::json!({
                "name": k,
                "value": v,
            }));
        }
        save_params.insert("data".to_string(), serde_json::Value::Array(data_arr));

        let save_resp = match self
            .ctx
            .write_client
            .call_raw(&creds.token, "mod_quiz_save_attempt", &save_params)
            .await
        {
            Ok(v) => v,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        if let Some(warn_msg) = extract_warnings(&save_resp) {
            return self
                .ctx
                .tool_error("api", warn_msg, None, None, None, Some(&creds));
        }

        let response = QuizSaveResponse {
            saved: true,
            attemptid: params.0.attemptid,
        };

        self.ctx.ok_json(&response, "quiz_save", Some(&creds))
    }

    /// Revisa un intento terminado de cuestionario con sus preguntas y retroalimentación.
    #[rmcp::tool(
        name = "quiz_review",
        description = "Revisa un intento finalizado de cuestionario, incluyendo la calificación y las preguntas analizadas con sus respuestas, estado y retroalimentación",
        annotations(read_only_hint = true)
    )]
    pub async fn quiz_review(&self, params: Parameters<QuizReviewParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let attempt_id_str = params.0.attemptid.to_string();
        let page_str = params.0.page.unwrap_or(-1).to_string();
        let query_review = [
            ("wstoken", creds.token.as_str()),
            ("wsfunction", "mod_quiz_get_attempt_review"),
            ("moodlewsrestformat", "json"),
            ("attemptid", attempt_id_str.as_str()),
            ("page", page_str.as_str()),
        ];

        let review_resp: AttemptReviewRaw = match self
            .ctx
            .client
            .post_webservice(&query_review, Priority::Interactive)
            .await
        {
            Ok(r) => r,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let grade = review_resp
            .grade
            .and_then(|v| {
                if let Some(n) = v.as_f64() {
                    Some(n)
                } else if let Some(s) = v.as_str() {
                    s.parse::<f64>().ok()
                } else {
                    None
                }
            })
            .or(review_resp.attempt.sumgrades);

        let parsed_questions = review_resp
            .questions
            .iter()
            .map(parse_question)
            .collect::<Vec<ParsedQuestion>>();

        let response = QuizReviewResponse {
            attemptid: review_resp.attempt.id,
            state: review_resp.attempt.state,
            grade,
            questions: parsed_questions,
        };

        self.ctx.ok_json(&response, "quiz_review", Some(&creds))
    }
}
