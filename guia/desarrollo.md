# Desarrollo

Requiere Rust estable (el repositorio fija el canal en `rust-toolchain.toml`).

```bash
git clone https://github.com/robertofabiot/moodle-mcp.git
cd moodle-mcp
cargo build --locked
cargo run -- login
```

## Estructura

| Ruta | Contenido |
|---|---|
| `src/main.rs` | CLI: `login`, `logout`, `whoami` y `serve`. |
| `src/moodle/` | Cliente del web service: reintentos, límite de concurrencia, circuit breaker y archivos. |
| `src/mcp/` | Servidor MCP, herramientas por área e instrucciones para el modelo (`instructions.md`). |
| `src/content/` | Conversión de HTML a Markdown, extracción de PDF, DOCX y PPTX, y análisis de preguntas de cuestionario. |
| `catalog/functions.json` | Firmas de las funciones de Moodle que usan las herramientas. |
| `tests/` | Pruebas con Moodle simulado (`wiremock`) y fixtures anonimizadas. |

## Pruebas

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Ninguna prueba usa la red: todas corren contra un Moodle simulado. La prueba en vivo de `tests/moodle_live.rs` está marcada con `#[ignore]` y lee `UAM_TEST_WSTOKEN` de `.env`. Las capturas reales van en `tests/fixtures_raw/`, que Git ignora, y se anonimizan antes de pasarlas a `tests/fixtures/`. `fixtures_hygiene_test.rs` comprueba que no queden datos personales.

## Publicar una versión

1. Sube la versión en `Cargo.toml` y actualiza `Cargo.lock` con `cargo build`.
2. Crea el tag `vX.Y.Z` y un borrador de release con ese tag desde la cuenta propietaria.
3. El flujo `release.yml` compila los paquetes de Linux, macOS y Windows, y los adjunta al borrador con sus sumas SHA-256.
4. Comprueba las sumas y el binario, y publica el borrador.

## Trabajo futuro

- Exponer cursos, temarios y archivos como recursos MCP (`moodle://…`). Hoy todo se consulta con herramientas.
