# Herramientas

moodle-mcp expone 31 herramientas. Las de alto nivel cubren los flujos frecuentes. Para el resto, `moodle_read` y `moodle_call` dan acceso a cualquier función del web service habilitada para tu cuenta.

Cada herramienta declara anotaciones MCP. Tu cliente puede aprobar automáticamente las de solo lectura (`read_only_hint`) y pedirte confirmación para las que modifican datos (`destructive_hint`).

## Cursos, notas y calendario

| Herramienta | Parámetros | Anotaciones | Descripción |
|---|---|---|---|
| `list_courses` | `include_past?: bool` | `read_only_hint = true` | Lista los cursos matriculados del estudiante, filtrando activos por defecto o incluyendo pasados con `include_past = true`. |
| `get_course` | `courseid: int` | `read_only_hint = true` | Devuelve el temario y árbol completo de secciones y módulos de un curso con resúmenes en Markdown y archivos asociados. |
| `course_updates_since` | `courseid: int`, `since: int` | `read_only_hint = true` | Consulta las actualizaciones o cambios producidos en los módulos de un curso desde un timestamp Unix epoch. |
| `get_grades` | `courseid?: int` | `read_only_hint = true` | Obtiene el resumen de notas globales entre cursos o el desglose detallado de actividades evaluables de un curso específico. |
| `upcoming_events` | `days?: int` | `read_only_hint = true` | Consulta las próximas fechas límites de entrega y eventos del calendario del estudiante (por defecto 14 días). |

## Tareas

| Herramienta | Parámetros | Anotaciones | Descripción |
|---|---|---|---|
| `get_assignment` | `cmid?: int`, `assignid?: int` | `read_only_hint = true` | Consulta los detalles de una tarea: enunciado en Markdown, archivos de consigna, configuración de entrega, fechas límites y estado/calificación del alumno. |
| `submit_assignment` | `cmid?: int`, `assignid?: int`, `files?: string[]`, `onlinetext?: string`, `onlinetext_format?: string`, `keep_existing_files?: bool`, `final?: bool` | `destructive_hint = true` | Guarda o envía una entrega de tarea con archivos locales y/o texto en línea, con opción de conservar archivos previos y envío definitivo a calificación. |

## Cuestionarios

| Herramienta | Parámetros | Anotaciones | Descripción |
|---|---|---|---|
| `quiz_info` | `cmid?: int`, `quizid?: int` | `read_only_hint = true` | Consulta la información detallada de un cuestionario, incluyendo reglas de acceso, restricciones de contraseña o SEB, fechas y el historial de intentos realizados. |
| `quiz_start` | `cmid?: int`, `quizid?: int`, `password?: string`, `forcenew?: bool` | `destructive_hint = true` | Inicia un nuevo intento de resolución o retoma uno en curso, soportando contraseña y verificando restricciones como Safe Exam Browser. |
| `quiz_get_page` | `attemptid: int`, `page: int` | `read_only_hint = true` | Lee una página de un intento de cuestionario, analizando las preguntas y guardando automáticamente sus campos ocultos en memoria. |
| `quiz_save` | `attemptid: int`, `answers: object` | `destructive_hint = false` | Guarda respuestas intermedias en un intento de cuestionario inyectando automáticamente los campos ocultos y tokens de secuencia correspondientes. Requiere al menos una respuesta. |
| `quiz_finish` | `attemptid: int` | `destructive_hint = true` | Envía definitivamente un intento de cuestionario para su calificación, reenviando las últimas respuestas guardadas con `quiz_save` en la sesión, y devuelve la revisión final. `finished` solo es `true` si Moodle confirma el cierre. |
| `quiz_review` | `attemptid: int`, `page?: int` | `read_only_hint = true` | Obtiene la revisión de un intento terminado, devolviendo la nota final y las preguntas analizadas con sus opciones, estado y retroalimentación en Markdown. |

## Foros

| Herramienta | Parámetros | Anotaciones | Descripción |
|---|---|---|---|
| `forum_list` | `courseid: int` | `read_only_hint = true` | Lista los foros de un curso con su tipo, descripción en Markdown, cantidad de discusiones y fechas configuradas. |
| `forum_list_discussions` | `cmid?: int`, `forumid?: int`, `page?: int`, `perpage?: int` | `read_only_hint = true` | Lista las discusiones de un foro aceptando `cmid` o `forumid`, con resúmenes en Markdown, autor, fechas y paginación. |
| `forum_get_discussion` | `discussionid: int` | `read_only_hint = true` | Obtiene todos los mensajes de una discusión de foro en orden cronológico con su contenido en Markdown y archivos adjuntos. |
| `forum_reply` | `postid: int`, `message: string`, `subject?: string`, `attachments?: string[]` | `destructive_hint = true` | Publica una respuesta en Markdown a un mensaje de foro, resolviendo automáticamente el asunto `"Re: <padre>"` si se omite y adjuntando archivos locales opcionales. |
| `forum_new_discussion` | `cmid?: int`, `forumid?: int`, `subject: string`, `message: string`, `groupid?: int`, `attachments?: string[]` | `destructive_hint = true` | Abre un nuevo tema de discusión en un foro aceptando `cmid` o `forumid`, asunto, mensaje en Markdown, grupo opcional y archivos adjuntos. |

