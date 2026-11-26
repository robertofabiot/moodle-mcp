# Conectar un cliente MCP

moodle-mcp funciona con cualquier cliente MCP que lance servidores por `stdio`. En todos los casos el comando es el mismo:

```text
moodle-mcp serve
```

Los ejemplos lo muestran así para abreviar: si te conectas a más de un Moodle, agrega la [URL de tu Moodle](#url-de-tu-moodle) antes de `serve`. Antes de conectarlo, inicia sesión una vez con `moodle-mcp login`. El servidor lee la sesión guardada y, si la renuevas con otro `login`, la recarga en la siguiente llamada sin reiniciar el cliente.

Se probó de punta a punta, incluido un cuestionario completo, con Claude Code, Codex y Antigravity.

- [Claude Code](#claude-code)
- [Claude Desktop](#claude-desktop)
- [Cursor](#cursor)
- [Codex](#codex)
- [Antigravity](#antigravity)
- [URL de tu Moodle](#url-de-tu-moodle)
- [Si el cliente no encuentra el comando](#si-el-cliente-no-encuentra-el-comando)

## Claude Code

```bash
claude mcp add moodle -- moodle-mcp serve
```

Agrega `--scope user` para tenerlo en todos tus proyectos.

## Claude Desktop

Edita `claude_desktop_config.json` y reinicia la aplicación:

| Sistema | Ubicación |
|---|---|
| macOS | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| Windows | `%APPDATA%\Claude\claude_desktop_config.json` |
| Linux | `~/.config/Claude/claude_desktop_config.json` |

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

## Cursor

Usa el mismo bloque `mcpServers` en `.cursor/mcp.json` (por proyecto) o en `~/.cursor/mcp.json` (global).

## Codex

```bash
codex mcp add moodle -- moodle-mcp serve
```

O directamente en `~/.codex/config.toml`:

```toml
[mcp_servers.moodle]
command = "moodle-mcp"
args = ["serve"]
```

## Antigravity

**CLI (`agy`):**

```bash
agy mcp add moodle moodle-mcp serve
agy mcp list
```

El comando guarda el servidor en `~/.gemini/config/mcp_config.json`.

**Editor:** en el panel del agente, abre el menú **⋯ → MCP Servers → Manage MCP Servers → View raw config** y agrega el bloque:

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

Guarda el archivo y actualiza la lista de servidores.

## URL de tu Moodle

Indica la URL de tu Moodle con `--base-url` al iniciar sesión:

```bash
moodle-mcp --base-url https://moodle.example.edu login
```

Si solo tienes sesión en un Moodle, `serve`, `whoami` y `logout` la usan sin `--base-url`. Si tienes sesión en varios, pasa la misma URL a `serve` o defínela una sola vez en la variable `MOODLE_BASE_URL`:

```bash
claude mcp add moodle -- moodle-mcp --base-url https://moodle.example.edu serve
```

La URL debe coincidir exactamente con la de `login`, incluida la ruta: `https://moodle.example.edu/grado` y `https://moodle.example.edu` son sesiones distintas. Si no coincide, el servidor responde con la URL que buscó y las que tienen sesión guardada.

En los archivos JSON, agrega `"--base-url", "https://moodle.example.edu"` antes de `"serve"` en `args`. El sitio debe tener habilitados los servicios web móviles. Las demás opciones están en la [guía de configuración](configuracion.md).

## Si el cliente no encuentra el comando

Las aplicaciones de escritorio no siempre heredan el `PATH` de tu terminal (pasa sobre todo en macOS). Si el servidor no arranca, usa la ruta absoluta del binario en `command`:

```bash
which moodle-mcp        # macOS y Linux
where moodle-mcp        # Windows
```
