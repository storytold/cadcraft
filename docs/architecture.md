# CADCraft architecture

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-18 · **Change:** minor (DXF/DWG save-version documentation corrected) · **Target:** Autodesk AutoCAD 2027

How CADCraft is built today. This describes the code on `main`, not plans; plans live in
[roadmap.md](roadmap.md). Agents: the rules that keep this shape (layering, never-crash,
everything-is-a-command, clean-room) are in [AGENTS.md](../AGENTS.md).

## Workspace

About 57,000 lines of Rust in 12 library crates, 3 apps and `xtask` (counted 2026-10-10).

| Layer | Crate | Lines | What it does |
|---|---|---:|---|
| L0 | `geom` | 2,067 | f64 2D/3D vectors, `Mat3`/`Mat4`, bounds, lines, arcs and bulges, circles, ellipses, (rational) B-splines with fit-point interpolation, line/arc intersections, tessellation, planar region tracing for HATCH/BOUNDARY (`region.rs`) |
| L0 | `dxf` | 571 | Standalone DXF tag reader/writer: ASCII and binary, tag-count limits for hostile input |
| L1 | `color` | 265 | AutoCAD Color Index table, true colour, ByLayer/ByBlock |
| L1 | `doc` | 3,054 | The drawing database: header, symbol tables (layers, linetypes, text/dimension/table/multileader styles), blocks, model space, layouts, constraints; copy-on-write `EntityStore`; our own hatch pattern and linetype library (`library.rs`) |
| L2 | `fonts` | 2,062 | Our single-stroke drafting font, TrueType/OpenType/`.ttc` outlines through `skrifa`, TEXT and MTEXT layout and formatting codes, per-character fallback and FONTALT |
| L2 | `render` | 3,300 | Display lists (polylines, triangles, points tagged with entity handles): linetypes, hatches, gradients, text, block expansion, dimension geometry, paper-space clipping; CPU rasteriser (`tiny-skia`) for PNG, plot preview and tests |
| L2 | `constraints` | 2,504 | Own parametric solver: damped Gauss–Newton / Levenberg–Marquardt (`lm.rs`), 12 geometric and the dimensional constraints, expressions and parameters, AutoConstrain inference |
| L3 | `io` | 5,124 | DXF ⇄ document mapping (`dxf_read.rs`, `dxf_write.rs`, CADCraft xdata in `dxf_ext.rs`), PDF plotting, SVG and PNG export |
| L3 | `dwg` | 149 | DWG through a DXF bridge: the `acadrust` crate (MPL-2.0, unmodified) converts DWG ⇄ DXF bytes; nothing else sees its types. Native only |
| L4 | `engine` | 25,090 | `Session`, the command registry (`cmd/*`), command-line parser and prompt machines, object snaps and polar/ortho (`snap.rs`), selection (`select.rs`), grips (`grips.rs`), associativity (`assoc.rs`), system variables, units, undo, R-tree spatial index (`spatial.rs`) |
| L5 | `ui-egui` | 9,849 | The swappable egui front end: GPU canvas (`gpu.rs`, wgpu), menus and shortcuts (`menus.rs`), command line, palettes, Layer Properties Manager and other dialogs, ViewCube, icons drawn in code, themes, the interface string catalog (`i18n/`, English, Ukrainian and German), JSON control channel (`control.rs`) |
| L5 | `mcp` | 559 | MCP server (JSON-RPC 2.0 over stdio) on a headless session or bridged to a running app |
| app | `cadcraft` | 1,102 | Desktop app (eframe/wgpu; DirectX 12 by default on Windows) |
| app | `cadcraft-cli` | 823 | `info`, `convert`, `run` (scripts), `commands`, `sample`, `mcp` |
| app | `cadcraft-web` | 88 | WASM build of the same UI (no DWG) |
| tool | `xtask` | 1,064 | `ci`, `layers`, `assets`, `wasm`, `parity`, `version`, `stats`, icon generation |

`cargo xtask layers` enforces the layering: nothing below L5 depends on egui, eframe, winit,
wgpu or rfd.

## Data model

