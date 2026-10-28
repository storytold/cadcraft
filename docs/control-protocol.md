# CADCraft control protocol

Start the desktop app with `--control PORT` (or `CADCRAFT_CONTROL_PORT=PORT`). It listens on
`127.0.0.1:PORT` for JSON lines: one request per line, one reply per line. A port given with
`--control` wins over the variable; a bare `--control` keeps the variable's port.

```json
{"id": 1, "method": "cmdline.input", "params": {"text": "circle 0,0 5"}, "token": "<session token>"}
{"id": 1, "ok": true, "result": {"prompt": "Command:", "output": ["Command: CIRCLE"], ...}}
```

Every request must carry the session `token`. Because loopback TCP cannot tell local operating
system users apart, the app writes an unguessable per-session token to an owner-only file when the
control server starts, and refuses (`unauthorized`) any request without a matching token. The file
is `cadcraft-control-<port>.token` in the per-user runtime directory (`$XDG_RUNTIME_DIR`, else
`~/.cache`; `%LOCALAPPDATA%` on Windows); its path is logged at startup. A client reads the token
from that file — which requires the desktop user's own filesystem access — and includes it on each
line. The MCP bridge (`cadcraft-cli mcp --connect`) does this automatically.

Errors come back as `{"id": …, "ok": false, "error": "message"}`. An unknown method's error points
to `control.methods`, which lists the methods the server answers.

