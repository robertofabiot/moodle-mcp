const READ_ONLY_EXCEPTIONS: &[&str] = &[
    "core_files_get_unused_draft_itemid",
    "core_calendar_get_calendar_export_token",
    "tool_mobile_get_autologin_key",
    "tool_mobile_get_tokens_for_qr_login",
];

const WRITE_INFIXES: &[&str] = &[
    "_save_",
    "_submit_",
    "_add_",
    "_delete_",
    "_update_",
    "_create_",
    "_send_",
    "_start_",
    "_process_",
    "_view_",
];

const READ_ONLY_INFIXES: &[&str] = &["_get_", "_search_", "_check_", "_is_", "_can_"];

/// Determina si una función de la API de Moodle es de solo lectura.
///
/// Retorna `true` si el nombre contiene `_get_`, `_search_`, `_check_`, `_is_` o `_can_`,
/// y no está en la lista de excepciones que escriben datos o generan secretos,
/// ni contiene verbos de mutación (`_save_`, `_submit_`, `_add_`, etc.).
/// Las funciones con `_view_` no se consideran de lectura porque registran actividad en el servidor.
pub fn is_read_only(function: &str) -> bool {
    if READ_ONLY_EXCEPTIONS.contains(&function) {
        return false;
    }

    if WRITE_INFIXES.iter().any(|infix| function.contains(infix)) {
        return false;
    }

    READ_ONLY_INFIXES
        .iter()
        .any(|infix| function.contains(infix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn test_is_read_only_exceptions_are_false() {
        for exc in READ_ONLY_EXCEPTIONS {
            assert!(
                !is_read_only(exc),
                "La excepción {} debería retornar false",
                exc
            );
        }
    }

    #[test]
    fn test_is_read_only_view_is_false() {
        assert!(!is_read_only("core_course_view_course"));
        assert!(!is_read_only("mod_page_view_page"));
        assert!(!is_read_only("mod_forum_view_forum"));
        assert!(!is_read_only("core_search_view_results"));
    }

    #[test]
    fn test_is_read_only_against_site_info_fixture() {
        let fixture_str = include_str!("../../tests/fixtures/site_info.json");
        let v: Value = serde_json::from_str(fixture_str).unwrap();
        let funcs = v["functions"].as_array().expect("functions array");

        for f in funcs {
            let name = f["name"].as_str().unwrap();

            // Ninguna función con verbos de escritura puede ser de lectura
            for pat in WRITE_INFIXES {
                if name.contains(pat) {
                    assert!(
                        !is_read_only(name),
                        "La función de escritura {} fue erróneamente clasificada como read_only",
                        name
                    );
                }
            }

            // Excepciones conocidas siempre retornan false
            if READ_ONLY_EXCEPTIONS.contains(&name) {
                assert!(
                    !is_read_only(name),
                    "La excepción {} debería ser false",
                    name
                );
            }
        }
    }
}
