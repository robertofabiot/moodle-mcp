use crate::content::{html_to_markdown, markdown_to_html};
use crate::mcp::server::MoodleMcp;
use crate::mcp::tools::{extract_warnings, format_epoch_iso};
use crate::moodle::error::MoodleError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListConversationsParams {
    /// Cantidad máxima de conversaciones a devolver (por defecto 20)
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetConversationParams {
    /// ID de la conversación a consultar
    pub conversationid: i64,
    /// Cantidad máxima de mensajes a devolver (por defecto 50)
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SendMessageParams {
    /// ID de una conversación existente. Mutuamente excluyente con touserid.
    pub conversationid: Option<i64>,
    /// ID del usuario destinatario para mensaje directo. Mutuamente excluyente con conversationid.
    pub touserid: Option<i64>,
    /// Contenido del mensaje en formato Markdown
    pub text: String,
}

// Modelos de salida

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ListConversationsResponse {
    pub userid: i64,
    pub conversations: Vec<ConversationSummary>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ConversationSummary {
    pub id: i64,
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subname: Option<String>,
    #[serde(rename = "type")]
    pub conv_type: Option<i64>,
    pub membercount: i64,
    pub ismuted: bool,
    pub isfavourite: bool,
    pub isread: bool,
    pub unreadcount: i64,
    pub members: Vec<ConversationMember>,
    pub messages: Vec<ConversationMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lastmessage_markdown: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ConversationMember {
    pub id: i64,
    pub fullname: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isonline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isblocked: Option<bool>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ConversationMessage {
    pub id: i64,
    pub useridfrom: i64,
    pub text_markdown: String,
    pub timecreated: i64,
    pub timecreated_iso: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct GetConversationResponse {
    pub conversationid: i64,
    pub members: Vec<ConversationMember>,
    pub messages: Vec<ConversationMessage>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct SendMessageResponse {
    pub sent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversationid: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub touserid: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messageid: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub useridfrom: Option<i64>,
    pub text_markdown: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timecreated: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timecreated_iso: Option<String>,
}

// Modelos internos para deserializar respuestas de Moodle

#[derive(Deserialize)]
struct ConversationsRawResponse {
    #[serde(default)]
    conversations: Vec<ConversationRaw>,
}

#[derive(Deserialize)]
struct ConversationRaw {
    id: i64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    subname: Option<String>,
    #[serde(rename = "type", default)]
    conv_type: Option<i64>,
    #[serde(default)]
    membercount: Option<i64>,
    #[serde(default)]
    ismuted: Option<bool>,
    #[serde(default)]
    isfavourite: Option<bool>,
    #[serde(default)]
    isread: Option<bool>,
    #[serde(default)]
    unreadcount: Option<i64>,
    #[serde(default)]
    members: Vec<ConversationMemberRaw>,
    #[serde(default)]
    messages: Vec<ConversationMessageRaw>,
    #[serde(default)]
    lastmessage: Option<String>,
}

#[derive(Deserialize)]
struct ConversationMessagesRawResponse {
    #[serde(default)]
    members: Vec<ConversationMemberRaw>,
    #[serde(default)]
    messages: Vec<ConversationMessageRaw>,
}

#[derive(Deserialize)]
struct ConversationMemberRaw {
    id: i64,
    #[serde(default)]
    fullname: String,
    #[serde(default)]
    isonline: Option<bool>,
    #[serde(default)]
    isblocked: Option<bool>,
}

#[derive(Deserialize)]
struct ConversationMessageRaw {
    id: i64,
    #[serde(default)]
    useridfrom: Option<i64>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    timecreated: Option<i64>,
}

fn map_member(m: ConversationMemberRaw) -> ConversationMember {
    ConversationMember {
        id: m.id,
        fullname: m.fullname,
        isonline: m.isonline,
        isblocked: m.isblocked,
    }
}

fn map_message(m: ConversationMessageRaw) -> ConversationMessage {
    let timecreated = m.timecreated.unwrap_or(0);
    ConversationMessage {
        id: m.id,
        useridfrom: m.useridfrom.unwrap_or(0),
        text_markdown: html_to_markdown(m.text.as_deref().unwrap_or(""))
            .trim()
            .to_string(),
        timecreated,
        timecreated_iso: format_epoch_iso(timecreated),
    }
}

enum MessageTarget {
    Conversation(i64),
    User(i64),
}

#[tool_router(router = messaging_router, vis = "pub(crate)")]
impl MoodleMcp {
    /// Lista las conversaciones de mensajería privada del estudiante autenticado.
    #[rmcp::tool(
        name = "list_conversations",
        description = "Lista las conversaciones de mensajería privada del usuario autenticado con participantes y el último mensaje convertido a Markdown",
        annotations(read_only_hint = true)
    )]
    pub async fn list_conversations(
        &self,
        params: Parameters<ListConversationsParams>,
    ) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let limit = params.0.limit.unwrap_or(20);
        let mut req_params = serde_json::Map::new();
        req_params.insert("userid".to_string(), serde_json::json!(creds.userid));
        req_params.insert("limitnum".to_string(), serde_json::json!(limit));

        let raw_val = match self
            .ctx
            .client
            .call_raw(&creds.token, "core_message_get_conversations", &req_params)
            .await
        {
            Ok(v) => v,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let raw_resp: ConversationsRawResponse = match serde_json::from_value(raw_val) {
            Ok(r) => r,
            Err(e) => {
                return self.ctx.tool_error(
                    "protocol",
                    format!("Respuesta inválida de core_message_get_conversations: {e}"),
                    None,
                    None,
                    None,
                    Some(&creds),
                );
            }
        };

        let conversations = raw_resp
            .conversations
            .into_iter()
            .map(|c| {
                let members: Vec<ConversationMember> =
                    c.members.into_iter().map(map_member).collect();
                let membercount = c.membercount.unwrap_or(members.len() as i64);
                let messages: Vec<ConversationMessage> =
                    c.messages.into_iter().map(map_message).collect();
                let lastmessage_markdown = c
                    .lastmessage
                    .as_deref()
                    .map(|txt| html_to_markdown(txt).trim().to_string());

                ConversationSummary {
                    id: c.id,
                    name: c.name,
                    subname: c.subname,
                    conv_type: c.conv_type,
                    membercount,
                    ismuted: c.ismuted.unwrap_or(false),
                    isfavourite: c.isfavourite.unwrap_or(false),
                    isread: c.isread.unwrap_or(true),
                    unreadcount: c.unreadcount.unwrap_or(0),
                    members,
                    messages,
                    lastmessage_markdown,
                }
            })
            .collect();

        let response = ListConversationsResponse {
            userid: creds.userid,
            conversations,
        };

        self.ctx
            .ok_json(&response, "list_conversations", Some(&creds))
    }

    /// Obtiene los mensajes y participantes de una conversación de mensajería privada.
    #[rmcp::tool(
        name = "get_conversation",
        description = "Obtiene los mensajes más recientes y los miembros de una conversación específica convirtiendo el contenido a Markdown",
        annotations(read_only_hint = true)
    )]
    pub async fn get_conversation(
        &self,
        params: Parameters<GetConversationParams>,
    ) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let limit = params.0.limit.unwrap_or(50);
        let mut req_params = serde_json::Map::new();
        req_params.insert("currentuserid".to_string(), serde_json::json!(creds.userid));
        req_params.insert(
            "convid".to_string(),
            serde_json::json!(params.0.conversationid),
        );
        req_params.insert("limitnum".to_string(), serde_json::json!(limit));
        req_params.insert("newest".to_string(), serde_json::json!(true));

        let raw_val = match self
            .ctx
            .client
            .call_raw(
                &creds.token,
                "core_message_get_conversation_messages",
                &req_params,
            )
            .await
        {
            Ok(v) => v,
            Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
            Err(e) => return self.ctx.tool_error_from_moodle(&e, Some(&creds)),
        };

        let raw_resp: ConversationMessagesRawResponse = match serde_json::from_value(raw_val) {
            Ok(r) => r,
            Err(e) => {
                return self.ctx.tool_error(
                    "protocol",
                    format!("Respuesta inválida de core_message_get_conversation_messages: {e}"),
                    None,
                    None,
                    None,
                    Some(&creds),
                );
            }
        };

        let members = raw_resp.members.into_iter().map(map_member).collect();
        let messages = raw_resp.messages.into_iter().map(map_message).collect();

        let response = GetConversationResponse {
            conversationid: params.0.conversationid,
            members,
            messages,
        };

        self.ctx
            .ok_json(&response, "get_conversation", Some(&creds))
    }

    /// Envía un mensaje privado a una conversación existente o directamente a un usuario.
    #[rmcp::tool(
        name = "send_message",
        description = "Envía un mensaje privado en Markdown a una conversación existente (conversationid) o a un usuario destinatario (touserid) de forma mutuamente excluyente",
        annotations(destructive_hint = true)
    )]
    pub async fn send_message(&self, params: Parameters<SendMessageParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let target = match (params.0.conversationid, params.0.touserid) {
            (Some(cid), None) => MessageTarget::Conversation(cid),
            (None, Some(uid)) => MessageTarget::User(uid),
            (Some(_), Some(_)) | (None, None) => {
                return self.ctx.tool_error(
                    "invalid_arguments",
                    "Debes especificar exactamente uno de conversationid o touserid",
                    None,
                    None,
                    None,
                    Some(&creds),
                );
            }
        };

        if params.0.text.trim().is_empty() {
            return self.ctx.tool_error(
                "invalid_arguments",
                "El texto del mensaje no puede estar vacío",
                None,
                None,
                None,
                Some(&creds),
            );
        }

        let html = markdown_to_html(&params.0.text);

        let (function_name, req_params, target_conv_id, target_user_id) = match target {
            MessageTarget::Conversation(cid) => {
                let mut map = serde_json::Map::new();
                map.insert("conversationid".to_string(), serde_json::json!(cid));
                map.insert(
                    "messages".to_string(),
                    serde_json::json!([
                        {
                            "text": html,
                            "textformat": 1,
                        }
                    ]),
                );
                (
                    "core_message_send_messages_to_conversation",
                    map,
                    Some(cid),
                    None,
                )
            }
            MessageTarget::User(uid) => {
                let mut map = serde_json::Map::new();
                map.insert(
                    "messages".to_string(),
                    serde_json::json!([
                        {
                            "touserid": uid,
                            "text": html,
                            "textformat": 1,
                        }
                    ]),
                );
                ("core_message_send_instant_messages", map, None, Some(uid))
            }
        };

        let resp = match self
            .ctx
            .write_client
            .call_raw(&creds.token, function_name, &req_params)
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

        let items = match resp.as_array() {
            Some(arr) => arr,
            None => {
                return self.ctx.tool_error(
                    "protocol",
                    format!("Respuesta inválida de {function_name}: se esperaba un array"),
                    None,
                    None,
                    None,
                    Some(&creds),
                );
            }
        };

        let mut error_messages = Vec::new();
        for item in items {
            if let Some(err_msg) = item.get("errormessage").and_then(|v| v.as_str()) {
                let trimmed = err_msg.trim();
                if !trimmed.is_empty() {
                    error_messages.push(trimmed.to_string());
                }
            }
        }
        if !error_messages.is_empty() {
            return self.ctx.tool_error(
                "api",
                error_messages.join("; "),
                None,
                None,
                None,
                Some(&creds),
            );
        }

        let first = items.first();
        let messageid = first.and_then(|item| {
            item.get("msgid")
                .or_else(|| item.get("id"))
                .and_then(|v| v.as_i64())
        });
        let conversationid = first
            .and_then(|item| item.get("conversationid").and_then(|v| v.as_i64()))
            .or(target_conv_id);
        let useridfrom = first.and_then(|item| item.get("useridfrom").and_then(|v| v.as_i64()));
        let timecreated = first.and_then(|item| item.get("timecreated").and_then(|v| v.as_i64()));
        let timecreated_iso = timecreated.and_then(format_epoch_iso);
        let text_markdown = first
            .and_then(|item| item.get("text").and_then(|v| v.as_str()))
            .map(|t| html_to_markdown(t).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| params.0.text.trim().to_string());

        let response = SendMessageResponse {
            sent: true,
            conversationid,
            touserid: target_user_id,
            messageid,
            useridfrom,
            text_markdown,
            timecreated,
            timecreated_iso,
        };

        self.ctx.ok_json(&response, "send_message", Some(&creds))
    }
}
