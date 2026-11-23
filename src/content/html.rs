use crate::moodle::submissions::strip_html_to_plain_text;
use pulldown_cmark::{html, Options, Parser};

/// Convierte HTML de Moodle a formato Markdown usando `htmd::convert`.
/// Si la conversión falla por sintaxis severa o error del parser,
/// recurre a la función de respaldo `strip_html_to_plain_text`.
pub fn html_to_markdown(html: &str) -> String {
    htmd::convert(html).unwrap_or_else(|_| strip_html_to_plain_text(html))
}

/// Convierte Markdown a HTML utilizando `pulldown-cmark` con tablas, listas de tareas y tachado habilitados.
pub fn markdown_to_html(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(markdown, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

/// Convierte texto plano a HTML escapando caracteres especiales (`&`, `<`, `>`, `"`, `'`),
/// transformando saltos de línea en `<br />` y envolviendo el contenido en `<p>...</p>`.
pub fn plain_text_to_html(plain: &str) -> String {
    let escaped = plain
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;");
    let with_br = escaped.replace("\r\n", "\n").replace('\n', "<br />");
    format!("<p>{with_br}</p>")
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

    #[test]
    fn test_markdown_to_html_formatting() {
        let md = "# Entrega Final\n\nTexto con **negrita** y *cursiva*.\n\n- Item 1\n- Item 2";
        let html = markdown_to_html(md);
        assert!(html.contains("<h1>Entrega Final</h1>"));
        assert!(html.contains("<strong>negrita</strong>"));
        assert!(html.contains("<em>cursiva</em>"));
        assert!(html.contains("<li>Item 1</li>"));
    }

    #[test]
    fn test_plain_text_to_html_escaping_and_tags() {
        let plain = "Línea 1 & 2 <script>\"alert('xss')\"</script>\nLínea 2";
        let html = plain_text_to_html(plain);
        assert_eq!(
            html,
            "<p>Línea 1 &amp; 2 &lt;script&gt;&quot;alert(&#39;xss&#39;)&quot;&lt;/script&gt;<br />Línea 2</p>"
        );
    }
}