## Mensajes privados

| Herramienta | Parámetros | Anotaciones | Descripción |
|---|---|---|---|
| `list_conversations` | `limit?: int` | `read_only_hint = true` | Lista las conversaciones de mensajería privada del estudiante autenticado con participantes y el último mensaje convertido a Markdown. |
| `get_conversation` | `conversationid: int`, `limit?: int` | `read_only_hint = true` | Obtiene los mensajes más recientes y los miembros de una conversación específica convirtiendo el contenido HTML a Markdown. |
| `send_message` | `conversationid?: int`, `touserid?: int`, `text: string` | `destructive_hint = true` | Envía un mensaje privado en Markdown a una conversación existente (`conversationid`) o a un usuario destinatario (`touserid`) de forma mutuamente excluyente. |

## Archivos

| Herramienta | Parámetros | Anotaciones | Descripción |
|---|---|---|---|
| `moodle_download` | `url: string`, `dest?: string`, `inline?: bool` | `read_only_hint = false`, `destructive_hint = false`, `idempotent_hint = true` | Descarga cualquier archivo de Moodle (recursos, tareas, adjuntos) a disco local. Si el archivo es texto o HTML menor al límite, incluye su representación en Markdown; con `inline = true`, aplica el visor de contenidos directamente sobre el archivo descargado. |
| `moodle_view` | `path?: string`, `url?: string` | `read_only_hint = false`, `destructive_hint = false`, `idempotent_hint = true` | Visualiza un archivo local (`path`) o remoto de Moodle (`url`), extrayendo texto/Markdown de PDF, DOCX, PPTX, HTML y texto plano (guardando a `<archivo>.txt` si desborda el límite) o devolviendo imágenes raster (PNG, JPEG, GIF, WebP) en línea dentro del límite configurado. |
| `moodle_upload` | `paths: string[]`, `itemid?: int` | `destructive_hint = false` | Sube uno o más archivos locales al área de borrador (`draft`) de Moodle vía `upload.php`, devolviendo el `itemid` para entregas o foros. |
| `moodle_sync_course` | `courseid: int`, `dest?: string` | `read_only_hint = false`, `destructive_hint = false`, `idempotent_hint = true` | Revisa y replica incrementalmente todos los contenidos y archivos de un curso a disco local, manteniendo un manifiesto `.moodle-sync.json` y listando enlaces externos en `enlaces.md`. |
| `moodle_open_in_browser` | `url?: string`, `cmid?: int` | `read_only_hint = true` | Devuelve un enlace del sitio Moodle para abrir una actividad con la sesión habitual del navegador. Usa una URL del sitio o un `cmid` positivo. |

## API de Moodle

| Herramienta | Parámetros | Anotaciones | Descripción |
|---|---|---|---|
| `moodle_whoami` | Ninguno | `read_only_hint = true` | Devuelve información del sitio Moodle y del usuario conectado, omitiendo el listado masivo de funciones y añadiendo `function_count`. |
| `moodle_list_functions` | `filter?: string` | `read_only_hint = true` | Lista funciones habilitadas para el usuario, filtrando por subcadena (case-insensitive) e indicando si son de lectura (`read_only`). |
| `moodle_describe_function` | `name: string` | `read_only_hint = true` | Describe si una función está habilitada, si es de lectura y sugiere cómo inspeccionar parámetros requeridos. |
| `moodle_read` | `function: string`, `params?: object` | `read_only_hint = true`, `open_world_hint = true` | Invoca una función del web service garantizando que sea de solo lectura. Rechaza funciones de mutación con error `invalid_arguments`. |
| `moodle_call` | `function: string`, `params?: object` | `destructive_hint = true`, `open_world_hint = true` | Invoca cualquier función habilitada del web service (lectura o mutación). Utiliza un cliente con 0 reintentos si la función muta datos. |

## Acciones definitivas

`submit_assignment` con `final: true`, `quiz_finish`, `forum_reply`, `forum_new_discussion` y `send_message` no se pueden deshacer. Las instrucciones del servidor le piden al modelo que te confirme cada una antes de ejecutarla. Las escrituras no se reintentan ante errores de red para no duplicar publicaciones.
