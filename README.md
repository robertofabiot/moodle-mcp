<p align="center">
  <img src="assets/logo.svg" alt="Logo de moodle-mcp" width="160">
</p>

<h1 align="center">moodle-mcp</h1>

<p align="center">
  <b>Tu cuenta de Moodle, conectada a tu asistente de IA.</b><br>
  Servidor MCP para trabajar con tus cursos, notas, tareas, foros y cuestionarios desde el chat.
</p>

<p align="center">
  <a href="https://github.com/robertofabiot/moodle-mcp/actions/workflows/ci.yml"><img src="https://github.com/robertofabiot/moodle-mcp/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/robertofabiot/moodle-mcp/releases/latest"><img src="https://img.shields.io/github/v/release/robertofabiot/moodle-mcp?color=FF6B00" alt="Última versión"></a>
  <img src="https://img.shields.io/badge/Rust-stable-orange?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/MCP-stdio-0F1115" alt="MCP por stdio">
  <img src="https://img.shields.io/badge/Linux%20%C2%B7%20macOS%20%C2%B7%20Windows-161A22" alt="Linux, macOS y Windows">
</p>

---

## De qué trata

Moodle tiene una API completa, pero tu asistente de IA no tiene forma de usarla. moodle-mcp hace de puente: inicias sesión una vez en la terminal y el asistente puede leer tus cursos, descargar el material, revisar tus notas y, cuando se lo pides, entregar una tarea o responder en un foro. Todo con los permisos de tu propia cuenta.

Funciona con cualquier Moodle que tenga habilitados los servicios web móviles, los mismos que usa la app oficial.

## Qué le puedes pedir

> ¿Qué entregas tengo esta semana y cuáles me faltan?
>
> Descarga el material nuevo de Estadística y hazme un resumen.
>
> Lee la consigna de la tarea 3 y sube mi informe como borrador.
>
> ¿Qué se discutió en el foro de la semana 4?

| Área | Qué cubre |
|---|---|
| **Cursos y notas** | Temarios, cambios recientes, calificaciones con retroalimentación y próximas fechas de entrega. |
| **Archivos** | Descarga y lectura de PDF, Word, PowerPoint, HTML e imágenes con `moodle_view`. Réplica incremental de un curso completo en tu disco. |
| **Tareas** | Enunciados, requisitos y estado. Entregas con archivos o texto, como borrador o definitivas. |
| **Cuestionarios** | Reglas e historial de intentos. Inicio, respuesta, envío y revisión. |
| **Foros y mensajes** | Lectura de discusiones y conversaciones. Respuestas, temas nuevos y mensajes privados. |
| **API completa** | Cualquier otra función del web service habilitada para tu cuenta. |

La lista completa está en la [referencia de herramientas](guia/herramientas.md).

## Empezar

**1. Instala.** Descarga el paquete de tu sistema desde [Releases](https://github.com/robertofabiot/moodle-mcp/releases/latest), comprueba la suma SHA-256 y deja `moodle-mcp` en una carpeta de tu `PATH`. Si tienes Rust:

```bash
cargo install --git https://github.com/robertofabiot/moodle-mcp --locked
```

**2. Inicia sesión** en tu Moodle. La contraseña se pide en la terminal y nunca llega al modelo.

```bash
moodle-mcp --base-url https://moodle.tu-institucion.edu login
```

**3. Conecta tu cliente** con la misma URL. En Claude Code:

```bash
claude mcp add moodle -- moodle-mcp --base-url https://moodle.tu-institucion.edu serve
```

Para [Claude Desktop](guia/clientes.md#claude-desktop), [Cursor](guia/clientes.md#cursor), [Codex](guia/clientes.md#codex) y [Antigravity](guia/clientes.md#antigravity), consulta la [guía de clientes](guia/clientes.md).

## Seguridad

- **Tu contraseña no se guarda.** Se cambia por un token de Moodle, que queda en el almacén de credenciales del sistema o en un archivo con permisos `0600`.
- **El token no sale del sitio.** Solo se envía a tu Moodle y se borra de todas las respuestas antes de que lleguen al modelo.
- **Tú apruebas los cambios.** Las herramientas que modifican datos están marcadas para que tu cliente pida confirmación. Las acciones definitivas (entregar, terminar un cuestionario, publicar o enviar un mensaje) nunca se reintentan solas.
- **Moodle pone los límites.** El servidor no agrega permisos: solo puede hacer lo que tu cuenta ya puede hacer.

Los detalles de privacidad, el almacén de credenciales y las variables de entorno están en la [guía de configuración](guia/configuracion.md).

## Documentación

| Guía | Contenido |
|---|---|
| [Clientes](guia/clientes.md) | Configuración de Claude Code, Claude Desktop, Cursor, Codex, Antigravity y otros sitios Moodle. |
| [Herramientas](guia/herramientas.md) | Las 31 herramientas, sus parámetros y anotaciones. |
| [Configuración](guia/configuracion.md) | Sesión, credenciales, variables de entorno, privacidad y alcance. |
| [Desarrollo](guia/desarrollo.md) | Estructura del código, pruebas y publicación de versiones. |
| [Marca](guia/marca.md) | Logo, colores y tipografía. |

<sub>Las novedades de cada versión están en [Releases](https://github.com/robertofabiot/moodle-mcp/releases)</sub>
