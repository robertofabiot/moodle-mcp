# Configuración

## Sesión

```bash
moodle-mcp login      # pide usuario y contraseña en la terminal
moodle-mcp whoami     # muestra el sitio y la cuenta conectada
moodle-mcp logout     # borra la sesión guardada
```

`login` cambia tu contraseña por un token de Moodle y guarda solo el token. La contraseña no pasa por el modelo ni queda en disco. Si el servidor ya está corriendo dentro de un cliente y el token vence, ejecuta `login` en cualquier terminal: la siguiente llamada usa la sesión nueva sin reiniciar nada.

Puedes tener sesión en varios Moodle a la vez, una por URL. Sin `--base-url` ni `MOODLE_BASE_URL`, los comandos usan la única sesión guardada; si hay varias o ninguna, usan la URL por defecto. El archivo `moodle-mcp/sessions.json` del directorio de configuración lista las URLs con sesión, sin tokens.

`logout` borra la sesión de tu equipo, pero el token sigue siendo válido en Moodle. Para revocarlo, abre **Preferencias → Claves de seguridad** en el sitio y restablece la clave del servicio móvil.

## Almacén de credenciales

El flag global `--store` elige dónde se guarda el token:

| Valor | Comportamiento |
|---|---|
| `auto` (por defecto) | Usa el almacén del sistema y, si no está disponible, un archivo con permisos `0600`. |
| `keyring` | Solo el almacén del sistema: Secret Service o KWallet en Linux, Keychain en macOS, Credential Manager en Windows. |
| `file` | Solo el archivo `moodle-mcp/credentials.json` dentro del directorio de configuración del usuario. |

Usa el mismo `--store` en `login` y en `serve`.

## Variables de entorno

| Variable | Por defecto | Uso |
|---|---|---|
| `MOODLE_BASE_URL` | — | URL de tu Moodle. Equivale a `--base-url` y tiene prioridad sobre la sesión guardada. |
| `MOODLE_TOKEN` | — | Usa este token en lugar del guardado. |
| `MOODLE_PRIVATE_TOKEN` | — | Token privado de la sesión. Se sanea en las respuestas igual que el token. |
| `MOODLE_MCP_DOWNLOAD_DIR` | `~/Downloads/moodle-mcp` | Carpeta de descargas, cursos replicados y respuestas extensas. |
| `MOODLE_MCP_MAX_RESPONSE_BYTES` | `60000` | Tamaño máximo de una respuesta. Si se supera, el contenido completo se guarda en un archivo y la respuesta indica su ruta. |
| `MOODLE_MCP_MAX_INLINE_IMAGE_BYTES` | `1000000` | Tamaño máximo de una imagen devuelta en línea. |

## Privacidad

- El token solo se envía al sitio Moodle configurado. Las URLs de otros dominios se rechazan y no se siguen redirecciones externas.
- Las respuestas y los errores pasan por un filtro que reemplaza el token por `<token>`.
- Los archivos descargados se guardan en la carpeta de descargas, con nombres saneados.
- El contenido de foros, páginas y mensajes lo escriben terceros. Las instrucciones del servidor le indican al modelo que lo trate como datos y no como órdenes.

## Alcance

Moodle decide qué puede hacer tu cuenta: moodle-mcp no agrega permisos.

| Caso | Qué hacer |
|---|---|
| Función del web service deshabilitada para tu cuenta | `moodle_open_in_browser` devuelve el enlace de la página equivalente. |
| Actividades LTI, SCORM, H5P o plugins sin API | Ábrelas con `moodle_open_in_browser` y continúa en el navegador. |
| Safe Exam Browser o exámenes supervisados | Usa el cliente que exige Moodle. `quiz_start` rechaza estos cuestionarios. |
| Google Drive, Meet y otros sitios externos | El enlace se devuelve tal cual. Ábrelo con tu sesión de ese servicio. |
| Archivos por encima del límite de entrega | `get_assignment` muestra el máximo de la tarea. |
