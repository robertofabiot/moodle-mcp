use crate::content::{html_to_markdown, markdown_to_html, plain_text_to_html};
use crate::mcp::server::MoodleMcp;
use crate::mcp::tools::{extract_warnings, format_epoch_iso};
use crate::moodle::error::MoodleError;
use crate::moodle::files::authorize_file_url;
use crate::moodle::gate::Priority;
use crate::moodle::ids::resolve_module;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use url::Url;

fn default_onlinetext_format() -> String {
    "markdown".to_string()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetAssignmentParams {
    /// ID del módulo de curso (cmid). Mutuamente excluyente con assignid.
    pub cmid: Option<i64>,
    /// ID de la instancia de la tarea (assignid). Mutuamente excluyente con cmid.
    pub assignid: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SubmitAssignmentParams {
    /// ID del módulo de curso (cmid). Mutuamente excluyente con assignid.
    pub cmid: Option<i64>,
    /// ID de la instancia de la tarea (assignid). Mutuamente excluyente con cmid.
    pub assignid: Option<i64>,
    /// Rutas locales de los archivos a subir como entrega
    pub files: Option<Vec<String>>,
    /// Texto en línea para la entrega
    pub onlinetext: Option<String>,
    /// Formato del texto en línea: "markdown" (por defecto), "html" o "plain"
    #[serde(default = "default_onlinetext_format")]
    pub onlinetext_format: String,
    /// Si es true, conserva y reenvía los archivos existentes junto con los nuevos
    #[serde(default)]
    pub keep_existing_files: bool,
    /// Si es true y la tarea usa borradores, envía la entrega definitivamente a calificación
    #[serde(default)]
    pub r#final: bool,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct AssignmentDetail {
    pub cmid: i64,
    pub instance: i64,
    pub courseid: i64,
    pub name: String,
    pub intro_markdown: String,
    pub intro_files: Vec<AssignmentFile>,
    pub allowsubmissionsfromdate: i64,
    pub allowsubmissionsfromdate_iso: Option<String>,
    pub duedate: i64,
    pub duedate_iso: Option<String>,
    pub cutoffdate: i64,
    pub cutoffdate_iso: Option<String>,
    pub gradingduedate: i64,
    pub gradingduedate_iso: Option<String>,
    pub submission_types: AssignmentSubmissionTypes,
    pub requiresubmissionstatement: bool,
    pub submissiondrafts: bool,
    pub teamsubmission: bool,
    pub status: AssignmentSubmissionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<AssignmentGradeInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback_markdown: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct AssignmentSubmissionTypes {
    pub file: Option<FileSubmissionConfig>,
    pub onlinetext: Option<OnlineTextSubmissionConfig>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct FileSubmissionConfig {
    pub enabled: bool,
    pub maxfilesubmissions: i64,
    pub maxsubmissionsizebytes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filetypeslist: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct OnlineTextSubmissionConfig {
    pub enabled: bool,
    pub wordlimit: i64,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct AssignmentFile {
    pub filename: String,
    pub fileurl: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesize: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mimetype: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct AssignmentSubmissionStatus {
    pub status: Option<String>,
    pub canedit: bool,
    pub cansubmit: bool,
    pub extensionduedate: Option<i64>,
    pub extensionduedate_iso: Option<String>,
    pub submitted_files: Vec<AssignmentFile>,
    pub submitted_text: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct AssignmentGradeInfo {
    pub grade: Option<String>,
    pub grader: Option<i64>,
    pub timemodified: Option<i64>,
}

// Modelos internos para deserializar respuestas de Moodle

#[derive(Deserialize)]
struct AssignGetAssignmentsResponse {
    #[serde(default)]
    courses: Vec<AssignCourse>,
}

#[derive(Deserialize)]
struct AssignCourse {
    #[serde(default)]
    assignments: Vec<AssignItem>,
}

#[derive(Deserialize)]
struct AssignItem {
    id: i64,
    #[allow(dead_code)]
    cmid: i64,
    #[allow(dead_code)]
    course: i64,
    name: String,
    #[serde(default)]
    intro: Option<String>,
    #[serde(default)]
    introfiles: Option<Vec<AssignFileRaw>>,
    #[serde(default)]
    introattachments: Option<Vec<AssignFileRaw>>,
    #[serde(default)]
    duedate: Option<i64>,
    #[serde(default)]
    allowsubmissionsfromdate: Option<i64>,
    #[serde(default)]
    cutoffdate: Option<i64>,
    #[serde(default)]
    gradingduedate: Option<i64>,
    #[serde(default)]
    submissiondrafts: Option<i64>,
    #[serde(default)]
    requiresubmissionstatement: Option<i64>,
    #[serde(default)]
    teamsubmission: Option<i64>,
    #[serde(default)]
    configs: Option<Vec<AssignConfigRaw>>,
}

#[derive(Deserialize)]
struct AssignFileRaw {
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    fileurl: Option<String>,
    #[serde(default)]
    filesize: Option<i64>,
    #[serde(default)]
    mimetype: Option<String>,
}

#[derive(Deserialize)]
struct AssignConfigRaw {
    #[serde(default)]
    plugin: String,
    #[serde(default)]
    subtype: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    value: String,
}

#[derive(Deserialize)]
struct SubmissionStatusResponse {
    #[serde(default)]
    lastattempt: Option<SubmissionLastAttemptRaw>,
    #[serde(default)]
    feedback: Option<SubmissionFeedbackRaw>,
}

#[derive(Deserialize)]
struct SubmissionLastAttemptRaw {
    #[serde(default)]
    submission: Option<SubmissionItemRaw>,
    #[serde(default)]
    teamsubmission: Option<SubmissionItemRaw>,
    #[serde(default)]
    canedit: Option<bool>,
    #[serde(default)]
    cansubmit: Option<bool>,
    #[serde(default)]
    extensionduedate: Option<i64>,
}

#[derive(Deserialize)]
struct SubmissionItemRaw {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    plugins: Option<Vec<SubmissionPluginRaw>>,
}

#[derive(Deserialize)]
struct SubmissionPluginRaw {
    #[serde(rename = "type")]
    plugin_type: Option<String>,
    #[serde(default)]
    fileareas: Option<Vec<SubmissionFileAreaRaw>>,
    #[serde(default)]
    editorfields: Option<Vec<SubmissionEditorFieldRaw>>,
}

#[derive(Deserialize)]
struct SubmissionFileAreaRaw {
    #[serde(default)]
    files: Option<Vec<AssignFileRaw>>,
}

#[derive(Deserialize)]
struct SubmissionEditorFieldRaw {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct SubmissionFeedbackRaw {
    #[serde(default)]
    grade: Option<SubmissionGradeRaw>,
    #[serde(default)]
    gradefordisplay: Option<String>,
    #[serde(default)]
    plugins: Option<Vec<SubmissionPluginRaw>>,
}

#[derive(Deserialize)]
struct SubmissionGradeRaw {
    #[serde(default)]
    grade: Option<String>,
    #[serde(default)]
    grader: Option<i64>,
    #[serde(default)]
    timemodified: Option<i64>,
}

fn parse_submission_types(configs: Option<&[AssignConfigRaw]>) -> AssignmentSubmissionTypes {
    let mut file_config = None;
    let mut onlinetext_config = None;

    if let Some(cfgs) = configs {
        let mut file_enabled = false;
        let mut maxfilesubmissions = 0i64;
        let mut maxsubmissionsizebytes = 0i64;
        let mut filetypeslist = None;
        let mut has_file_cfg = false;

        let mut onlinetext_enabled = false;
        let mut wordlimit = 0i64;
        let mut has_onlinetext_cfg = false;

        for c in cfgs {
            if c.subtype == "assignsubmission" {
                match c.plugin.as_str() {
                    "file" => {
                        has_file_cfg = true;
                        match c.name.as_str() {
                            "enabled" => file_enabled = c.value == "1",
                            "maxfilesubmissions" => {
                                maxfilesubmissions = c.value.parse().unwrap_or(0);
                            }
                            "maxsubmissionsizebytes" => {
                                maxsubmissionsizebytes = c.value.parse().unwrap_or(0);
                            }
                            "filetypeslist" => {
                                let trimmed = c.value.trim();
                                if !trimmed.is_empty() {
                                    filetypeslist = Some(trimmed.to_string());
                                }
                            }
                            _ => {}
                        }
                    }
                    "onlinetext" => {
                        has_onlinetext_cfg = true;
                        match c.name.as_str() {
                            "enabled" => onlinetext_enabled = c.value == "1",
                            "wordlimit" => {
                                wordlimit = c.value.parse().unwrap_or(0);
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }

        if has_file_cfg {
            file_config = Some(FileSubmissionConfig {
                enabled: file_enabled,
                maxfilesubmissions,
                maxsubmissionsizebytes,
                filetypeslist,
            });
        }
        if has_onlinetext_cfg {
            onlinetext_config = Some(OnlineTextSubmissionConfig {
                enabled: onlinetext_enabled,
                wordlimit,
            });
        }
    }

    AssignmentSubmissionTypes {
        file: file_config,
        onlinetext: onlinetext_config,
    }
}

fn parse_intro_files(assign: &AssignItem) -> Vec<AssignmentFile> {
    let mut intro_files = Vec::new();
    if let Some(ref attachments) = assign.introattachments {
        for f in attachments {
            if let (Some(filename), Some(fileurl)) = (&f.filename, &f.fileurl) {
                intro_files.push(AssignmentFile {
                    filename: filename.clone(),
                    fileurl: fileurl.clone(),
                    filesize: f.filesize,
                    mimetype: f.mimetype.clone(),
                });
            }
        }
    }
    if let Some(ref files) = assign.introfiles {
        for f in files {
            if let (Some(filename), Some(fileurl)) = (&f.filename, &f.fileurl) {
                if !intro_files
                    .iter()
                    .any(|existing| existing.fileurl == *fileurl)
                {
                    intro_files.push(AssignmentFile {
                        filename: filename.clone(),
                        fileurl: fileurl.clone(),
                        filesize: f.filesize,
                        mimetype: f.mimetype.clone(),
                    });
                }
            }
        }
    }
    intro_files
}

fn parse_submission_status(raw: &SubmissionStatusResponse) -> AssignmentSubmissionStatus {
    let mut status = None;
    let mut canedit = false;
    let mut cansubmit = false;
    let mut extensionduedate = None;
    let mut extensionduedate_iso = None;
    let mut submitted_files = Vec::new();
    let mut submitted_text = None;

    if let Some(ref la) = raw.lastattempt {
        status = la
            .submission
            .as_ref()
            .and_then(|s| s.status.clone())
            .or_else(|| la.teamsubmission.as_ref().and_then(|ts| ts.status.clone()));
        canedit = la.canedit.unwrap_or(false);
        cansubmit = la.cansubmit.unwrap_or(false);
        if let Some(ext) = la.extensionduedate {
            if ext > 0 {
                extensionduedate = Some(ext);
                extensionduedate_iso = format_epoch_iso(ext);
            }
        }

        let submissions = [la.submission.as_ref(), la.teamsubmission.as_ref()];
        for sub in submissions.into_iter().flatten() {
            if let Some(ref plugins) = sub.plugins {
                for p in plugins {
                    let ptype = p.plugin_type.as_deref().unwrap_or("");
                    if ptype == "file" {
                        if let Some(ref fileareas) = p.fileareas {
                            for fa in fileareas {
                                if let Some(ref files) = fa.files {
                                    for f in files {
                                        if let (Some(filename), Some(fileurl)) =
                                            (&f.filename, &f.fileurl)
                                        {
                                            if !submitted_files.iter().any(
                                                |existing: &AssignmentFile| {
                                                    existing.fileurl == *fileurl
                                                },
                                            ) {
                                                submitted_files.push(AssignmentFile {
                                                    filename: filename.clone(),
                                                    fileurl: fileurl.clone(),
                                                    filesize: f.filesize,
                                                    mimetype: f.mimetype.clone(),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else if ptype == "onlinetext" && submitted_text.is_none() {
                        if let Some(ref fields) = p.editorfields {
                            for f in fields {
                                if let Some(ref text) = f.text {
                                    if !text.trim().is_empty() {
                                        submitted_text =
                                            Some(html_to_markdown(text).trim().to_string());
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    AssignmentSubmissionStatus {
        status,
        canedit,
        cansubmit,
        extensionduedate,
        extensionduedate_iso,
        submitted_files,
        submitted_text,
    }
}

fn parse_grade_and_feedback(
    raw: &SubmissionStatusResponse,
) -> (Option<AssignmentGradeInfo>, Option<String>) {
    let mut grade_info = None;
    let mut feedback_markdown = None;

    if let Some(ref fb) = raw.feedback {
        let grade_val = fb
            .gradefordisplay
            .as_ref()
            .filter(|g| !g.trim().is_empty())
            .cloned()
            .or_else(|| fb.grade.as_ref().and_then(|g| g.grade.clone()));

        if grade_val.is_some() || fb.grade.is_some() {
            grade_info = Some(AssignmentGradeInfo {
                grade: grade_val,
                grader: fb.grade.as_ref().and_then(|g| g.grader),
                timemodified: fb.grade.as_ref().and_then(|g| g.timemodified),
            });
        }

        if let Some(ref plugins) = fb.plugins {
            for p in plugins {
                if let Some(ref fields) = p.editorfields {
                    for f in fields {
                        if let Some(ref text) = f.text {
                            if !text.trim().is_empty() {
                                feedback_markdown = Some(html_to_markdown(text).trim().to_string());
                                break;
                            }
                        }
                    }
                }
                if feedback_markdown.is_some() {
                    break;
                }
            }
        }
    }

    (grade_info, feedback_markdown)
}

#[tool_router(router = assignments_router, vis = "pub(crate)")]
impl MoodleMcp {
    /// Consulta la información detallada de una tarea en Moodle.
    #[rmcp::tool(
        name = "get_assignment",
        description = "Consulta la información detallada de una tarea, incluyendo enunciado en Markdown, configuración de entrega, fechas límites, estado de entrega del estudiante y calificación",
        annotations(read_only_hint = true)
    )]
    pub async fn get_assignment(&self, params: Parameters<GetAssignmentParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let module_ref = match resolve_module(
            &self.ctx.client,
            &creds.token,
            "assign",
            params.0.cmid,
            params.0.assignid,
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
        let query = [
            ("wstoken", creds.token.as_str()),
            ("wsfunction", "mod_assign_get_assignments"),
            ("moodlewsrestformat", "json"),
            ("courseids[0]", course_str.as_str()),
        ];

        let assign_resp: AssignGetAssignmentsResponse = match self
            .ctx
            .client
            .post_webservice(&query, Priority::Interactive)
            .await
        {
            Ok(r) => r,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let assignment_item = assign_resp
            .courses
            .into_iter()
            .flat_map(|c| c.assignments)
            .find(|a| a.id == module_ref.instance);

        let assign = match assignment_item {
            Some(a) => a,
            None => {
                return self.ctx.tool_error(
                    "invalid_arguments",
                    format!(
                        "No se encontró la tarea con instancia {} en el curso {}",
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
        let status_query = [
            ("wstoken", creds.token.as_str()),
            ("wsfunction", "mod_assign_get_submission_status"),
            ("moodlewsrestformat", "json"),
            ("assignid", instance_str.as_str()),
        ];

        let status_resp: SubmissionStatusResponse = match self
            .ctx
            .client
            .post_webservice(&status_query, Priority::Interactive)
            .await
        {
            Ok(r) => r,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let status = parse_submission_status(&status_resp);
        let (grade, feedback_markdown) = parse_grade_and_feedback(&status_resp);

        let intro_files = parse_intro_files(&assign);
        let submission_types = parse_submission_types(assign.configs.as_deref());

        let detail = AssignmentDetail {
            cmid: module_ref.cmid,
            instance: module_ref.instance,
            courseid: module_ref.course,
            name: assign.name,
            intro_markdown: assign
                .intro
                .as_deref()
                .map(html_to_markdown)
                .unwrap_or_default(),
            intro_files,
            allowsubmissionsfromdate: assign.allowsubmissionsfromdate.unwrap_or(0),
            allowsubmissionsfromdate_iso: format_epoch_iso(
                assign.allowsubmissionsfromdate.unwrap_or(0),
            ),
            duedate: assign.duedate.unwrap_or(0),
            duedate_iso: format_epoch_iso(assign.duedate.unwrap_or(0)),
            cutoffdate: assign.cutoffdate.unwrap_or(0),
            cutoffdate_iso: format_epoch_iso(assign.cutoffdate.unwrap_or(0)),
            gradingduedate: assign.gradingduedate.unwrap_or(0),
            gradingduedate_iso: format_epoch_iso(assign.gradingduedate.unwrap_or(0)),
            submission_types,
            requiresubmissionstatement: assign.requiresubmissionstatement.unwrap_or(0) == 1,
            submissiondrafts: assign.submissiondrafts.unwrap_or(0) == 1,
            teamsubmission: assign.teamsubmission.unwrap_or(0) == 1,
            status,
            grade,
            feedback_markdown,
        };

        self.ctx.ok_json(&detail, "get_assignment", Some(&creds))
    }

    /// Guarda o envía una entrega de tarea con archivos y/o texto en línea.
    #[rmcp::tool(
        name = "submit_assignment",
        description = "Guarda o envía una entrega de tarea con archivos locales y/o texto en línea, con opción de conservar archivos previos y envío definitivo a calificación",
        annotations(destructive_hint = true)
    )]
    pub async fn submit_assignment(
        &self,
        params: Parameters<SubmitAssignmentParams>,
    ) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        // 1. Validar parámetros mutuamente excluyentes y resolver módulo
        let module_ref = match resolve_module(
            &self.ctx.client,
            &creds.token,
            "assign",
            params.0.cmid,
            params.0.assignid,
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

        // 2. Validar que al menos uno de files o onlinetext esté presente y no vacío
        let new_files = params.0.files.unwrap_or_default();
        let onlinetext = params.0.onlinetext;
        let has_files = !new_files.is_empty();
        let has_text = onlinetext
            .as_ref()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false);

        if !has_files && !has_text {
            return self.ctx.tool_error(
                "invalid_arguments",
                "Debes proporcionar files o onlinetext",
                None,
                None,
                None,
                Some(&creds),
            );
        }

        let base_url = match Url::parse(self.ctx.client.base_url()) {
            Ok(u) => u,
            Err(e) => {
                return self.ctx.tool_error(
                    "invalid_arguments",
                    format!("URL base de Moodle inválida: {e}"),
                    None,
                    None,
                    None,
                    Some(&creds),
                );
            }
        };

        // 3. Manejo de archivos (files)
        let mut files_itemid: Option<i64> = None;

        if params.0.keep_existing_files {
            let instance_str = module_ref.instance.to_string();
            let status_query = [
                ("wstoken", creds.token.as_str()),
                ("wsfunction", "mod_assign_get_submission_status"),
                ("moodlewsrestformat", "json"),
                ("assignid", instance_str.as_str()),
            ];

            let status_resp: SubmissionStatusResponse = match self
                .ctx
                .client
                .post_webservice(&status_query, Priority::Interactive)
                .await
            {
                Ok(r) => r,
                Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
                Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
            };

            let prev_status = parse_submission_status(&status_resp);
            let prev_files = prev_status.submitted_files;

            let temp_dir = match tempfile::tempdir() {
                Ok(td) => td,
                Err(e) => {
                    return self.ctx.tool_error(
                        "io",
                        format!("Fallo al crear directorio temporal: {e}"),
                        None,
                        None,
                        None,
                        Some(&creds),
                    );
                }
            };

            let mut all_upload_paths = Vec::new();

            for pf in &prev_files {
                let auth_url = match authorize_file_url(&base_url, &pf.fileurl, &creds.token) {
                    Ok(u) => u,
                    Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
                };
                let dl = match self
                    .ctx
                    .file_client
                    .download(&auth_url, None, temp_dir.path())
                    .await
                {
                    Ok(d) => d,
                    Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
                };
                all_upload_paths.push(dl.path);
            }

            for nf in &new_files {
                all_upload_paths.push(PathBuf::from(nf));
            }

            if !all_upload_paths.is_empty() {
                let upload_res = match self
                    .ctx
                    .file_client
                    .upload(&base_url, &creds.token, &all_upload_paths, None)
                    .await
                {
                    Ok(res) => res,
                    Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
                };
                files_itemid = Some(upload_res.itemid);
            }
        } else if has_files {
            let paths: Vec<PathBuf> = new_files.iter().map(PathBuf::from).collect();
            let upload_res = match self
                .ctx
                .file_client
                .upload(&base_url, &creds.token, &paths, None)
                .await
            {
                Ok(res) => res,
                Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
            };
            files_itemid = Some(upload_res.itemid);
        }

        // 4. Manejo de texto en línea (onlinetext)
        let mut onlinetext_html_and_itemid: Option<(String, i64)> = None;
        if let Some(text) = onlinetext {
            if !text.trim().is_empty() {
                let html = match params.0.onlinetext_format.as_str() {
                    "plain" => plain_text_to_html(&text),
                    "html" => text,
                    _ => markdown_to_html(&text),
                };

                let empty_map = serde_json::Map::new();
                let draft_resp = match self
                    .ctx
                    .client
                    .call_raw(
                        &creds.token,
                        "core_files_get_unused_draft_itemid",
                        &empty_map,
                    )
                    .await
                {
                    Ok(v) => v,
                    Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
                    Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
                };

                let draft_itemid = draft_resp
                    .as_i64()
                    .or_else(|| draft_resp.get("value").and_then(|v| v.as_i64()))
                    .or_else(|| draft_resp.get("itemid").and_then(|v| v.as_i64()))
                    .unwrap_or_default();

                onlinetext_html_and_itemid = Some((html, draft_itemid));
            }
        }

        // 5. Guardar entrega (mod_assign_save_submission) con write_client
        let mut save_params = serde_json::Map::new();
        save_params.insert(
            "assignmentid".to_string(),
            serde_json::json!(module_ref.instance),
        );

        let mut plugindata = serde_json::Map::new();
        if let Some(fid) = files_itemid {
            plugindata.insert("files_filemanager".to_string(), serde_json::json!(fid));
        }
        if let Some((html, text_itemid)) = onlinetext_html_and_itemid {
            plugindata.insert(
                "onlinetext_editor".to_string(),
                serde_json::json!({
                    "text": html,
                    "format": 1,
                    "itemid": text_itemid,
                }),
            );
        }
        save_params.insert(
            "plugindata".to_string(),
            serde_json::Value::Object(plugindata),
        );

        let save_resp = match self
            .ctx
            .write_client
            .call_raw(&creds.token, "mod_assign_save_submission", &save_params)
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

        // 6. Entrega final si params.final es true y submissiondrafts == 1
        if params.0.r#final {
            let course_str = module_ref.course.to_string();
            let query = [
                ("wstoken", creds.token.as_str()),
                ("wsfunction", "mod_assign_get_assignments"),
                ("moodlewsrestformat", "json"),
                ("courseids[0]", course_str.as_str()),
            ];

            let assign_resp: AssignGetAssignmentsResponse = match self
                .ctx
                .client
                .post_webservice(&query, Priority::Interactive)
                .await
            {
                Ok(r) => r,
                Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
                Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
            };

            let has_drafts = assign_resp
                .courses
                .into_iter()
                .flat_map(|c| c.assignments)
                .find(|a| a.id == module_ref.instance)
                .map(|a| a.submissiondrafts.unwrap_or(0) == 1)
                .unwrap_or(false);

            if has_drafts {
                let mut submit_params = serde_json::Map::new();
                submit_params.insert(
                    "assignmentid".to_string(),
                    serde_json::json!(module_ref.instance),
                );
                submit_params.insert(
                    "acceptsubmissionstatement".to_string(),
                    serde_json::json!(1),
                );

                let submit_resp = match self
                    .ctx
                    .write_client
                    .call_raw(
                        &creds.token,
                        "mod_assign_submit_for_grading",
                        &submit_params,
                    )
                    .await
                {
                    Ok(v) => v,
                    Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
                    Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
                };

                if let Some(warn_msg) = extract_warnings(&submit_resp) {
                    return self
                        .ctx
                        .tool_error("api", warn_msg, None, None, None, Some(&creds));
                }
            }
        }

        // 7. Consultar nuevamente mod_assign_get_submission_status
        let instance_str = module_ref.instance.to_string();
        let status_query = [
            ("wstoken", creds.token.as_str()),
            ("wsfunction", "mod_assign_get_submission_status"),
            ("moodlewsrestformat", "json"),
            ("assignid", instance_str.as_str()),
        ];

        let final_status_resp: SubmissionStatusResponse = match self
            .ctx
            .client
            .post_webservice(&status_query, Priority::Interactive)
            .await
        {
            Ok(r) => r,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let final_status = parse_submission_status(&final_status_resp);
        self.ctx
            .ok_json(&final_status, "submit_assignment", Some(&creds))
    }
}
