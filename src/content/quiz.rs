use crate::content::html_to_markdown;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Opción de respuesta disponible para preguntas de selección única, múltiple o emparejamiento.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionOption {
    pub value: String,
    pub label: String,
}

/// Campo de entrada interactivo dentro de una pregunta de cuestionario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionInput {
    pub name: String,
    /// "single" (radio), "boolean" (checkbox), "text" (input[type=text]), "text_long" (textarea/editor), "select" (select)
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<QuestionOption>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
}

/// Pregunta de cuestionario analizada y estructurada para navegación y resolución por modelos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedQuestion {
    pub slot: i64,
    pub number: Option<String>,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mark: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maxmark: Option<String>,
    pub text_markdown: String,
    pub inputs: Vec<QuestionInput>,
    pub hidden_fields: HashMap<String, String>,
    pub supported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback_markdown: Option<String>,
}

fn clean_label(label: &str) -> String {
    label.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn find_label_for_input(doc: &Html, input_element: &scraper::ElementRef) -> String {
    // 1. Buscar <label for="..."> coincidente con el id del input
    if let Some(id) = input_element.value().attr("id") {
        if !id.is_empty() {
            let label_selector = Selector::parse("label").unwrap();
            for label in doc.select(&label_selector) {
                if label.value().attr("for") == Some(id) {
                    let text = html_to_markdown(&label.inner_html());
                    let cleaned = clean_label(&text);
                    if !cleaned.is_empty() {
                        return cleaned;
                    }
                }
            }
        }
    }

    // 2. Buscar si el input está contenido dentro de un <label>
    let mut parent = input_element.parent();
    while let Some(p) = parent {
        if let Some(el) = scraper::ElementRef::wrap(p) {
            if el.value().name() == "label" {
                let text = html_to_markdown(&el.inner_html());
                let cleaned = clean_label(&text);
                if !cleaned.is_empty() {
                    return cleaned;
                }
            }
        }
        parent = p.parent();
    }

    // 3. Buscar en el contenedor padre (.r0, .r1, .answer > div o div contenedor)
    if let Some(p) = input_element.parent() {
        if let Some(parent_el) = scraper::ElementRef::wrap(p) {
            let mut text_parts = Vec::new();
            for node in parent_el.children() {
                if let Some(child_el) = scraper::ElementRef::wrap(node) {
                    if child_el.value().name() != "input" {
                        text_parts.push(html_to_markdown(&child_el.html()));
                    }
                } else if let Some(t) = node.value().as_text() {
                    text_parts.push(t.to_string());
                }
            }
            let combined = text_parts.join(" ");
            let cleaned = clean_label(&combined);
            if !cleaned.is_empty() {
                return cleaned;
            }
        }
    }

    String::new()
}

/// Parsea el objeto JSON de una pregunta de `mod_quiz_get_attempt_data` o `mod_quiz_get_attempt_review`.
/// Espera encontrar los campos estándar del objeto `questions` de Moodle:
/// `slot`, `type`, `html`, `number` (opcional), `status`, `state`, `mark`, `maxmark`.
pub fn parse_question(question_val: &serde_json::Value) -> ParsedQuestion {
    let slot = question_val
        .get("slot")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let number = question_val.get("number").and_then(|v| {
        if v.is_null() {
            None
        } else if let Some(s) = v.as_str() {
            Some(s.to_string())
        } else {
            v.as_i64().map(|n| n.to_string())
        }
    });

    let q_type = question_val
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let status = question_val
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let state = question_val
        .get("state")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mark = question_val.get("mark").and_then(|v| {
        if v.is_null() {
            None
        } else if let Some(s) = v.as_str() {
            Some(s.to_string())
        } else {
            Some(v.to_string())
        }
    });

    let maxmark = question_val.get("maxmark").and_then(|v| {
        if v.is_null() {
            None
        } else if let Some(s) = v.as_str() {
            Some(s.to_string())
        } else {
            Some(v.to_string())
        }
    });

    let html = question_val
        .get("html")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let doc = Html::parse_fragment(html);

    // 1. Extraer texto del enunciado (.qtext o fallback)
    let qtext_selector = Selector::parse(".qtext").unwrap();
    let text_markdown = if let Some(qtext_elem) = doc.select(&qtext_selector).next() {
        html_to_markdown(&qtext_elem.inner_html())
            .trim()
            .to_string()
    } else {
        let formulation_selector = Selector::parse(".formulation").unwrap();
        if let Some(form_elem) = doc.select(&formulation_selector).next() {
            html_to_markdown(&form_elem.inner_html()).trim().to_string()
        } else {
            html_to_markdown(html).trim().to_string()
        }
    };

    // 2. Extraer campos ocultos (hidden_fields)
    let mut hidden_fields = HashMap::new();
    let hidden_selector = Selector::parse("input[type=\"hidden\"]").unwrap();
    for h in doc.select(&hidden_selector) {
        if let Some(name) = h.value().attr("name") {
            let val = h.value().attr("value").unwrap_or("").to_string();
            hidden_fields.insert(name.to_string(), val);
        }
    }

    // 3. Extraer inputs interactivos preservando orden de aparición
    let mut inputs = Vec::new();
    let mut seen_radio_names = HashSet::new();
    let input_selector = Selector::parse("input, textarea, select").unwrap();

    for elem in doc.select(&input_selector) {
        let tag = elem.value().name();
        if tag == "input" {
            let input_type = elem.value().attr("type").unwrap_or("text").to_lowercase();
            if input_type == "hidden"
                || input_type == "submit"
                || input_type == "button"
                || input_type == "image"
            {
                continue;
            }

            if input_type == "radio" {
                let name = elem.value().attr("name").unwrap_or("").to_string();
                if name.is_empty() || seen_radio_names.contains(&name) {
                    continue;
                }
                seen_radio_names.insert(name.clone());

                let mut options = Vec::new();
                let mut current = None;

                for r in doc.select(&input_selector) {
                    if r.value().name() == "input"
                        && r.value().attr("type").unwrap_or("").to_lowercase() == "radio"
                        && r.value().attr("name") == Some(&name)
                    {
                        let val = r.value().attr("value").unwrap_or("").to_string();
                        let lbl = find_label_for_input(&doc, &r);
                        if r.value().attr("checked").is_some() {
                            current = Some(val.clone());
                        }
                        options.push(QuestionOption {
                            value: val,
                            label: lbl,
                        });
                    }
                }

                inputs.push(QuestionInput {
                    name,
                    kind: "single".to_string(),
                    options: Some(options),
                    current,
                });
            } else if input_type == "checkbox" {
                let name = elem.value().attr("name").unwrap_or("").to_string();
                let val = elem.value().attr("value").unwrap_or("1").to_string();
                let lbl = find_label_for_input(&doc, &elem);
                let current = if elem.value().attr("checked").is_some() {
                    Some(val.clone())
                } else {
                    None
                };

                inputs.push(QuestionInput {
                    name,
                    kind: "boolean".to_string(),
                    options: Some(vec![QuestionOption {
                        value: val,
                        label: lbl,
                    }]),
                    current,
                });
            } else {
                let name = elem.value().attr("name").unwrap_or("").to_string();
                let current = elem
                    .value()
                    .attr("value")
                    .map(|v| v.to_string())
                    .filter(|v| !v.is_empty());

                inputs.push(QuestionInput {
                    name,
                    kind: "text".to_string(),
                    options: None,
                    current,
                });
            }
        } else if tag == "textarea" {
            let name = elem.value().attr("name").unwrap_or("").to_string();
            let text_content = elem.text().collect::<Vec<_>>().join("");
            let current = if text_content.trim().is_empty() {
                None
            } else {
                Some(text_content.trim().to_string())
            };

            inputs.push(QuestionInput {
                name,
                kind: "text_long".to_string(),
                options: None,
                current,
            });
        } else if tag == "select" {
            let name = elem.value().attr("name").unwrap_or("").to_string();
            let mut options = Vec::new();
            let mut current = None;
            let option_selector = Selector::parse("option").unwrap();

            for opt in elem.select(&option_selector) {
                let val = opt.value().attr("value").unwrap_or("").to_string();
                let lbl = clean_label(&opt.text().collect::<Vec<_>>().join(" "));
                if opt.value().attr("selected").is_some() {
                    current = Some(val.clone());
                }
                options.push(QuestionOption {
                    value: val,
                    label: lbl,
                });
            }

            inputs.push(QuestionInput {
                name,
                kind: "select".to_string(),
                options: Some(options),
                current,
            });
        }
    }

    // 4. Retroalimentación (.outcome o .feedback)
    let outcome_selector = Selector::parse(".outcome").unwrap();
    let feedback_selector = Selector::parse(".feedback, .generalfeedback").unwrap();

    let feedback_markdown = if let Some(elem) = doc.select(&outcome_selector).next() {
        let md = html_to_markdown(&elem.html()).trim().to_string();
        if !md.is_empty() {
            Some(md)
        } else {
            None
        }
    } else if let Some(elem) = doc.select(&feedback_selector).next() {
        let md = html_to_markdown(&elem.html()).trim().to_string();
        if !md.is_empty() {
            Some(md)
        } else {
            None
        }
    } else {
        None
    };

    // 5. Determinar si el tipo es soportado
    let is_unsupported_type = matches!(
        q_type.as_str(),
        "ddimageortext" | "ddmarker" | "ddwtos" | "gapselect"
    );
    let has_unsupported_html = html.contains("ddimageortext")
        || html.contains("ddmarker")
        || html.contains("draghomes")
        || html.contains("dropzones")
        || html.contains("droparea");

    let supported = !is_unsupported_type && !has_unsupported_html;
    let raw_html = if !supported {
        Some(html.to_string())
    } else {
        None
    };

    ParsedQuestion {
        slot,
        number,
        r#type: q_type,
        status,
        state,
        mark,
        maxmark,
        text_markdown,
        inputs,
        hidden_fields,
        supported,
        raw_html,
        feedback_markdown,
    }
}
