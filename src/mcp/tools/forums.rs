use crate::content::{html_to_markdown, markdown_to_html};
use crate::mcp::server::MoodleMcp;
use crate::mcp::tools::{extract_warnings, format_epoch_iso};
use crate::moodle::error::MoodleError;
use crate::moodle::ids::resolve_module;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use url::Url;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForumListParams {
    /// ID numérico del curso en Moodle (courseid)
    pub courseid: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForumListDiscussionsParams {
    /// ID del módulo de curso (cmid). Mutuamente excluyente con forumid.
    pub cmid: Option<i64>,
    /// ID de la instancia del foro (forumid). Mutuamente excluyente con cmid.
    pub forumid: Option<i64>,
    /// Número de página de resultados (inicia en 0, por defecto 0)
    pub page: Option<i64>,
    /// Cantidad máxima de discusiones por página (por defecto 20)
    pub perpage: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForumGetDiscussionParams {
    /// ID numérico de la discusión de foro (discussionid)
    pub discussionid: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForumReplyParams {
    /// ID del mensaje (postid) al que se responde
    pub postid: i64,
    /// Cuerpo de la respuesta en formato Markdown
    pub message: String,
    /// Asunto opcional de la respuesta (si se omite o es vacío, se construye desde el mensaje padre)
    pub subject: Option<String>,
    /// Rutas locales opcionales de archivos a adjuntar
    pub attachments: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForumNewDiscussionParams {
    /// ID del módulo de curso (cmid). Mutuamente excluyente con forumid.
    pub cmid: Option<i64>,
    /// ID de la instancia del foro (forumid). Mutuamente excluyente con cmid.
    pub forumid: Option<i64>,
    /// Asunto del nuevo tema de discusión
    pub subject: String,
    /// Cuerpo del mensaje inicial en formato Markdown
    pub message: String,
    /// ID del grupo (por defecto 0 para todos los grupos o sin grupos)
    pub groupid: Option<i64>,
    /// Rutas locales opcionales de archivos a adjuntar
    pub attachments: Option<Vec<String>>,
}

// Modelos de salida

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ForumListResponse {
    pub courseid: i64,
    pub forums: Vec<ForumSummary>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ForumSummary {
    pub id: i64,
    pub cmid: i64,
    pub course: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub forum_type: String,
    pub intro_markdown: String,
    pub numdiscussions: i64,
    pub duedate: i64,
    pub cutoffdate: i64,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct ForumListDiscussionsResponse {
    pub cmid: i64,
    pub instance: i64,
    pub courseid: i64,
    pub discussions: Vec<ForumDiscussionSummary>,
    pub warnings: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ForumDiscussionSummary {
    pub id: i64,
    pub discussion: i64,
    pub name: String,
    pub subject: String,
    pub message_markdown: String,
    pub userfullname: String,
    pub timemodified: i64,
    pub timemodified_iso: Option<String>,
    pub numreplies: i64,
    pub pinned: bool,
    pub locked: bool,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct ForumGetDiscussionResponse {
    pub discussionid: i64,
    pub posts: Vec<ForumPostSummary>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ForumPostSummary {
    pub postid: i64,
    pub parentid: i64,
    pub author: String,
    pub subject: String,
    pub message_markdown: String,
    pub timecreated: i64,
    pub timecreated_iso: Option<String>,
    pub attachments: Vec<ForumAttachment>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ForumAttachment {
    pub filename: String,
    pub filesize: Option<i64>,
    pub fileurl: String,
    pub mimetype: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ForumReplyResponse {
    pub postid: i64,
    pub subject: String,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ForumNewDiscussionResponse {
    pub cmid: i64,
    pub instance: i64,
    pub courseid: i64,
    pub discussionid: i64,
}

// Modelos internos para deserializar respuestas de Moodle

#[derive(Deserialize)]
struct ForumItemRaw {
    id: i64,
    #[serde(default)]
    cmid: Option<i64>,
    #[serde(default)]
    coursemodule: Option<i64>,
    #[serde(default)]
    course: Option<i64>,
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    forum_type: String,
    #[serde(default)]
    intro: Option<String>,
    #[serde(default)]
    numdiscussions: Option<i64>,
    #[serde(default)]
    duedate: Option<i64>,
    #[serde(default)]
    cutoffdate: Option<i64>,
}

#[derive(Deserialize)]
struct ForumDiscussionsRawResponse {
    #[serde(default)]
    discussions: Vec<ForumDiscussionRaw>,
    #[serde(default)]
    warnings: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct ForumDiscussionRaw {
    id: i64,
    #[serde(default)]
    discussion: Option<i64>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    userfullname: Option<String>,
    #[serde(default)]
    timemodified: Option<i64>,
    #[serde(default)]
    modified: Option<i64>,
    #[serde(default)]
    numreplies: Option<i64>,
    #[serde(default)]
    pinned: Option<bool>,
    #[serde(default)]
    locked: Option<bool>,
}

#[derive(Deserialize)]
struct ForumDiscussionPostsRawResponse {
    #[serde(default)]
    posts: Vec<ForumPostRaw>,
}

#[derive(Deserialize)]
struct ForumPostRaw {
    id: i64,
    #[serde(default)]
    parentid: Option<i64>,
    #[serde(default)]
    author: Option<ForumPostAuthorRaw>,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    timecreated: Option<i64>,
    #[serde(default)]
    attachments: Vec<ForumAttachmentRaw>,
}

#[derive(Deserialize)]
struct ForumPostAuthorRaw {
    #[serde(default)]
    fullname: Option<String>,
}

#[derive(Deserialize)]
struct ForumAttachmentRaw {
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    filesize: Option<i64>,
    #[serde(default)]
    fileurl: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    mimetype: Option<String>,
}

/// Construye el asunto de respuesta anteponiendo "Re: " si el asunto padre no comienza ya con dicho prefijo.
fn build_reply_subject(parent_subject: &str) -> String {
    let trimmed = parent_subject.trim();
    if trimmed.is_empty() {
        "Re: Publicación".to_string()
    } else if trimmed
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("re: "))
    {
        trimmed.to_string()
    } else {
        format!("Re: {trimmed}")
    }
}

#[tool_router(router = forums_router, vis = "pub(crate)")]
impl MoodleMcp {
    /// Lista los foros de un curso específico en Moodle.
    #[rmcp::tool(
        name = "forum_list",
        description = "Lista los foros de un curso en Moodle con su tipo, descripción en Markdown, cantidad de discusiones y fechas configuradas",
        annotations(read_only_hint = true)
    )]
    pub async fn forum_list(&self, params: Parameters<ForumListParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let mut req_params = serde_json::Map::new();
        req_params.insert(
            "courseids".to_string(),
            serde_json::json!([params.0.courseid]),
        );

        let raw_val = match self
            .ctx
            .client
            .call_raw(&creds.token, "mod_forum_get_forums_by_courses", &req_params)
            .await
        {
            Ok(v) => v,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let raw_items: Vec<ForumItemRaw> = if raw_val.is_array() {
            match serde_json::from_value(raw_val) {
                Ok(items) => items,
                Err(e) => {
                    return self.ctx.tool_error(
                        "protocol",
                        format!("Respuesta inválida de mod_forum_get_forums_by_courses: {e}"),
                        None,
                        None,
                        None,
                        Some(&creds),
                    );
                }
            }
        } else if let Some(forums_arr) = raw_val.get("forums") {
            match serde_json::from_value(forums_arr.clone()) {
                Ok(items) => items,
                Err(e) => {
                    return self.ctx.tool_error(
                        "protocol",
                        format!("Respuesta inválida de mod_forum_get_forums_by_courses: {e}"),
                        None,
                        None,
                        None,
                        Some(&creds),
                    );
                }
            }
        } else {
            return self.ctx.tool_error(
                "protocol",
                "Respuesta inesperada de mod_forum_get_forums_by_courses: se esperaba una lista de foros",
                None,
                None,
                None,
                Some(&creds),
            );
        };

        let forums = raw_items
            .into_iter()
            .map(|f| ForumSummary {
                id: f.id,
                cmid: f.cmid.or(f.coursemodule).unwrap_or(0),
                course: f.course.unwrap_or(params.0.courseid),
                name: f.name,
                forum_type: f.forum_type,
                intro_markdown: html_to_markdown(f.intro.as_deref().unwrap_or(""))
                    .trim()
                    .to_string(),
                numdiscussions: f.numdiscussions.unwrap_or(0),
                duedate: f.duedate.unwrap_or(0),
                cutoffdate: f.cutoffdate.unwrap_or(0),
            })
            .collect();

        let response = ForumListResponse {
            courseid: params.0.courseid,
            forums,
        };

        self.ctx.ok_json(&response, "forum_list", Some(&creds))
    }

    /// Lista las discusiones o hilos de un foro en Moodle.
    #[rmcp::tool(
        name = "forum_list_discussions",
        description = "Lista las discusiones de un foro aceptando cmid o forumid, con resúmenes en Markdown, autor, fechas y paginación",
        annotations(read_only_hint = true)
    )]
    pub async fn forum_list_discussions(
        &self,
        params: Parameters<ForumListDiscussionsParams>,
    ) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let resolved = match resolve_module(
            &self.ctx.client,
            &creds.token,
            "forum",
            params.0.cmid,
            params.0.forumid,
        )
        .await
        {
            Ok(m) => m,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
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

        let page = params.0.page.unwrap_or(0);
        let perpage = params.0.perpage.unwrap_or(20);

        let mut req_params = serde_json::Map::new();
        req_params.insert("forumid".to_string(), serde_json::json!(resolved.instance));
        req_params.insert("page".to_string(), serde_json::json!(page));
        req_params.insert("perpage".to_string(), serde_json::json!(perpage));

        let raw_val = match self
            .ctx
            .client
            .call_raw(&creds.token, "mod_forum_get_forum_discussions", &req_params)
            .await
        {
            Ok(v) => v,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let raw_resp: ForumDiscussionsRawResponse = match serde_json::from_value(raw_val) {
            Ok(r) => r,
            Err(e) => {
                return self.ctx.tool_error(
                    "protocol",
                    format!("Respuesta inválida de mod_forum_get_forum_discussions: {e}"),
                    None,
                    None,
                    None,
                    Some(&creds),
                );
            }
        };

        let discussions = raw_resp
            .discussions
            .into_iter()
            .map(|d| {
                let timemodified = d.timemodified.or(d.modified).unwrap_or(0);
                let subject = d.subject.unwrap_or_else(|| d.name.clone());
                ForumDiscussionSummary {
                    id: d.id,
                    discussion: d.discussion.unwrap_or(d.id),
                    name: d.name,
                    subject,
                    message_markdown: html_to_markdown(d.message.as_deref().unwrap_or(""))
                        .trim()
                        .to_string(),
                    userfullname: d.userfullname.unwrap_or_default(),
                    timemodified,
                    timemodified_iso: format_epoch_iso(timemodified),
                    numreplies: d.numreplies.unwrap_or(0),
                    pinned: d.pinned.unwrap_or(false),
                    locked: d.locked.unwrap_or(false),
                }
            })
            .collect();

        let response = ForumListDiscussionsResponse {
            cmid: resolved.cmid,
            instance: resolved.instance,
            courseid: resolved.course,
            discussions,
            warnings: raw_resp.warnings,
        };

        self.ctx
            .ok_json(&response, "forum_list_discussions", Some(&creds))
    }

    /// Obtiene todos los mensajes y respuestas de una discusión de foro en orden cronológico.
    #[rmcp::tool(
        name = "forum_get_discussion",
        description = "Obtiene los mensajes de una discusión de foro ordenados cronológicamente con su contenido en Markdown y archivos adjuntos",
        annotations(read_only_hint = true)
    )]
    pub async fn forum_get_discussion(
        &self,
        params: Parameters<ForumGetDiscussionParams>,
    ) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let mut req_params = serde_json::Map::new();
        req_params.insert(
            "discussionid".to_string(),
            serde_json::json!(params.0.discussionid),
        );
        req_params.insert("sortby".to_string(), serde_json::json!("created"));
        req_params.insert("sortdirection".to_string(), serde_json::json!("ASC"));

        let raw_val = match self
            .ctx
            .client
            .call_raw(&creds.token, "mod_forum_get_discussion_posts", &req_params)
            .await
        {
            Ok(v) => v,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let raw_resp: ForumDiscussionPostsRawResponse = match serde_json::from_value(raw_val) {
            Ok(r) => r,
            Err(e) => {
                return self.ctx.tool_error(
                    "protocol",
                    format!("Respuesta inválida de mod_forum_get_discussion_posts: {e}"),
                    None,
                    None,
                    None,
                    Some(&creds),
                );
            }
        };

        let posts = raw_resp
            .posts
            .into_iter()
            .map(|p| {
                let timecreated = p.timecreated.unwrap_or(0);
                let attachments = p
                    .attachments
                    .into_iter()
                    .map(|a| ForumAttachment {
                        filename: a.filename.unwrap_or_default(),
                        filesize: a.filesize,
                        fileurl: a.fileurl.or(a.url).unwrap_or_default(),
                        mimetype: a.mimetype,
                    })
                    .collect();

                ForumPostSummary {
                    postid: p.id,
                    parentid: p.parentid.unwrap_or(0),
                    author: p.author.and_then(|a| a.fullname).unwrap_or_default(),
                    subject: p.subject,
                    message_markdown: html_to_markdown(p.message.as_deref().unwrap_or(""))
                        .trim()
                        .to_string(),
                    timecreated,
                    timecreated_iso: format_epoch_iso(timecreated),
                    attachments,
                }
            })
            .collect();

        let response = ForumGetDiscussionResponse {
            discussionid: params.0.discussionid,
            posts,
        };

        self.ctx
            .ok_json(&response, "forum_get_discussion", Some(&creds))
    }

    /// Publica una respuesta a un mensaje existente dentro de una discusión de foro.
    #[rmcp::tool(
        name = "forum_reply",
        description = "Publica una respuesta en Markdown a un mensaje de foro, resolviendo automáticamente el asunto si se omite y subiendo archivos adjuntos opcionales",
        annotations(destructive_hint = true)
    )]
    pub async fn forum_reply(&self, params: Parameters<ForumReplyParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        if params.0.message.trim().is_empty() {
            return self.ctx.tool_error(
                "invalid_arguments",
                "El mensaje de la respuesta no puede estar vacío",
                None,
                None,
                None,
                Some(&creds),
            );
        }

        let subject = match params
            .0
            .subject
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(s) => s.to_string(),
            None => {
                let mut post_params = serde_json::Map::new();
                post_params.insert("postid".to_string(), serde_json::json!(params.0.postid));

                let post_resp = match self
                    .ctx
                    .client
                    .call_raw(&creds.token, "mod_forum_get_discussion_post", &post_params)
                    .await
                {
                    Ok(v) => v,
                    Err(MoodleError::InvalidToken) => {
                        return self.ctx.handle_invalid_token().await;
                    }
                    Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
                };

                let parent_subject = post_resp
                    .get("post")
                    .and_then(|p| p.get("subject"))
                    .and_then(|s| s.as_str())
                    .or_else(|| post_resp.get("subject").and_then(|s| s.as_str()));

                match parent_subject {
                    Some(subj) => build_reply_subject(subj),
                    None => {
                        if let Some(warn_msg) = extract_warnings(&post_resp) {
                            return self.ctx.tool_error(
                                "api",
                                warn_msg,
                                None,
                                None,
                                None,
                                Some(&creds),
                            );
                        }
                        return self.ctx.tool_error(
                            "protocol",
                            "Respuesta inválida de mod_forum_get_discussion_post: falta el asunto del mensaje padre",
                            None,
                            None,
                            None,
                            Some(&creds),
                        );
                    }
                }
            }
        };

        let attachment_paths: Vec<PathBuf> = params
            .0
            .attachments
            .unwrap_or_default()
            .into_iter()
            .map(PathBuf::from)
            .collect();

        let mut attachments_itemid: Option<i64> = None;
        if !attachment_paths.is_empty() {
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

            let upload = match self
                .ctx
                .file_client
                .upload(&base_url, &creds.token, &attachment_paths, None)
                .await
            {
                Ok(res) => res,
                Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
                Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
            };
            attachments_itemid = Some(upload.itemid);
        }

        let html = markdown_to_html(&params.0.message);

        let mut req_params = serde_json::Map::new();
        req_params.insert("postid".to_string(), serde_json::json!(params.0.postid));
        req_params.insert("subject".to_string(), serde_json::json!(subject));
        req_params.insert("message".to_string(), serde_json::json!(html));
        req_params.insert("messageformat".to_string(), serde_json::json!(1));
        if let Some(itemid) = attachments_itemid {
            req_params.insert(
                "options".to_string(),
                serde_json::json!([
                    {
                        "name": "attachmentsid",
                        "value": itemid.to_string(),
                    }
                ]),
            );
        }

        let resp = match self
            .ctx
            .write_client
            .call_raw(&creds.token, "mod_forum_add_discussion_post", &req_params)
            .await
        {
            Ok(v) => v,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        if let Some(warn_msg) = extract_warnings(&resp) {
            return self
                .ctx
                .tool_error("api", warn_msg, None, None, None, Some(&creds));
        }

        let created_postid = match resp
            .get("postid")
            .or_else(|| resp.get("id"))
            .and_then(|v| v.as_i64())
        {
            Some(id) => id,
            None => {
                return self.ctx.tool_error(
                    "protocol",
                    "Respuesta inválida de mod_forum_add_discussion_post: falta el identificador postid creado",
                    None,
                    None,
                    None,
                    Some(&creds),
                );
            }
        };

        let response = ForumReplyResponse {
            postid: created_postid,
            subject,
        };

        self.ctx.ok_json(&response, "forum_reply", Some(&creds))
    }

    /// Crea un nuevo tema de discusión en un foro de Moodle.
    #[rmcp::tool(
        name = "forum_new_discussion",
        description = "Abre una nueva discusión en un foro aceptando cmid o forumid, asunto, mensaje en Markdown, grupo opcional y archivos adjuntos",
        annotations(destructive_hint = true)
    )]
    pub async fn forum_new_discussion(
        &self,
        params: Parameters<ForumNewDiscussionParams>,
    ) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let subject = params.0.subject.trim().to_string();
        if subject.is_empty() {
            return self.ctx.tool_error(
                "invalid_arguments",
                "El asunto de la discusión no puede estar vacío",
                None,
                None,
                None,
                Some(&creds),
            );
        }

        if params.0.message.trim().is_empty() {
            return self.ctx.tool_error(
                "invalid_arguments",
                "El mensaje de la discusión no puede estar vacío",
                None,
                None,
                None,
                Some(&creds),
            );
        }

        let resolved = match resolve_module(
            &self.ctx.client,
            &creds.token,
            "forum",
            params.0.cmid,
            params.0.forumid,
        )
        .await
        {
            Ok(m) => m,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
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

        let attachment_paths: Vec<PathBuf> = params
            .0
            .attachments
            .unwrap_or_default()
            .into_iter()
            .map(PathBuf::from)
            .collect();

        let mut attachments_itemid: Option<i64> = None;
        if !attachment_paths.is_empty() {
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

            let upload = match self
                .ctx
                .file_client
                .upload(&base_url, &creds.token, &attachment_paths, None)
                .await
            {
                Ok(res) => res,
                Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
                Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
            };
            attachments_itemid = Some(upload.itemid);
        }

        let html = markdown_to_html(&params.0.message);

        // No enviar messageformat: Moodle 4.5 lo rechaza en mod_forum_add_discussion.
        let mut req_params = serde_json::Map::new();
        req_params.insert("forumid".to_string(), serde_json::json!(resolved.instance));
        req_params.insert("subject".to_string(), serde_json::json!(subject));
        req_params.insert("message".to_string(), serde_json::json!(html));
        req_params.insert(
            "groupid".to_string(),
            serde_json::json!(params.0.groupid.unwrap_or(0)),
        );
        if let Some(itemid) = attachments_itemid {
            req_params.insert(
                "options".to_string(),
                serde_json::json!([
                    {
                        "name": "attachmentsid",
                        "value": itemid.to_string(),
                    }
                ]),
            );
        }

        let resp = match self
            .ctx
            .write_client
            .call_raw(&creds.token, "mod_forum_add_discussion", &req_params)
            .await
        {
            Ok(v) => v,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        if let Some(warn_msg) = extract_warnings(&resp) {
            return self
                .ctx
                .tool_error("api", warn_msg, None, None, None, Some(&creds));
        }

        let discussionid = match resp.get("discussionid").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => {
                return self.ctx.tool_error(
                    "protocol",
                    "Respuesta inválida de mod_forum_add_discussion: falta el identificador discussionid",
                    None,
                    None,
                    None,
                    Some(&creds),
                );
            }
        };

        let response = ForumNewDiscussionResponse {
            cmid: resolved.cmid,
            instance: resolved.instance,
            courseid: resolved.course,
            discussionid,
        };

        self.ctx
            .ok_json(&response, "forum_new_discussion", Some(&creds))
    }
}
