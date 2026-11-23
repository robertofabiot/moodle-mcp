use crate::moodle::client::MoodleClient;
use crate::moodle::error::{MoodleError, Result};
use crate::moodle::gate::Priority;
use serde::{Deserialize, Serialize};

/// Referencia cruzada a un módulo de curso en Moodle, resolviendo cmid, instancia y curso.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CourseModuleRef {
    pub cmid: i64,
    pub instance: i64,
    pub course: i64,
    pub modname: String,
    pub name: String,
}

#[derive(Deserialize)]
struct CourseModuleResponse {
    cm: CourseModuleItem,
}

#[derive(Deserialize)]
struct CourseModuleItem {
    id: i64,
    instance: i64,
    course: i64,
    modname: String,
    name: String,
}

/// Resuelve una referencia de módulo a partir de su ID de módulo de curso (cmid)
/// invocando `core_course_get_course_module`.
pub async fn resolve_cmid(
    client: &MoodleClient,
    token: &str,
    cmid: i64,
) -> Result<CourseModuleRef> {
    let cmid_str = cmid.to_string();
    let params = [
        ("wstoken", token),
        ("wsfunction", "core_course_get_course_module"),
        ("moodlewsrestformat", "json"),
        ("cmid", &cmid_str),
    ];

    let resp: CourseModuleResponse = client
        .post_webservice(&params, Priority::Interactive)
        .await?;

    Ok(CourseModuleRef {
        cmid: resp.cm.id,
        instance: resp.cm.instance,
        course: resp.cm.course,
        modname: resp.cm.modname,
        name: resp.cm.name,
    })
}

/// Resuelve una referencia de módulo a partir de su nombre de módulo e ID de instancia
/// invocando `core_course_get_course_module_by_instance`.
pub async fn resolve_instance(
    client: &MoodleClient,
    token: &str,
    modname: &str,
    instance: i64,
) -> Result<CourseModuleRef> {
    let instance_str = instance.to_string();
    let params = [
        ("wstoken", token),
        ("wsfunction", "core_course_get_course_module_by_instance"),
        ("moodlewsrestformat", "json"),
        ("module", modname),
        ("instance", &instance_str),
    ];

    let resp: CourseModuleResponse = client
        .post_webservice(&params, Priority::Interactive)
        .await?;

    Ok(CourseModuleRef {
        cmid: resp.cm.id,
        instance: resp.cm.instance,
        course: resp.cm.course,
        modname: resp.cm.modname,
        name: resp.cm.name,
    })
}

/// Resuelve una referencia de módulo a partir de cmid o de instance (exactamente uno de los dos),
/// validando que el tipo de módulo coincida con `modname`.
pub async fn resolve_module(
    client: &MoodleClient,
    token: &str,
    modname: &str,
    cmid: Option<i64>,
    instance: Option<i64>,
) -> Result<CourseModuleRef> {
    match (cmid, instance) {
        (Some(_), Some(_)) | (None, None) => Err(MoodleError::InvalidArguments(
            "Indica exactamente uno de cmid o el id de la actividad".to_string(),
        )),
        (Some(c), None) => {
            let module_ref = resolve_cmid(client, token, c).await?;
            if module_ref.modname != modname {
                return Err(MoodleError::InvalidArguments(format!(
                    "El módulo {c} es de tipo '{}', no '{modname}'",
                    module_ref.modname
                )));
            }
            Ok(module_ref)
        }
        (None, Some(i)) => {
            let module_ref = resolve_instance(client, token, modname, i).await?;
            Ok(module_ref)
        }
    }
}
