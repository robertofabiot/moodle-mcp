use crate::moodle::client::MoodleClient;
use crate::moodle::error::Result;
use crate::moodle::gate::Priority;
use serde::{Deserialize, Serialize};

/// Sección de un curso de Moodle con sus módulos.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub id: i64,
    pub section: i64,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub visible: Option<i64>,
    #[serde(default)]
    pub modules: Vec<Module>,
}

/// Módulo (actividad o recurso) dentro de una sección de un curso.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub id: i64,
    #[serde(default)]
    pub instance: Option<i64>,
    pub modname: String,
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub visible: Option<i64>,
    #[serde(default)]
    pub uservisible: Option<bool>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub contents: Vec<ContentEntry>,
}

/// Entrada de contenido (archivo, URL u otros) dentro de un módulo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentEntry {
    #[serde(rename = "type", default)]
    pub r#type: String,
    pub filename: String,
    #[serde(default)]
    pub filepath: Option<String>,
    #[serde(default)]
    pub filesize: Option<i64>,
    #[serde(default)]
    pub fileurl: Option<String>,
    #[serde(default)]
    pub timemodified: Option<i64>,
    #[serde(default)]
    pub mimetype: Option<String>,
}

impl MoodleClient {
    /// Obtiene el árbol completo de secciones y módulos de un curso mediante `core_course_get_contents`.
    pub async fn course_tree(&self, token: &str, courseid: i64) -> Result<Vec<Section>> {
        let params = [
            ("wstoken", token),
            ("wsfunction", "core_course_get_contents"),
            ("moodlewsrestformat", "json"),
            ("courseid", &courseid.to_string()),
        ];
        self.post_webservice(&params, Priority::Interactive).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserializes_course_contents_fixture() {
        let raw = include_str!("../../tests/fixtures/course_contents.json");
        let sections: Vec<Section> = serde_json::from_str(raw).expect("deserializar fixture");
        assert!(!sections.is_empty(), "debe haber al menos una sección");
        // La sección 0 existe siempre
        let first = &sections[0];
        assert!(first.id > 0);
    }
}