A line that is not a JSON object with a string `method` (an HTTP request, text that isn't JSON,
bytes that aren't UTF-8) gets one error reply and the server closes the connection; nothing sent
after it runs. Blank lines are skipped.

The server bounds what a client can hold: 16 connections at once (more get an error line and are
closed), 256 MiB per request line (longer closes the connection), 64 queued requests (more are
answered `{"ok": false, "error": "busy: …", "state": "not-run"}`), and 10 minutes idle or 30 seconds
per blocked reply write before the socket is closed.

The server waits up to 60 s for the app to answer a request. After that it replies with a timeout, and
`state` tells whether sending the request again is safe:

- `{"id": …, "ok": false, "error": "timeout", "state": "not-run"}`: the app had not started the request.
  It is cancelled and will never run, so sending it again is safe.
- `{"id": …, "ok": false, "error": "timeout", "state": "may-have-run"}`: the app had already started it.
  It may still finish and change the drawing, and its late result is discarded. Inspect the drawing
  (`drawing.inspect`) before sending it again.

The table below describes top-level control-protocol methods: names for the request's `method`
field. `engine.commands` returns a separate command catalog, not a list of additional methods.

| Method | Params | What it does |
|---|---|---|
| `cmdline.input` | `{text}` | Type a line at the command line exactly like a user. Starts commands by name/alias and answers prompts (points `x,y`, `@dx,dy`, `@d<a`, distances, keywords, empty = Enter). Spaces act as Enter except at text prompts. |
| `cmdline.script` | `{text}` | Run a multi-line script (like `.scr` files). |
| `cmdline.key` | `{key: "enter"\|"escape"}` | Press Enter or Escape. Replies like `cmdline.input`: the state, `output`, and `error` when Enter was refused. |
| `cmdline.state` | | Current prompt, keywords, accepted input kinds, history tail. |
| `engine.execute` | `{command, params}` | Run any command with JSON parameters. Never opens a dialog. `command` is an alias of this method, with the same parameters and result. |
| `engine.commands` | | Every command: id, label, menu path, shortcut, aliases, params doc, enabled. |
| `drawing.inspect` | `{entities?, limit?}` | Drawing summary (counts, extents, layers, styles, blocks, layouts, selection, undo) and entities with handles and geometry. |
| `ui.inspect` | | UI state, canvas rect, view, cursor and snap, performance counters (`uiFrames`, `passIndex` included); `closePrompt`: the drawing a "Save changes?" prompt is asking about (null when none); and actual egui keyboard focus (`focus.id`, `focus.label`, `focus.role`). |
| `ui.elements` | `{label?, role?, focused?}` | Observed toolbar, tab, drafting and layer-palette controls from the latest UI frame. Filters match exactly. Each entry has its real egui `id`, label, role, rectangle `[x,y,width,height]`, enabled/visible/selected state and current focus. |
| `ui.menu.list` / `ui.menu.invoke` | `{command}` | The menu tree; invoke an item like a click (interactive). |
| `ui.pointer` | `{x, y, space?: "world"\|"screen", button?, action?: "click"\|"move"}` | Click in the drawing area (world coordinates by default). Right button = Enter. |
| `ui.click` / `ui.move` | `{x, y, button?, shift?, ctrl?}` | Real egui pointer events in screen points (reach every widget). |
| `ui.drag` | `{x, y, to: [x, y], button?}` | Real press, move and release (drag the command line's top edge, the ViewCube ring…). |
| `ui.key` / `ui.text` | `{key, cmd?, shift?, alt?}` / `{text}` | Synthetic keyboard input. |
| `ui.set` | UiState fields | Show/hide palettes, toolbars, command line, ViewCube…; `theme: "system"\|"light"\|"dark"` (also `engine.execute` with `ui.theme {theme}`); `ui.inspect` reports the theme shown (`theme`: light/dark). `animations: "system"\|"on"\|"off"` (also `ui.animations {animations}`) shows or hides animation effects (dialog and menu fades, smoothed scrolling), `system` following the operating system's setting; `ui.inspect` reports whether they show (`animations`: true/false). `saveFormat: "dxf"\|"dwg"` (also `ui.saveformat {format}`) is the format Save As suggests for new drawings; `saveVersion: "2000"`…`"2018"` (also `ui.saveversion {version}`) the version it saves them in (SAVEAS also takes `version` and `binary` directly). |
| `ui.resize` | `{width, height}` | Resize the window. |
| `ui.focus` | | Activate the native window. Does not assign focus to a widget. |
| `ui.screenshot` | `{path?}` | PNG of the window (needs a presented frame). Bounded: at most 4 queued/in-flight/completed requests, 64 MiB per image and 64 MiB total (completed but undrained images still count), an 8 s deadline, and cancellation when the document or keyboard focus changes while the request is outstanding. A queued request adopts the settled window size when issued. After issue, a change in window size, scale, OS window focus, minimization or occlusion cancels it. Every reply carries `request`; failures include `ok:false,error`. Files are written only after the deadline and identity checks pass. Without `path`, each request gets a unique PNG in a private owner-only per-process directory under the temp dir (created exclusively; an existing symlink is never followed), so concurrent app instances cannot collide; the caller owns those files and should remove them when done. |
| `ui.render` | `{path?, width?, height?, fit?}` | Headless render. `fit` (default true) frames the whole drawing; `false` uses the current view. Replies `{pngBase64, width, height, bytes, path?}`. |
| `app.open` / `app.save` / `app.quit` | `{path}` | File operations. |
| `control.methods` | | Every method this server answers: `{method, params, description}`. Headless (`cadcraft-cli mcp`) lists only the methods it serves. |
| `ping` | | Replies `"pong"`. A cheap health check. |

To run a command listed by `engine.commands`, put its id in `params.command` and its arguments
in the nested `params.params` object, using `engine.execute` as the top-level method. This
includes `open` and `document.bytes`, and the UI commands in the desktop app's catalog.
`drawing.inspect` is also an engine command, but has an explicit top-level method; a dotted
command id alone does not make it a protocol method.

For example, inspect the drawing and list commands directly, then open a drawing and get its
DXF bytes through `engine.execute` (one request per line):

```json
{"id":2,"method":"drawing.inspect","params":{}}
{"id":3,"method":"engine.commands","params":{}}
{"id":4,"method":"engine.execute","params":{"command":"open","params":{"path":"examples/apartment.dxf"}}}
{"id":5,"method":"engine.execute","params":{"command":"document.bytes","params":{}}}
```

The `open` example assumes the app was started from the repository root; otherwise use a path
to an existing drawing accessible to the app. `document.bytes` defaults to DXF and returns
base64-encoded bytes in the result's `data` field. The `command` alias can be used identically:

```json
{"id":6,"method":"command","params":{"command":"document.bytes","params":{}}}
```

Sending `open` or `document.bytes` as the top-level `method` returns an `unknown method` error:
the protocol dispatches methods explicitly and does not fall through to the command registry.
`app.open` is a top-level convenience method for opening a drawing with `{path}`.

To read selected objects as structured geometry in a single query, execute `entities` with
`selected: true` (a boolean; omitted/false keeps the unfiltered selection behavior):

```json
{"id":7,"method":"engine.execute","params":{"command":"entities","params":{"selected":true,"limit":1000}}}
```

Only selected entities in the active space are returned; an empty selection returns an empty
page. The selection filter combines with `type`, `layer` and `window` before `offset`/`limit`.
The existing response is unchanged: `count` is all matches, `returned` is the page size and
`entities` contains handles, layers and geometry. This query does not change selection, drawing
contents or undo history. `entities` is a command, not a top-level protocol method.

The MCP server (`cadcraft-cli mcp --connect 127.0.0.1:PORT`) wraps this protocol; headless
(`cadcraft-cli mcp`) it serves the same methods from an in-process session.
MCP tool names are another interface: `execute` maps to `engine.execute`, `list_commands` to
`engine.commands`, and the MCP `open` tool to `app.open`. See [mcp.md](mcp.md).

`ui.key` queues real egui key events; its reply acknowledges the queue. Poll `ui.inspect` or
`ui.elements` for the resulting focus or state before sending the next action. Use Tab and
Shift-Tab to traverse controls and Enter/Space to activate them. The observed registry is not
the operating system's accessibility tree and does not establish physical input or screen-reader
acceptance. Unregistered controls still report their actual focus ID, with a null label/role.

The native host restores layout choices from `workspace.json` and saves changes atomically.
It stores palette/toolset visibility, selected toolset, collapsed groups, command-line history
height and related chrome choices. It does not store drawings, command buffers, dialogs or the
current Start tab. The default directory is `~/Library/Application Support/CADCraft` on macOS,
`%APPDATA%/CADCraft` on Windows, and `$XDG_CONFIG_HOME/cadcraft` or `~/.config/cadcraft` elsewhere.
Set `CADCRAFT_CONFIG_DIR` to a private directory for an isolated profile; an explicitly empty
value disables workspace reads and writes. An invalid, oversized or newer-version file is
reported and left untouched, with persistence disabled for that launch. Repair or move that
file before restarting to resume saving workspace choices.

## Panel docking

Tool Sets, Layers, and Properties can be regrouped, split, floated within the app window, closed, and reopened. The drawing canvas stays visible and cannot be closed or turned into a tab. The document/session view is protected. Layouts, active tabs, panel visibility, and floating rectangles are saved with the app's existing UI preferences. These operations do not edit document contents.

Panel IDs: `toolSets`, `layers`, `properties`, `externalReferences`. The protected center is `canvas`.

`ui.dock` accepts `operation`; basic per-panel operations also require `panel`. The advanced `resizeSplit` operation below uses a split `path` instead of a panel. Basic operations are `open`, `close`, `activate`, `float` (with finite positive `rect: [x, y, width, height]`), or `move` (with `target` and `zone: center|tab|left|right|top|bottom|above|below`, default `center`). Center and tab group with the target; side zones split beside it; above and below add a group in a floating window. `open` and `activate` show a hidden panel at its last place and select it. `ui.dock.reset` restores the default arrangement. Invalid moves return an error without changing the layout.

The QuickCalc palette (`quickCalc`) is not part of the default arrangement. Typed `QUICKCALC`/`QC`
and `ui.panel.quickcalc` explicitly open and activate it: it floats the first time and later
reopens at its saved location. **Window > QuickCalc** and **Ctrl/Cmd+8** invoke
`ui.toggle.quickcalc` instead, closing the panel when present and reopening it otherwise.
The shortcut also works with text-field focus. Toggling preserves the running drawing command
and pending command-line input. The expression, history and calculator variables survive
closing/reopening within the same session; they are not saved across application restarts.

Invoke these UI commands through `engine.execute`, for example `{"command":"ui.dock","params":{"operation":"float","panel":"layers","rect":[100,100,320,440]}}`.

Advanced `ui.dock` operations use the same validated transaction as pointer gestures:
`moveFloating` updates `rect`, `resizeSplit` accepts a boolean child `path` and
`size` (`{"Ratio":0.5}`, `{"FixedFirst":280}`, or `{"FixedSecond":280}`),
`setStackOpen` takes `open`, and `resizeStack` takes a point `height` or null.
A center or tab `move` can include `before` (panel ID or null to append) for tab order.
The equivalent `{"action": ...}` form accepts a serialized shared docking action;
this is also the form emitted by the renderer, and it takes every action of craft-ui's docking, group, column and window moves included. Protected-canvas and app-specific panel
rules apply equally to both forms.

## Command-line replies

The app and the headless session reply with the same fields.

`cmdline.state`, `cmdline.key` and `cmdline.input` return the command-line state:

| Field | Value |
|---|---|
| `prompt` | The prompt text (`"Command:"` when idle). |
| `running` | The id of the running command, or `null`. |
| `keywords` | The current prompt's options. |
| `accept` | The kinds of input the current prompt accepts, or `null` when idle. |
| `buffer` | Text typed in the command line but not yet submitted (always `""` headless). |
| `history` | The last 20 history lines. |
| `historyExpanded` | Whether the history is expanded (always `false` headless). |

`cmdline.input` adds:

- `output`: the history lines the input added. This includes the error message when the line was refused.
- `error`: the error message, only when the line was refused (unknown command, invalid input…).

A refused line still replies `"ok": true`, because the text was typed. Check `error`.

`app.open` without a string `path` fails with `"missing path"` before any command runs.

## Interface language

Select a language with `engine.execute` and the UI command `ui.language`:

```json
{"id":1,"method":"engine.execute","params":{"command":"ui.language","params":{"lang":"uk"}}}
```

Supported values are `auto`, `en`, `uk`, `pt-BR`, `de`, `es`, `cs`, `ru`, `fr`, `zh`, `it`, `ko`, `sv`, `ar` and `hu`.
Chinese needs an installed CJK font, Arabic an installed Arabic font; Korean needs a font covering the Korean interface from the
optional craft-fonts input or the operating system. Without a suitable font, the saved choice is
retained and the result's `shown` is `en`. `ui.set` also accepts
`{"interfaceLanguage":"uk"}`; `ui.inspect` reports the saved choice in its UI state.
Invalid choices leave the current language unchanged. Menus, command labels and prompt text
returned by the control channel retain their English source text. See [localization.md](localization.md).