- A `Drawing` is header variables, symbol tables, block definitions, model space and paper-space
  layouts. Entities carry a handle, layer, colour, linetype, lineweight, transparency and an
  `EntityKind`: Line, Point, Circle, Arc, Ellipse, LwPolyline, Polyline3d, Spline, Ray, XLine,
  Text, MText, AttDef, Insert (with attributes), Dimension (linear, aligned, angular, diameter,
  radius, ordinate, arc length), Leader, MLeader, Hatch (pattern, solid, gradient), Solid,
  Trace, Face3d, Viewport, Image, Wipeout, Table and Unknown (raw DXF tags).
- There are no 3D solids, surfaces, meshes, regions (ACIS), multilines, tolerance frames or
  underlays in the model yet ([gaps.md](gaps.md)).
- Entity collections are copy-on-write, so undo is a whole-drawing snapshot that costs little.
- Coordinates are f64 world units. 2D work happens in the WCS; there is no UCS yet.

## Commands

Every user-visible action is a `CommandSpec` (`crates/engine/src/cmd/mod.rs`): id = AutoCAD's
command name in lower case, label, menu path, shortcut, aliases, a JSON `run`, an `enabled`
check and an optional interactive prompt machine. 295 engine commands are registered (128 with
interactive prompts, counted from source 2026-10-10), plus about 30 UI-only commands in
`crates/ui-egui/src/menus.rs`. The command line, menus, toolbar, Tool Sets, scripts, CLI, control
channel and MCP all reach the same commands. JSON calls never open dialogs.

## Rendering

`render::build` turns a space into a display list once per change; the UI uploads it to wgpu in
batches (`gpu.rs`). Tessellation tolerance follows the view. The R-tree (`spatial.rs`) is patched
per changed store chunk after an edit, so picking stays near 0.001 ms at 200k entities. PNG, PDF,
SVG and plot preview reuse the same display list through the CPU rasteriser or vector writers.

## Files

DXF is the native path: `dxf` parses tags, `io::dxf_read` maps them to the document, and
`io::dxf_write` uses R2000 DXF structure with the chosen version's `$ACADVER` (2000–2018),
version-specific groups and UTF-8 text from 2007. New drawings default to 2018 (AC1032).
In the app's Save dialog, File ▸ Default Save Format (also Options ▸ Open and Save,
`saveVersion`, default 2018) sets the version for untitled drawings; drawings with a file keep
their own version, subject to supported-version mapping and the DWG minimum.
Without an explicit save version, `io::save_version` uses the drawing's `$ACADVER`, keeping
supported versions, mapping older files to 2000 and newer files to 2018 (missing or invalid
codes also default to 2018). DWG is converted to DXF in memory by `acadrust` and read through
the same path; saving DWG converts an internal R2000 DXF to the chosen DWG version, not
necessarily R2000. DWG defaults to at least 2004 to retain true colours, transparency, table
styles and gradients; an explicit save version can still request 2000. CADCraft-only data
(constraints, associativity details, table flags) travels in xdata under our own application
name. Detail and gaps: [file-format-parity.md](file-format-parity.md).

## Agent control

- **Control channel:** `cadcraft --control PORT` accepts JSON lines (`cmdline.input`,
  `engine.execute`, `ui.screenshot`, `ui.render` …); see [control-protocol.md](control-protocol.md).
- **MCP:** `cadcraft-cli mcp` (headless) or `--connect HOST:PORT` (drive the running app); see
  [mcp.md](mcp.md).
- **CLI:** `cadcraft-cli run --script … --export out.png` for headless batch work. A reader that
  closes stdout early (`… | head`) drops the rest of the output but keeps the work — later
  commands, `--save`, `--export` — and the exit status still reports failures; another stdout
  write error is reported and makes it 1.

## Safety

Production crates deny `unwrap`/`expect`/`panic!` and forbid `unsafe`. `Session::execute` and
interactive input run under `catch_unwind`, restoring the drawing if a panic escapes. Input-sized
allocations are capped (DXF tag counts, DWG size limits, hatch segment and island limits,
`MAX_BLOCK_DEPTH`).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-18 | minor | Clarify 2018 defaults, opened-drawing version selection, the DWG 2004 minimum unless explicitly overridden, and R2000 DXF structure versus output version |
| 2026-10-10 | major | First committed architecture doc, measured from the code (crate sizes, entity kinds, command counts) |
