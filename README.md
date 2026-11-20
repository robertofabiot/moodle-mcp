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

# Verificar la sesión activa
moodle-mcp whoami
```

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

---

## Variables de Entorno

Puedes personalizar el comportamiento de `moodle-mcp` mediante variables de entorno:

- `MOODLE_BASE_URL`: URL base de la instancia de Moodle (por defecto `https://uamvirtual.uam.edu.ni/grado`).
- `MOODLE_TOKEN`: Token de autenticación directo (omite la lectura del keyring o archivo).
- `MOODLE_PRIVATE_TOKEN`: Token privado complementario para descarga de archivos autorizados.
- `MOODLE_MCP_DOWNLOAD_DIR`: Directorio base para descargas y respuestas truncadas (por defecto `~/Downloads/moodle-mcp`).
- `MOODLE_MCP_MAX_RESPONSE_BYTES`: Tamaño máximo en bytes de una respuesta antes de truncarla y guardarla en archivo (por defecto `60000`).
