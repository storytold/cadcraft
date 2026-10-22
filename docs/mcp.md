# CADCraft and AI agents (MCP)

CADCraft speaks the [Model Context Protocol](https://modelcontextprotocol.io) over stdio.

## Start it

```sh
# Headless: an in-process drawing session, no window.
cadcraft-cli mcp

# Bridged to the running app (watch the agent draw):
cadcraft --control 7979 &
cadcraft-cli mcp --connect 127.0.0.1:7979
```

An MCP client usually starts its servers when it starts, often before the app is open. With
`--lazy` the bridge starts anyway and reaches the app at the first tool call; until the app answers,
each call fails with a message saying how to open it. `--launch` also opens CADCraft (macOS) with
that control port at a call that finds nothing, and waits for it:

```json
{ "mcpServers": { "cadcraft": { "command": "cadcraft-cli", "args": ["mcp", "--connect", "127.0.0.1:7979", "--launch"] } } }
```

`--connect` needs the app's `HOST:PORT`. Without one (or with an unknown argument) `cadcraft-cli mcp`
exits with an error instead of falling back to a headless session, so a client meant for the app
never edits a separate drawing by mistake. If the app can't be reached, startup fails too.

Example client configuration (Claude Code / Claude Desktop):

```json
{ "mcpServers": { "cadcraft": { "command": "cadcraft-cli", "args": ["mcp"] } } }
```

## Tools

| Tool | What it does |
|---|---|
| `command_line {text}` | Type at the command line exactly like a person: `line 0,0 @10,0 @0,5 c`, `circle 5,2 1`, `offset 0.5`, `zoom e`. Spaces act as Enter; empty text is Enter. Returns the new prompt, keywords and output. |
| `script {text}` | A multi-line script (like `.scr` files). |
| `execute {command, params}` | Any command with JSON parameters, never a dialog. `list_commands` shows ids and parameter docs. |
| `list_commands {filter?}` | The command catalog; `filter` matches part of a command's id, label or an alias. |
| `inspect_drawing {entities?, limit?}` | Counts, extents, insertion units (`insunits`), layers, styles, blocks, layouts, selection and undo history. Entities only with `entities: true` (up to `limit`, default 200); `query_entities` filters and pages them. |
| `query_entities {type?, layer?, selected?, window?, crossing?, limit?, offset?}` | Filtered entities with handles, geometry and `bbox` (see [Coordinates, units and angles](#coordinates-units-and-angles)). `selected: true` restricts results to the current selection in the active space; an empty selection returns no entities. Omitted/false queries all entities in that space. Filters combine before paging. `window` `[[x0,y0],[x1,y1]]` finds objects whose geometry is inside it or crosses it (`crossing: false`: entirely inside). `count` is every match, `returned` the page (`offset`, `limit`, default 500). |
| `execute {"command": "shapes", "params": {type?, layer?, window?, crossing?, limit?, offset?, tolerance?}}` | Facts about the closed shapes — closed polylines, circles, full ellipses, closed splines: perimeter, area, centroid, winding, the first self-intersection and coincident vertices, exact with arcs and bulges. Other matches are counted under `skipped` with the reason. |
| `render {width?, height?, fit?, window?}` | A PNG image of the drawing, or of just the area `window` `[[x0,y0],[x1,y1]]` (one sheet or room of a large drawing; 1600 px wide with the window's shape unless sized). |
| `find_text {contains?, layer?, window?, limit?, offset?}` | The readable texts — TEXT, MTEXT without formatting codes, block attributes, multileaders — with text, insertion point, height, rotation, style, layer and handle; filtered by content (any case), layer and a window around the insertion points. Read-only (the `texts` command). |
| `new_drawing {metric?}`, `open {path}`, `save {path?}`, `cancel` | Files and Escape. `save` to a `.svg`, `.png` or `.pdf` path exports: the drawing keeps its name and unsaved changes. |
| `screenshot`, `ui_inspect`, `ui_click {x, y}` | Only when connected to the app. |

Resources: `cadcraft://drawing` (inspect JSON) and `cadcraft://commands` (catalog).

Tool results are compact JSON text (no indentation). When the client and server agree on
protocol version `2025-06-18` or later, a result that is a JSON object also comes as
`structuredContent` (the same object); older protocol versions get the text only. An array result
(`list_commands`) and images (`render`) have no structured form.

Arguments are checked against each tool's input schema. An unknown tool, `arguments` that aren't an
object, a missing required argument or one of the wrong type is a JSON-RPC error `-32602` (invalid
params), so `command_line {}` never presses Enter by accident. A tool that runs and fails (a bad
point, an unknown command) returns a result with `isError: true` and the message.

### Error codes

From protocol version `2025-06-18` on, a tool error that comes from the engine also carries
`structuredContent` `{code, command, param?, message}`: `message` is the same text as the content
block, `command` the command the error concerns, and `param` the parameter it names, when the
message names one (`` `radius` is required`` gives `radius`). `param` is best effort: the first key
the message names. Errors outside the engine (no UI for
`screenshot`, a lost connection, a timeout) are text only. The codes are stable; new ones may be
added.

| Code | Meaning |
|---|---|
| `unknown_command` | No command has this name or alias. |
| `not_available` | The command exists but can't run now (its `enabled` check refused it; `list_commands` shows `enabled`). |
| `bad_params` | The parameters were refused: missing, of the wrong type, out of range, or naming something that doesn't exist (a block, a file). |
| `no_document` | No drawing is open. |
| `failed` | The command ran and failed for another reason (nothing to act on, an unknown or malformed object handle, an operation it can't do). |
| `internal` | A bug in the command: it panicked and the drawing was kept as it was. Please report it. |

## Coordinates, units and angles

- **Coordinates** are drawing units in the world coordinate system (WCS), x right, y up. Each
  entity of `query_entities` and `inspect_drawing {entities: true}` has `geometry` (the object's
  own data) and `bbox` `[[x0,y0],[x1,y1]]`, its WCS extents as drawn (block references with their
  contents; no `bbox` for an object that draws nothing, such as an empty text).
- **`extrusion`** `[x,y,z]` is only present when an object's extrusion direction is not `(0,0,1)`.
  `(0,0,-1)` means the object was mirrored in the file it came from: the DXF reader has already
  mapped its points from that object coordinate system (OCS) to WCS, so `geometry` is WCS. Any
  other direction means the object lies in a tilted plane; its 2D points (polyline vertices, block
  insertion points) may then still be in that plane's OCS, and `bbox` is where CADCraft draws it.
- **Units:** `inspect_drawing` reports `insunits` `{code, name}`, what one drawing unit is (the
  `INSUNITS` header variable: 0 Unitless, 1 Inches, 4 Millimeters, 5 Centimeters, 6 Meters, …);
  a file without the variable reads as 1 (Inches), as the UNITS dialog shows it.
- **Angles** in `geometry` are radians, counter-clockwise from +x: `Arc` `start`/`end`, `Ellipse`
  `start`/`end` (parameters), `Text`/`MText`/`Insert` `rotation`, `Text` `oblique`, `Point`
  `angle`, `Hatch` `angle`, dimension `rotation` and `textRotation`. `find_text` reports
  `rotation` in **degrees**. Command parameters given to `execute` take the units their
  `list_commands` entry names.

## A typical session

1. `command_line {"text": "rectang 0,0 100,60"}`
2. `execute {"command": "circle", "params": {"center": [10, 10], "radius": 4}}`
3. `command_line {"text": "dimlinear 0,0 100,0 50,-8"}`
4. `inspect_drawing {}` — verify counts; `query_entities {"type": "Circle"}` — the geometry.
5. `render {}` — look at it.

The end-to-end test in `crates/mcp/src/tests.rs` runs a session like this and checks the result
only through MCP.
