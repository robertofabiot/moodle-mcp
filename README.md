# moodle-mcp

Servidor MCP (Model Context Protocol) por transporte `stdio` y CLI para Moodle (diseñado para la plataforma UAM Virtual). Permite a modelos de lenguaje (LLMs) y entornos de desarrollo interactuar de forma segura con la API del web service de Moodle utilizando la cuenta del estudiante.

## Características

- **Transporte estándar MCP**: Comunicación nativa JSON-RPC 2.0 a través de `stdio` compatible con la especificación MCP.
- **Herramientas genéricas seguras**:
  - `moodle_whoami`: Consulta información del usuario y del sitio conectado.
  - `moodle_list_functions`: Descubre funciones disponibles y habilitadas en el servicio web.
  - `moodle_describe_function`: Describe el estado y modo de solo lectura de una función.
  - `moodle_read`: Ejecuta funciones de consulta y lectura segura (rechaza funciones de mutación).
  - `moodle_call`: Ejecuta cualquier función habilitada; aplica exactamente 1 intento sin reintento para mutaciones y reintentos automáticos para lecturas.
- **Seguridad y privacidad**:
  - Sanitización automática (`scrub`) de tokens y credenciales en todas las salidas.
  - Almacén de credenciales seguro en el keyring del sistema operativo con fallback a archivo local protegido con permisos `0600`.
  - Truncado automático de respuestas masivas a disco si superan el límite configurado (`max_response_bytes`), protegiendo la ventana de contexto del LLM.

## Instalación y Autenticación Previa

Antes de usar el servidor MCP con cualquier cliente, inicia sesión en tu terminal para almacenar las credenciales de forma segura:

```bash
# Compilar e instalar binario
cargo install --path .

# Iniciar sesión e intercambiar credenciales por un token del web service
moodle-mcp login

# O especificando el tipo de almacén (auto por defecto, keyring o file)
moodle-mcp --store auto login

# Verificar la sesión activa
moodle-mcp whoami
```

> **Recarga dinámica de sesión:** Si el servidor MCP ya está en ejecución (por ejemplo, lanzado por Claude Code, Cursor o Claude Desktop) y no había sesión activa o el token expiró, puedes ejecutar `moodle-mcp login` en cualquier terminal. El servidor detectará y recargará automáticamente la nueva sesión en la siguiente invocación de herramienta sin necesidad de reiniciar el servidor MCP ni el cliente.

### Opciones de Almacenamiento (`--store`)

El flag global `--store` permite seleccionar el almacén de credenciales:
- `auto` (por defecto): Intenta almacenar y leer del keyring del sistema operativo; si no está disponible o falla, utiliza automáticamente un archivo local (`credentials.json`) con permisos restrictivos `0600`.
- `keyring`: Utiliza exclusivamente el almacén seguro del sistema operativo (Secret Service / KWallet en Linux, Keychain en macOS, Credential Manager en Windows).
- `file`: Utiliza exclusivamente el archivo local `credentials.json` protegido con permisos `0600`.

---

## Configuración en Clientes MCP

### 1. Claude Code CLI

Para agregar el servidor MCP a Claude Code de forma persistente:

```bash
claude mcp add moodle -- moodle-mcp serve
```

O especificando una URL base diferente si es necesario:

```bash
claude mcp add moodle -- moodle-mcp --base-url https://uamvirtual.uam.edu.ni/grado serve
```

### 2. Cursor

Agrega la configuración en `.cursor/mcp.json` o en los ajustes globales de Cursor (`Settings > Features > MCP`):

```json
{
  "mcpServers": {
    "moodle": {
      "command": "moodle-mcp",
      "args": ["serve"]
    }
  }
}
```

### 3. Claude Desktop

