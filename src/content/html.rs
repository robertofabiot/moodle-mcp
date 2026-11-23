use crate::moodle::submissions::strip_html_to_plain_text;

/// Convierte HTML de Moodle a formato Markdown usando `htmd::convert`.
/// Si la conversión falla por sintaxis severa o error del parser,
/// recurre a la función de respaldo `strip_html_to_plain_text`.
pub fn html_to_markdown(html: &str) -> String {
    htmd::convert(html).unwrap_or_else(|_| strip_html_to_plain_text(html))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_to_markdown_headings_and_formatting() {
        let html = "<h2>Instrucciones de la tarea</h2><p>Por favor subir el archivo en formato <b>PDF</b> o <i>DOCX</i>.</p><ul><li>Punto 1</li><li>Punto 2</li></ul>";
        let md = html_to_markdown(html);
        assert!(md.contains("## Instrucciones de la tarea"));
        assert!(md.contains("PDF"));
        assert!(md.contains("Punto 1"));
    }

    #[test]
    fn test_html_to_markdown_fallback() {
        let plain = "Texto simple sin etiquetas HTML";
        let md = html_to_markdown(plain);
        assert_eq!(md.trim(), plain);
    }
}
