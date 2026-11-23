use crate::mcp::server::MoodleMcp;
use crate::moodle::classify::is_read_only;
use crate::moodle::error::MoodleError;
use crate::moodle::gate::Priority;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::sync::{Arc, LazyLock};

static CATALOG: LazyLock<Value> = LazyLock::new(|| {
    let raw = include_str!("../../../catalog/functions.json");
    serde_json::from_str(raw).expect("catalog/functions.json debe ser un JSON válido")
});

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListFunctionsParams {
    /// Filtro opcional por subcadena para el nombre de la función (sin distinguir mayúsculas)
    pub filter: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DescribeFunctionParams {
    /// Nombre de la función de Moodle a describir
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenericCallParams {
    /// Nombre de la función del web service de Moodle
    pub function: String,
    /// Parámetros de la llamada en formato clave-valor
    #[serde(default)]
    pub params: Option<Map<String, Value>>,
}

#[derive(Debug, Serialize)]
struct WhoamiOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    sitename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    firstname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lastname: Option<String>,
    fullname: String,
    userid: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    release: Option<String>,
    function_count: usize,
}

#[derive(Debug, Serialize)]
struct FunctionItem {
    name: String,
    read_only: bool,
}

#[derive(Debug, Serialize)]
struct DescribeFunctionOutput {
    name: String,
    enabled: bool,
    read_only: bool,
    signature: Option<Value>,
    hint: String,
}

#[tool_router(router = generic_router, vis = "pub(crate)")]
impl MoodleMcp {
    /// Muestra información de la sesión activa y del sitio Moodle.
    #[rmcp::tool(
        name = "moodle_whoami",
        description = "Muestra información de la sesión activa y del sitio Moodle, incluyendo el conteo de funciones habilitadas",
        annotations(read_only_hint = true)
    )]
    pub async fn moodle_whoami(&self) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let site_info = match self
            .ctx
            .client
            .get_site_info(&creds.token, Priority::Interactive)
            .await
        {
            Ok(info) => {
                let arc_info = Arc::new(info);
                *self.ctx.site_info.write().await = Some(Arc::clone(&arc_info));
                arc_info
            }
            Err(MoodleError::InvalidToken) => {
                self.ctx.invalidate_session().await;
                return self.ctx.invalid_token_error().await;
            }
            Err(e) => return self.ctx.tool_error_from_moodle(&e),
        };

        let function_count = site_info.functions.as_ref().map(|f| f.len()).unwrap_or(0);
        let output = WhoamiOutput {
            sitename: site_info.sitename.clone(),
            username: site_info.username.clone(),
            firstname: site_info.firstname.clone(),
            lastname: site_info.lastname.clone(),
            fullname: site_info.fullname.clone(),
            userid: site_info.userid,
            release: site_info.release.clone(),
            function_count,
        };

        self.ctx.ok_json(&output, "moodle_whoami")
    }

    /// Lista las funciones del web service de Moodle habilitadas para el usuario autenticado.
    #[rmcp::tool(
        name = "moodle_list_functions",
        description = "Lista las funciones del web service de Moodle habilitadas para el usuario autenticado, indicando si son de solo lectura",
        annotations(read_only_hint = true)
    )]
    pub async fn moodle_list_functions(
        &self,
        params: Parameters<ListFunctionsParams>,
    ) -> CallToolResult {
        let functions = match self.ctx.get_enabled_functions().await {
            Ok(f) => f,
            Err(err) => return err,
        };

        let filter_str = params.0.filter.as_deref().unwrap_or("").to_lowercase();
        let items: Vec<FunctionItem> = functions
            .iter()
            .filter(|f| {
                if filter_str.is_empty() {
                    true
                } else {
                    f.name.to_lowercase().contains(&filter_str)
                }
            })
            .map(|f| FunctionItem {
                read_only: is_read_only(&f.name),
                name: f.name.clone(),
            })
            .collect();

        self.ctx.ok_json(&items, "moodle_list_functions")
    }

    /// Describe una función del web service de Moodle.
    #[rmcp::tool(
        name = "moodle_describe_function",
        description = "Describe una función del web service de Moodle, indicando si está habilitada y si es de solo lectura",
        annotations(read_only_hint = true)
    )]
    pub async fn moodle_describe_function(
        &self,
        params: Parameters<DescribeFunctionParams>,
    ) -> CallToolResult {
        let name = params.0.name.trim();
        let enabled = match self.ctx.is_function_enabled(name).await {
            Ok(e) => e,
            Err(err) => return err,
        };

        let read_only = is_read_only(name);
        let (signature, hint) = if let Some(sig) = CATALOG.get(name) {
            (
                Some(sig.clone()),
                "Firma disponible en el catálogo".to_string(),
            )
        } else {
            (
                None,
                "Para descubrir los parámetros esperados, invoca la función con moodle_read o moodle_call con parámetros vacíos y consulta el debuginfo de Moodle.".to_string(),
            )
        };

        let output = DescribeFunctionOutput {
            name: name.to_string(),
            enabled,
            read_only,
            signature,
            hint,
        };

        self.ctx.ok_json(&output, "moodle_describe_function")
    }

    /// Invoca una función de solo lectura del web service de Moodle.
    #[rmcp::tool(
        name = "moodle_read",
        description = "Invoca una función de solo lectura del web service de Moodle. Rechaza funciones de mutación o escritura.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    pub async fn moodle_read(&self, params: Parameters<GenericCallParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let function = params.0.function.trim();

        // 1. Verificar si está habilitada
        match self.ctx.is_function_enabled(function).await {
            Ok(true) => {}
            Ok(false) => {
                return self.ctx.tool_error(
                    "not_enabled",
                    format!("La función '{function}' no está habilitada en este sitio de Moodle"),
                    None,
                    None,
                    None,
                );
            }
            Err(err) => return err,
        }

        // 2. Verificar que sea de solo lectura
        if !is_read_only(function) {
            return self.ctx.tool_error(
                "invalid_arguments",
                format!("La función '{function}' no es de solo lectura. Usa moodle_call para invocarla."),
                None,
                None,
                Some("Usa moodle_call para ejecutar funciones de escritura o mutación".to_string()),
            );
        }

        // 3. Invocar con client de lectura
        let empty_params = Map::new();
        let call_params = params.0.params.as_ref().unwrap_or(&empty_params);
        match self
            .ctx
            .client
            .call_raw(&creds.token, function, call_params)
            .await
        {
            Ok(data) => self.ctx.ok_json(&data, function),
            Err(MoodleError::InvalidToken) => {
                self.ctx.invalidate_session().await;
                self.ctx.invalid_token_error().await
            }
            Err(e) => self.ctx.tool_error_from_moodle(&e),
        }
    }

    /// Invoca cualquier función del web service de Moodle (lectura o escritura).
    #[rmcp::tool(
        name = "moodle_call",
        description = "Invoca cualquier función del web service de Moodle. Utiliza reintentos para lectura y exactamente 1 intento sin reintento para escrituras.",
        annotations(destructive_hint = true, open_world_hint = true)
    )]
    pub async fn moodle_call(&self, params: Parameters<GenericCallParams>) -> CallToolResult {
        let creds = match self.ctx.session().await {
            Ok(c) => c,
            Err(err) => return err,
        };

        let function = params.0.function.trim();

        // 1. Verificar si está habilitada
        match self.ctx.is_function_enabled(function).await {
            Ok(true) => {}
            Ok(false) => {
                return self.ctx.tool_error(
                    "not_enabled",
                    format!("La función '{function}' no está habilitada en este sitio de Moodle"),
                    None,
                    None,
                    None,
                );
            }
            Err(err) => return err,
        }

        // 2. Seleccionar client: write_client (0 reintentos) para escrituras, client normal para lecturas
        let client = if is_read_only(function) {
            &self.ctx.client
        } else {
            &self.ctx.write_client
        };

        // 3. Invocar
        let empty_params = Map::new();
        let call_params = params.0.params.as_ref().unwrap_or(&empty_params);
        match client.call_raw(&creds.token, function, call_params).await {
            Ok(data) => self.ctx.ok_json(&data, function),
            Err(MoodleError::InvalidToken) => {
                self.ctx.invalidate_session().await;
                self.ctx.invalid_token_error().await
            }
            Err(e) => self.ctx.tool_error_from_moodle(&e),
        }
    }
}
