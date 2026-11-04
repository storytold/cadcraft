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
| `inspect_drawing {entities?, limit?}` | Counts, extents, layers, styles, blocks, layouts, selection, undo history and entities. |
| `query_entities {type?, layer?, window?, crossing?, limit?, offset?}` | Filtered entities with handles and geometry. `window` `[[x0,y0],[x1,y1]]` finds objects whose geometry is inside it or crosses it (`crossing: false`: entirely inside). `count` is every match, `returned` the page (`offset`, `limit`, default 500). |
| `execute {"command": "shapes", "params": {type?, layer?, window?, crossing?, limit?, offset?, tolerance?}}` | Facts about the closed shapes — closed polylines, circles, full ellipses, closed splines: perimeter, area, centroid, winding, the first self-intersection and coincident vertices, exact with arcs and bulges. Other matches are counted under `skipped` with the reason. `selfCheck` says what was found, analysed and skipped, the caps hit with their values (`limit`, `crossingSegments`) and implausible values (perimeter past 10× the bounding-box diagonal, area larger than the drawing extents, each past a 1e-9 relative tolerance for rounding; with no visible objects there are no extents, so no area bound applies). |
| `execute {"command": "shapes", "params": {type?, layer?, window?, crossing?, limit?, offset?, tolerance?}}` | Facts about the closed shapes — closed polylines, circles, full ellipses, closed splines: perimeter, area, centroid, winding, the first self-intersection and coincident vertices, exact with arcs and bulges. Other matches are counted under `skipped` with the reason. |
| `execute {"command": "usage", "params": {type?, limit?, offset?}}` | Read-only cross-reference of the layers, blocks, text styles and linetypes: for each, the objects in model space and layouts that use it, the objects in block definitions that use it, whether it is used only inside block definitions, and whether it is empty or unused. `summary` counts each kind and `unused` lists what PURGE would offer, in table order. One scan reads at most 5,000,000 objects; a larger drawing sets `incomplete: true` with an `incompleteReason`, because counts, statuses and summaries then omit the objects past the limit. |
| `execute {"command": "loops", "params": {tolerance?, maxPairs?, limit?, offset?}}` | Implied closed loops from loose LINE and ARC chains: each closed loop with its member handles, perimeter, area and winding, exact with arcs; each open chain with the endpoint gap, its location (`at`, `ends`) and length; and branch junctions (three or more ends meeting), reported but not resolved. `tolerance` defaults to 1e-9 × the extents diagonal; the quadratic endpoint pairing stops at `maxPairs` (default 1,000,000) and reports `capped` with `pairTestsNeeded` instead of guessing from a partial pairing. |
| `query_entities {type?, layer?, selected?, window?, crossing?, limit?, offset?}` | Filtered entities with handles and geometry. `selected: true` restricts results to the current selection in the active space; an empty selection returns no entities. Omitted/false queries all entities in that space. Filters combine before paging. `window` `[[x0,y0],[x1,y1]]` finds objects whose geometry is inside it or crosses it (`crossing: false`: entirely inside). `count` is every match, `returned` the page (`offset`, `limit`, default 500). |
| `render {width?, height?, fit?, window?}` | A PNG image of the drawing, or of just the area `window` `[[x0,y0],[x1,y1]]` (one sheet or room of a large drawing; 1600 px wide with the window's shape unless sized). |
| `find_text {contains?, layer?, window?, limit?, offset?}` | The readable texts — TEXT, MTEXT without formatting codes, block attributes, multileaders — with text, insertion point, height, rotation, style, layer and handle; filtered by content (any case), layer and a window around the insertion points. Read-only (the `texts` command). |
| `new_drawing {metric?}`, `open {path}`, `save {path?}`, `cancel` | Files and Escape. `save` to a `.svg`, `.png` or `.pdf` path exports: the drawing keeps its name and unsaved changes. |
| `screenshot`, `ui_inspect`, `ui_click {x, y}` | Only when connected to the app. |

Resources: `cadcraft://drawing` (inspect JSON) and `cadcraft://commands` (catalog).

Arguments are checked against each tool's input schema. An unknown tool, `arguments` that aren't an
object, a missing required argument or one of the wrong type is a JSON-RPC error `-32602` (invalid
params), so `command_line {}` never presses Enter by accident. A tool that runs and fails (a bad
point, an unknown command) returns a result with `isError: true` and the message.

## A typical session

1. `command_line {"text": "rectang 0,0 100,60"}`
2. `execute {"command": "circle", "params": {"center": [10, 10], "radius": 4}}`
3. `command_line {"text": "dimlinear 0,0 100,0 50,-8"}`
4. `inspect_drawing {}` — verify counts and geometry.
5. `render {}` — look at it.

The end-to-end test in `crates/mcp/src/tests.rs` runs a session like this and checks the result
only through MCP.