En tu archivo de configuración de Claude Desktop:
- **macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`
- **Windows**: `%APPDATA%\Claude\claude_desktop_config.json`
- **Linux**: `~/.config/Claude/claude_desktop_config.json`

```json
{
  "mcpServers": {
    "moodle": {
      "command": "moodle-mcp",
      "args": ["serve"]
    }
  }
}
```

---

## Herramientas Genéricas Disponibles

| Herramienta | Parámetros | Modo / Anotaciones | Descripción |
|---|---|---|---|
| `moodle_whoami` | Ninguno | `read_only_hint = true` | Devuelve información del sitio Moodle y del usuario conectado, omitiendo el listado masivo de funciones y añadiendo `function_count`. |
| `moodle_list_functions` | `filter?: string` | `read_only_hint = true` | Lista funciones habilitadas para el usuario, filtrando por subcadena (case-insensitive) e indicando si son de lectura (`read_only`). |
| `moodle_describe_function` | `name: string` | `read_only_hint = true` | Describe si una función está habilitada, si es de lectura y sugiere cómo inspeccionar parámetros requeridos. |
| `moodle_read` | `function: string`, `params?: object` | `read_only_hint = true`, `open_world_hint = true` | Invoca una función del web service garantizando que sea de solo lectura. Rechaza funciones de mutación con error `invalid_arguments`. |
| `moodle_call` | `function: string`, `params?: object` | `destructive_hint = true`, `open_world_hint = true` | Invoca cualquier función habilitada del web service (lectura o mutación). Utiliza un cliente con 0 reintentos si la función muta datos. |
| `moodle_download` | `url: string`, `dest?: string`, `inline?: bool` | `read_only_hint = false`, `destructive_hint = false`, `idempotent_hint = true` | Descarga cualquier archivo de Moodle (recursos, tareas, adjuntos) a disco local. Si el archivo es texto o HTML menor al límite, incluye su representación en Markdown; con `inline = true`, aplica el visor de contenidos directamente sobre el archivo descargado. |
| `moodle_view` | `path?: string`, `url?: string` | `read_only_hint = false`, `destructive_hint = false`, `idempotent_hint = true` | Visualiza un archivo local (`path`) o remoto de Moodle (`url`), extrayendo texto/Markdown de PDF, DOCX, PPTX, HTML y texto plano (guardando a `<archivo>.txt` si desborda el límite) o devolviendo imágenes raster (PNG, JPEG, GIF, WebP) en línea dentro del límite configurado. |
| `moodle_upload` | `paths: string[]`, `itemid?: int` | `destructive_hint = false` | Sube uno o más archivos locales al área de borrador (`draft`) de Moodle vía `upload.php`, devolviendo el `itemid` para entregas o foros. |
| `moodle_sync_course` | `courseid: int`, `dest?: string` | `read_only_hint = false`, `destructive_hint = false`, `idempotent_hint = true` | Revisa y replica incrementalmente todos los contenidos y archivos de un curso a disco local, manteniendo un manifiesto `.moodle-sync.json` y listando enlaces externos en `enlaces.md`. |
| `list_courses` | `include_past?: bool` | `read_only_hint = true` | Lista los cursos matriculados del estudiante, filtrando activos por defecto o incluyendo pasados con `include_past = true`. |
| `get_course` | `courseid: int` | `read_only_hint = true` | Devuelve el temario y árbol completo de secciones y módulos de un curso con resúmenes en Markdown y archivos asociados. |
| `course_updates_since` | `courseid: int`, `since: int` | `read_only_hint = true` | Consulta las actualizaciones o cambios producidos en los módulos de un curso desde un timestamp Unix epoch. |
| `get_grades` | `courseid?: int` | `read_only_hint = true` | Obtiene el resumen de notas globales entre cursos o el desglose detallado de actividades evaluables de un curso específico. |
| `upcoming_events` | `days?: int` | `read_only_hint = true` | Consulta las próximas fechas límites de entrega y eventos del calendario del estudiante (por defecto 14 días). |
| `get_assignment` | `cmid?: int`, `assignid?: int` | `read_only_hint = true` | Consulta los detalles de una tarea: enunciado en Markdown, archivos de consigna, configuración de entrega, fechas límites y estado/calificación del alumno. |
| `submit_assignment` | `cmid?: int`, `assignid?: int`, `files?: string[]`, `onlinetext?: string`, `onlinetext_format?: string`, `keep_existing_files?: bool`, `final?: bool` | `destructive_hint = true` | Guarda o envía una entrega de tarea con archivos locales y/o texto en línea, con opción de conservar archivos previos y envío definitivo a calificación. |
| `quiz_info` | `cmid?: int`, `quizid?: int` | `read_only_hint = true` | Consulta la información detallada de un cuestionario, incluyendo reglas de acceso, restricciones de contraseña o SEB, fechas y el historial de intentos realizados. |
| `quiz_start` | `cmid?: int`, `quizid?: int`, `password?: string`, `forcenew?: bool` | `destructive_hint = true` | Inicia un nuevo intento de resolución o retoma uno en curso, soportando contraseña y verificando restricciones como Safe Exam Browser. |
| `quiz_get_page` | `attemptid: int`, `page: int` | `read_only_hint = true` | Lee una página de un intento de cuestionario, analizando las preguntas y guardando automáticamente sus campos ocultos en memoria. |
| `quiz_save` | `attemptid: int`, `answers: object` | `destructive_hint = false` | Guarda respuestas intermedias en un intento de cuestionario inyectando automáticamente los campos ocultos y tokens de secuencia correspondientes. |
| `quiz_finish` | `attemptid: int` | `destructive_hint = true` | Envía definitivamente un intento de cuestionario para su calificación, transmitiendo los campos ocultos y obteniendo la revisión final. |
| `quiz_review` | `attemptid: int`, `page?: int` | `read_only_hint = true` | Obtiene la revisión de un intento terminado, devolviendo la nota final y las preguntas analizadas con sus opciones, estado y retroalimentación en Markdown. |
| `forum_list` | `courseid: int` | `read_only_hint = true` | Lista los foros de un curso con su tipo, descripción en Markdown, cantidad de discusiones y fechas configuradas. |
| `forum_list_discussions` | `cmid?: int`, `forumid?: int`, `page?: int`, `perpage?: int` | `read_only_hint = true` | Lista las discusiones de un foro aceptando `cmid` o `forumid`, con resúmenes en Markdown, autor, fechas y paginación. |
| `forum_get_discussion` | `discussionid: int` | `read_only_hint = true` | Obtiene todos los mensajes de una discusión de foro en orden cronológico con su contenido en Markdown y archivos adjuntos. |
| `forum_reply` | `postid: int`, `message: string`, `subject?: string`, `attachments?: string[]` | `destructive_hint = true` | Publica una respuesta en Markdown a un mensaje de foro, resolviendo automáticamente el asunto `"Re: <padre>"` si se omite y adjuntando archivos locales opcionales. |
| `forum_new_discussion` | `cmid?: int`, `forumid?: int`, `subject: string`, `message: string`, `groupid?: int`, `attachments?: string[]` | `destructive_hint = true` | Abre un nuevo tema de discusión en un foro aceptando `cmid` o `forumid`, asunto, mensaje en Markdown, grupo opcional y archivos adjuntos. |
| `list_conversations` | `limit?: int` | `read_only_hint = true` | Lista las conversaciones de mensajería privada del estudiante autenticado con participantes y el último mensaje convertido a Markdown. |
| `get_conversation` | `conversationid: int`, `limit?: int` | `read_only_hint = true` | Obtiene los mensajes más recientes y los miembros de una conversación específica convirtiendo el contenido HTML a Markdown. |
| `send_message` | `conversationid?: int`, `touserid?: int`, `text: string` | `destructive_hint = true` | Envía un mensaje privado en Markdown a una conversación existente (`conversationid`) o a un usuario destinatario (`touserid`) de forma mutuamente excluyente. |

---

## Variables de Entorno

Puedes personalizar el comportamiento de `moodle-mcp` mediante variables de entorno:

- `MOODLE_BASE_URL`: URL base de la instancia de Moodle (por defecto `https://uamvirtual.uam.edu.ni/grado`).
- `MOODLE_TOKEN`: Token de autenticación directo (omite la lectura del keyring o archivo).
- `MOODLE_PRIVATE_TOKEN`: Token privado (`privatetoken`) devuelto en el inicio de sesión de Moodle, utilizado exclusivamente para generar claves de inicio de sesión automático en el navegador.
- `MOODLE_MCP_DOWNLOAD_DIR`: Directorio base para descargas y respuestas truncadas (por defecto `~/Downloads/moodle-mcp`).
- `MOODLE_MCP_MAX_RESPONSE_BYTES`: Tamaño máximo en bytes de una respuesta antes de truncarla y guardarla en archivo (por defecto `60000`).
- `MOODLE_MCP_MAX_INLINE_IMAGE_BYTES`: Tamaño máximo en bytes para incrustar imágenes raster (PNG, JPEG, GIF, WebP) en línea dentro de la respuesta MCP (por defecto `1000000`).

---

## Trabajo Futuro

- **Recursos MCP (`moodle://…`)**: la exposición de cursos, temarios y archivos mediante URIs de recursos MCP (`moodle://…`) queda fuera del alcance de la versión `0.1.0` y se deja planteada como trabajo futuro; en esta versión toda la inspección y visualización de archivos se realiza mediante herramientas (`moodle_view`, `moodle_download`, `moodle_sync_course`).
