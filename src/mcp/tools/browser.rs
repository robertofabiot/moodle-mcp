use crate::mcp::server::MoodleMcp;
use crate::moodle::ids::resolve_cmid;
use crate::moodle::{is_moodle_site_url, MoodleError, Result};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpenInBrowserParams {
    /// URL de la actividad dentro del sitio Moodle configurado
    pub url: Option<String>,
    /// ID positivo de un módulo de curso de Moodle
    pub cmid: Option<i64>,
}

#[derive(Serialize)]
struct BrowserLink {
    url: String,
    login_hint: &'static str,
}

fn browser_url(base: &Url, input: &str) -> Result<Url> {
    let mut directory = base.clone();
    directory.set_path(&format!("{}/", base.path().trim_end_matches('/')));
    let url = directory
        .join(input)
        .map_err(|_| MoodleError::InvalidArguments("URL de Moodle inválida".into()))?;

    if !is_moodle_site_url(base, &url) {
        return Err(MoodleError::InvalidArguments(
            "La URL debe pertenecer al sitio de Moodle configurado".into(),
        ));
    }

    if !url.username().is_empty()
        || url.password().is_some()
        || url.query_pairs().any(|(key, _)| {
            matches!(
                key.to_ascii_lowercase().as_str(),
                "token" | "wstoken" | "privatetoken" | "sesskey"
            )
        })
        || url.path_segments().is_some_and(|mut segments| {
            segments.any(|segment| matches!(segment, "tokenpluginfile.php" | "autologin.php"))
        })
    {
        return Err(MoodleError::InvalidArguments(
            "Indica una URL de Moodle sin credenciales".into(),
        ));
    }

    Ok(url)
}

#[tool_router(router = browser_router, vis = "pub(crate)")]
impl MoodleMcp {
    /// Devuelve un enlace normal de Moodle para usar la sesión del navegador.
    #[rmcp::tool(
        name = "moodle_open_in_browser",
        description = "Devuelve un enlace de Moodle para abrir una actividad en el navegador con tu sesión habitual. Indica url o cmid.",
        annotations(read_only_hint = true)
    )]
    pub async fn moodle_open_in_browser(
        &self,
        params: Parameters<OpenInBrowserParams>,
    ) -> CallToolResult {
        let base = match Url::parse(&self.ctx.config.base_url) {
            Ok(base) if matches!(base.scheme(), "http" | "https") => base,
            _ => {
                let error = MoodleError::Protocol("La URL base de Moodle es inválida".into());
                return self.ctx.tool_error_from_moodle(&error, None);
            }
        };

        let mut creds = None;
        let url = match (params.0.url.as_deref(), params.0.cmid) {
            (Some(url), None) if !url.trim().is_empty() => browser_url(&base, url.trim()),
            (None, Some(cmid)) if cmid > 0 => {
                let session = match self.ctx.session().await {
                    Ok(session) => session,
                    Err(error) => return error,
                };
                let module = match resolve_cmid(&self.ctx.client, &session.token, cmid).await {
                    Ok(module) => module,
                    Err(MoodleError::InvalidToken) => return self.ctx.handle_invalid_token().await,
                    Err(error) => return self.ctx.tool_error_from_moodle(&error, Some(&session)),
                };

                if module.cmid != cmid
                    || module.modname.is_empty()
                    || !module
                        .modname
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
                {
                    let error = MoodleError::Protocol("Moodle devolvió un módulo inválido".into());
                    return self.ctx.tool_error_from_moodle(&error, Some(&session));
                }

                creds = Some(session);
                browser_url(&base, &format!("mod/{}/view.php?id={cmid}", module.modname))
            }
            _ => Err(MoodleError::InvalidArguments(
                "Indica solo una URL no vacía o un cmid positivo".into(),
            )),
        };

        match url {
            Ok(url) => self.ctx.ok_json(
                &BrowserLink {
                    url: url.into(),
                    login_hint:
                        "Abre el enlace en tu navegador e inicia sesión si Moodle lo solicita.",
                },
                "moodle_open_in_browser",
                creds.as_deref(),
            ),
            Err(error) => self.ctx.tool_error_from_moodle(&error, creds.as_deref()),
        }
    }
}
