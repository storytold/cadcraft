# UI parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (first interaction-precision audit against AutoCAD 2027 for Mac) · **Target:** Autodesk AutoCAD 2027 for Mac (26.0)

How CADCraft's interaction compares with AutoCAD's: point entry, object snaps, tracking, dynamic
input, grips, selection, the command line, chrome and navigation. A drafter's speed in AutoCAD
comes from these, so "the command exists" isn't enough: it has to accept the same input, snap to
the same points and respond to the same keys. Work items: [gaps.md](gaps.md#uiux).

**Headline (estimated): UI/UX fidelity ≈ 40%.** The command line, point syntax, the common
running snaps, polar/ortho and the palettes are real. Object snap tracking, extension, parallel
and apparent-intersection snaps are toggles with no behaviour; dynamic input is a read-only
tooltip; FROM, M2P, TK and point filters don't exist; hot-grip options are advertised but
ignored; there are no context menus. Remaining: **≈ 110–170 Opus 5.5 hours**.

## How this was measured

- CADCraft: read from the source on 2026-10-10 (main at bd2412f): `crates/engine/src/{prompt,
  snap, select, grips, lib, units}.rs`, `cmd/*`, `crates/ui-egui/src/*`. Not run in this pass.
- AutoCAD: behaviour observed black-box in the running AutoCAD 2027 for Mac
  (`plan/autocad/01-observed-ui.md`, local only, with reference screenshots that are never
  committed), its menu tree (`xtask/data/menu-catalog.txt`) and Autodesk's documentation.
- Weights (how often a drafter hits each area): point entry 15%, object snaps 15%, tracking and
  polar 10%, dynamic input 10%, grips 10%, selection 10%, command line 10%, chrome and palettes
  10%, navigation 5%, keyboard and context menus 5%.

| Area | Weight | CADCraft | Weighted |
|---|---:|---:|---:|
| Point entry | 15% | 45% | 6.8 |
| Object snaps | 15% | 40% | 6.0 |
| Tracking, polar, ortho, grid/snap | 10% | 30% | 3.0 |
| Dynamic input | 10% | 15% | 1.5 |
| Grips | 10% | 40% | 4.0 |
| Selection | 10% | 40% | 4.0 |
| Command line | 10% | 60% | 6.0 |
| Chrome and palettes | 10% | 45% | 4.5 |
| Navigation | 5% | 55% | 2.8 |
| Keyboard shortcuts, context menus | 5% | 25% | 1.3 |
| **Total** | 100% | | **≈ 40%** |

## Point entry (≈ 45%)

| Input | AutoCAD | CADCraft | Evidence |
|---|---|---|---|
| `x,y[,z]`, `@dx,dy`, `@d<a`, `d<a`, `#x,y`, bare `@` | yes | yes; `#` equals absolute because there is no UCS; Z parsed then discarded | `prompt.rs:147-180` |
| `*x,y` (WCS override) | yes | no | |
| Direct distance entry | yes, honours ortho, polar and tracking paths | yes, ortho; polar only through the UI's snapped cursor | `lib.rs:782-793` |
| Angle forms: `45d30'`, radians, grads, surveyor `N45dE` | yes | yes | `units.rs:141-197` |
| Architectural/engineering distances `5'6-1/2"` | yes | yes | `units.rs:93-140` |
| `FROM` (base point + offset) | yes | **no** | |
| `M2P` / `MTP` (midpoint between two points) | yes | **no** | |
| `TK` (tracking) | yes | **no** | |
| Point filters `.x`, `.y`, `.xy`, `.z` | yes | **no** | |
| Angle override `<45` | yes | not found | |
| Typed calculator `'CAL` | yes | no | |

## Object snaps (≈ 40%)

| Mode | AutoCAD | CADCraft (`snap.rs:135-334`) |
|---|---|---|
| Endpoint | every curve, solid/3D vertices, dimension ends | lines, arcs, polyline segments, open ellipses and splines, ray base, solid/trace corners |
| Midpoint | every curve incl. ellipse/spline arcs, polyline segments | segments only; **none on circles, ellipses, splines** |
| Center | arcs, circles, ellipses, polyarcs | arcs, circles, ellipses |
| Geometric center | any closed polyline, spline | closed LWPOLYLINE only |
| Node | points, dimension definition points, text alignment points | POINT only |
| Quadrant | circles, arcs, ellipses | yes |
| Intersection | every pair, extended intersection | among up to 200 nearby primitives; ellipse/spline pairs approximate ([geometry-parity.md](geometry-parity.md)) |
| Extension | yes, with dashed extension path | **declared, never produces a point** |
| Insertion | text, mtext, blocks, attributes, attdefs | TEXT, MTEXT, INSERT; no ATTDEF or MLEADER |
| Perpendicular | every curve, deferred | lines, arcs, circles, xlines; deferred for the first point (#168) |
| Tangent | circles, arcs, ellipses, splines, deferred | arcs and circles; deferred for the first point; **no ellipse or spline** |
| Nearest | every curve | yes |
| Apparent intersection | projected intersection | **same as Intersection** |
| Parallel | yes, with parallel path | **declared, never produces a point** |
| Temporary overrides typed at a prompt (`END`, `MID`, `INT` …) | yes | **no**: the text arrives as plain input |
| Shift/Ctrl+right-click snap menu | yes | **no** |
| Tab to cycle candidate snaps | yes | **no** |
| AutoSnap marker, tooltip, magnet, aperture box | yes | marker per mode and name label; aperture from APERTURE; no magnet, no aperture box |
| OSNAPZ, 3D object snaps | yes | no |

## Tracking, polar, ortho, grid and snap (≈ 30%)

| Feature | AutoCAD | CADCraft |
|---|---|---|
| Ortho (F8) | yes | yes (`snap.rs:503`) |
| Polar tracking increment | yes | yes, one increment, dashed ray and "Polar: d < a°" tooltip (`snap.rs:510`, `canvas.rs:838-843`) |
| Additional polar angles, relative to last segment | yes | **no** |
| Polar distance snap (PolarSnap) | yes | **no** |
| **Object snap tracking** (acquire points, alignment paths, intersections of paths) | yes, F11 | **toggle only**: the F11 button and flag exist, nothing acquires points or draws paths |
| Grid (rectangular, major lines, adaptive, limits) | yes | rectangular, major lines, adaptive display |
| Snap (rectangular, isometric, polar snap) | yes | rectangular from 0,0 only (`snap.rs:531`) |
| Isometric drafting (ISODRAFT, isoplanes F5) | yes | **status-bar toggle with no effect** |
| Drafting Settings dialog | yes | yes (`dialogs.rs:36-80`) |

## Dynamic input (≈ 15%)

AutoCAD's heads-up input (F12) has editable pointer-input fields (x, y), dimensional input
fields (length and angle) with Tab between them and lock icons, relative-by-default second
points, dimensional input on grips, a prompt with clickable options and a down-arrow menu.
CADCraft's `dynamic_input` (`canvas.rs:900-925`) paints a read-only tooltip: the prompt and
either `distance < angle` or `x, y`, and echoes what is typed at the command line. No fields, no
Tab, no grip dimensional input, no DYNPROMPT/DYNDIM/DYNPICOORDS. PR #60 (open) adds pointer
input boxes with Tab, comma and relative entry.

## Grips (≈ 40%)

| Feature | AutoCAD | CADCraft |
|---|---|---|
| Stretch grips per object type | yes | lines (midpoint moves), arcs (start/mid/end), ellipses (axes), polylines (vertices; segment midpoints move or reshape the bulge), dimensions (text and definition points), MLEADER, viewport, wipeout, spline (`grips.rs:79-270`) |
| Text/MText grips | position, width, column grips | move the whole object |
| Hot-grip modes with Space/Enter cycling | Stretch, Move, Rotate, Scale, Mirror | yes (`canvas.rs:58-67`) |
| Hot-grip options: Base point, Copy, Undo, Reference, eXit | yes | **advertised in the prompt but not parsed**; the engine's JSON grip commands accept `copy`, the UI never passes it |
| Several hot grips at once (Shift-click) | yes | **no** (one handle) |
| Multifunctional grip hover menus (Add Vertex, Remove Vertex, Convert to Arc, Lengthen …) | yes | **no** |
| Grip dimensional input (length/angle fields) | yes | no |
| Dynamic block grips | yes | no (no dynamic blocks) |
| Grip appearance (GRIPSIZE, colours, hover) | yes | GRIPSIZE honoured |

## Selection (≈ 40%)

| Feature | AutoCAD | CADCraft |
|---|---|---|
| Pick, implied window (left→right) and crossing (right→left) | yes | yes, coloured and dashed rectangles (`lib.rs:576-589`, `canvas.rs:823-836`) |
| Keywords W, C, WP, CP, F, BOX, AU, SI, M, G (group), CL (class) | yes | **All, Last, Previous, Add, Remove only** (`lib.rs:595-599`, `machines.rs:77-80`); fence only as a JSON parameter |
| Lasso (press-drag) | yes | **no** (PR #63 open: PICKDRAG = 2) |
| Selection cycling (SELECTIONCYCLING) | yes | toggle nothing reads |
| Selection preview, hover highlight | yes | hover highlight |
| PICKADD, PICKFIRST, PICKAUTO | yes | PICKADD (Shift), PICKFIRST |
| QSELECT, SELECTSIMILAR, FILTER | yes | QSELECT dialog; no SELECTSIMILAR or FILTER |
| Object isolate/hide (ISOLATEOBJECTS, HIDEOBJECTS) | yes | **no** (only layer isolate) |
| Groups selectable as one (PICKSTYLE) | yes | no |

## Command line (≈ 60%)

| Feature | AutoCAD | CADCraft |
|---|---|---|
| AutoComplete (commands, sysvars, mid-string, synonyms) | yes | command list with Tab to accept (`cmdline.rs:30,176,317`); no sysvar or mid-string search confirmed |
| Aliases | about 250 in `acad.pgp` | 144 aliases across the commands (counted from source) |
| History (F2), Up/Down recall | yes | yes |
| Transparent commands (`'zoom`, `'pan`, `'osnap`) | yes | yes (`lib.rs:496-503`) |
| Enter/Space repeats last command | yes | yes |
| Clickable keywords in the prompt | yes | yes (`cmdline.rs:253-267`) |
| UNDO with Mark, Back, BEgin, End, Auto, Control | yes | count only, whole-document snapshots capped at 2,000 (`lib.rs:477`, `edit.rs:56`) |
| SETVAR for every system variable | ~1,000 sysvars | about 60 session/header variables by name (`sysvars.rs`) |
| Scripts (SCRIPT, `.scr`) | yes | script text via CLI/MCP/control; no `.scr` file command |
| Floating/docked command window, transparency, lines of history | yes | floating overlay, history lines setting |

## Chrome and palettes (≈ 45%)

| Feature | AutoCAD for Mac | CADCraft |
|---|---|---|
| Menu bar | 11 menus, 491 leaf items | same 11 menus; 245/491 items covered (233 by `cargo xtask parity`, which counts engine commands only, plus 12 UI-only items: see [parity-checklist.md](parity-checklist.md)) |
| Tool Bar, File tabs, Start tab, layout tabs | yes | yes |
| Tool Sets palette (Drafting, Modeling, … with flyouts) | yes | yes for 2D groups |
| Properties Inspector, Quick Properties | yes | yes (every control acts or is visibly unavailable, #182) |
| Layer Properties Manager (filters, states, VP overrides) | yes | yes |
| Blocks palette, Content | yes | Blocks window (click to insert, #28) |
| Status bar toggles | ~25 | most toggles; Annotation Visibility and Workspace do nothing |
| ViewCube | interactive 3D | clickable for 2D (Top/Home → extents); dragging does nothing (#7) |
| Navigation bar, SteeringWheels | yes | no |
| Workspaces | Drafting & Annotation, 3D Modeling | no |
| Options / Preferences dialog | many tabs | **no** (issue #23) |
| Customisation (CUI, aliases editor, tool palettes) | yes | no |
| Themes | Dark, Light | Dark, Light, System (#179) |
| Accessibility | VoiceOver basics | AccessKit through eframe; screen-reader coverage untested; magnifier issue #39 |

## Navigation (≈ 55%)

| Feature | AutoCAD | CADCraft |
|---|---|---|
| ZOOM Extents, All, Window, Previous, In/Out, Center, Scale, Object | yes | yes, 50-entry history (`cmd/view.rs:107-149`) |
| ZOOM Realtime (drag), Dynamic | yes | **no** |
| Wheel zoom about cursor, ZOOMFACTOR, ZOOMWHEEL | yes | fixed factor (no ZOOMFACTOR) |
| Middle-drag pan, double-middle-click extents | yes | yes (`canvas.rs:538-583`) |
| Trackpad two-finger pan, pinch zoom (macOS) | yes | yes (#58) |
| Named views (VIEW) | yes | **no** (VIEW table not read or written) |
| Tiled model-space viewports (VPORTS) | yes | not confirmed: VPORTS maps to paper-space MVIEW |
| 3D orbit, ViewCube drag, SteeringWheels | yes | no (M11) |

## Keyboard and context menus (≈ 25%)

- Shortcut table: 25 entries (Cmd shortcuts, F1, F2, F3, F7–F12) plus 21 command `.key()`
  shortcuts. AutoCAD for Mac's defaults are similar in number; F4 (3D osnap), F5 (isoplane), F6
  (dynamic UCS) are missing.
- Delete erases the selection on macOS (#64); Escape cancels.
- **No right-click context menus** anywhere: right-click is Enter, clear selection or repeat
  (`canvas.rs:651-660`). AutoCAD has context menus on the canvas (per selection, per command),
  the command line, palettes and tabs.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | First UI-parity audit: point entry, snaps, tracking, dynamic input, grips, selection, command line, chrome, navigation, keys |
