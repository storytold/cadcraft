# CADCraft control protocol

Start the desktop app with `--control PORT` (or `CADCRAFT_CONTROL_PORT=PORT`). It listens on
`127.0.0.1:PORT` for JSON lines: one request per line, one reply per line.

```json
{"id": 1, "method": "cmdline.input", "params": {"text": "circle 0,0 5"}}
{"id": 1, "ok": true, "result": {"prompt": "Command:", "output": ["Command: CIRCLE"], ...}}
```

Errors come back as `{"id": …, "ok": false, "error": "message"}`.

| Method | Params | What it does |
|---|---|---|
| `cmdline.input` | `{text}` | Type a line at the command line exactly like a user. Starts commands by name/alias and answers prompts (points `x,y`, `@dx,dy`, `@d<a`, distances, keywords, empty = Enter). Spaces act as Enter except at text prompts. |
| `cmdline.script` | `{text}` | Run a multi-line script (like `.scr` files). |
| `cmdline.key` | `{key: "enter"\|"escape"}` | Press Enter or Escape. |
| `cmdline.state` | | Current prompt, keywords, accepted input kinds, history tail. |
| `engine.execute` | `{command, params}` | Run any command with JSON parameters. Never opens a dialog. |
| `engine.commands` | | Every command: id, label, menu path, shortcut, aliases, params doc, enabled. |
| `drawing.inspect` | `{entities?, limit?}` | Drawing summary (counts, extents, layers, styles, blocks, layouts, selection, undo) and entities with handles and geometry. |
| `ui.inspect` | | UI state, canvas rect, view, cursor and snap, performance counters; `closePrompt`: the drawing a "Save changes?" prompt is asking about (null when none). |
| `ui.menu.list` / `ui.menu.invoke` | `{command}` | The menu tree; invoke an item like a click (interactive). |
| `ui.pointer` | `{x, y, space?: "world"\|"screen", button?, action?: "click"\|"move"}` | Click in the drawing area (world coordinates by default). Right button = Enter. |
| `ui.click` / `ui.move` | `{x, y, button?, shift?}` | Real egui pointer events in screen points (reach every widget). |
| `ui.key` / `ui.text` | `{key, cmd?, shift?, alt?}` / `{text}` | Synthetic keyboard input. |
| `ui.set` | UiState fields | Show/hide palettes, toolbars, command line, ViewCube… |
| `ui.resize` | `{width, height}` | Resize the window. |
| `ui.screenshot` | `{path?}` | PNG of the window (needs a presented frame). |
| `ui.render` | `{path?, width?, height?}` | Headless render of the current view of the drawing. |
| `app.open` / `app.save` / `app.quit` | `{path}` | File operations. |

The MCP server (`cadcraft-cli mcp --connect 127.0.0.1:PORT`) wraps this protocol; headless
(`cadcraft-cli mcp`) it serves the same methods from an in-process session.
