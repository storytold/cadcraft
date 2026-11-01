# UI parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-27 · **Change:** minor (WIPEOUT closure-aware grips) · **Target:** Autodesk AutoCAD 2027 for Mac (26.0)

How CADCraft's interaction compares with AutoCAD's: point entry, object snaps, tracking, dynamic
input, grips, selection, the command line, chrome and navigation. A drafter's speed in AutoCAD
comes from these, so "the command exists" isn't enough: it has to accept the same input, snap to
the same points and respond to the same keys. Work items: [gaps.md](gaps.md#uiux).

**Headline (estimated): UI/UX fidelity ≈ 48%.** The command line, point syntax, the common
running snaps, polar/ortho, object snap tracking and the palettes are real, and so are the
Extension, Parallel and Apparent Intersection snaps (#379); dynamic input is a read-only
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
| Object snaps | 15% | 45% | 6.8 |
| Tracking, polar, ortho, grid/snap | 10% | 70% | 7.0 |
| Dynamic input | 10% | 45% | 4.5 |
| Grips | 10% | 40% | 4.0 |
| Selection | 10% | 40% | 4.0 |
| Command line | 10% | 60% | 6.0 |
| Chrome and palettes | 10% | 45% | 4.5 |
| Navigation | 5% | 55% | 2.8 |
| Keyboard shortcuts, context menus | 5% | 25% | 1.3 |
| **Total** | 100% | | **≈ 48%** |

## Point entry (≈ 45%)

| Input | AutoCAD | CADCraft | Evidence |
|---|---|---|---|
| `x,y[,z]`, `@dx,dy`, `@d<a`, `d<a`, `#x,y`, bare `@` | yes | yes; `#` equals absolute because there is no UCS; Z parsed then discarded | `prompt.rs:147-180` |
| `*x,y` (WCS override) | yes | yes (`*x,y`, `*@dx,dy`, `@*dx,dy`); the same as the plain forms because there is no UCS | `prompt.rs` |
| Direct distance entry | yes, honours ortho, polar and tracking paths | yes, ortho; polar only through the UI's snapped cursor | `lib.rs:782-793` |
| Angle forms: `45d30'`, radians, grads, surveyor `N45dE` | yes | yes | `units.rs:141-197` |
| Architectural/engineering distances `5'6-1/2"` | yes | yes | `units.rs:93-140` |
| `FROM` (base point + offset) | yes | yes: "Base point:", "<Offset>:" with `@` measured from the base; nests with M2P and filters | `pointmod.rs` |
| `M2P` / `MTP` (midpoint between two points) | yes | yes | `pointmod.rs` |
| `TK` (tracking) | yes | **no** | |
| Point filters `.x`, `.y`, `.xy`, `.z` | yes | yes (`.x` … `.yz`, "(need YZ):"); Z is asked for and dropped (no 3D) | `pointmod.rs` |
| Angle override `<45` | yes | yes: `<a`, `<<a`, `<<<a`; a typed distance or a pick follows the angle, typed coordinates win | `pointmod.rs` |
| Object snap overrides (`END`, `MID`, `INT` … `NON`) | yes | yes, every mode and `NON`, with long names and `_`/`'` prefixes, for the next pick only; a typed point is snapped within the aperture; a command's own keyword wins (`_cen` always snaps); also from the snap menu | `pointmod.rs` |
| Typed calculator `'CAL` | yes | no | |

## Object snaps (≈ 45%)

| Mode | AutoCAD | CADCraft (`snap.rs:135-334`) |
|---|---|---|
| Endpoint | every curve, solid/3D vertices, dimension ends | lines, arcs, polyline segments, open ellipses and splines, ray base, solid/trace corners |
| Midpoint | every curve incl. ellipse/spline arcs, polyline segments | segments only; **none on circles, ellipses, splines** |
| Center | arcs, circles, ellipses, polyarcs | arcs, circles, ellipses |
| Geometric center | any closed polyline, spline | closed LWPOLYLINE only |
| Node | points, dimension definition points, text alignment points | POINT only |
| Quadrant | circles, arcs, ellipses | yes |
| Intersection | every pair, extended intersection | among up to 200 nearby primitives; ellipse/spline pairs approximate ([geometry-parity.md](geometry-parity.md)) |
| Extension | yes, with dashed extension path | yes: rest on a line or arc end, then the line runs on (a ray) and the arc round its circle; dotted path and "Extension: d < a°" (`snap/tracking.rs`); not from ellipse, spline or polyline-arc ends |
| Insertion | text, mtext, blocks, attributes, attdefs | TEXT, MTEXT, INSERT; no ATTDEF or MLEADER |
| Perpendicular | every curve, deferred | lines, arcs, circles, xlines; deferred for the first point (#168) |
| Tangent | circles, arcs, ellipses, splines, deferred | arcs and circles; deferred for the first point; **no ellipse or spline** |
| Nearest | every curve | yes |
| Apparent intersection | projected intersection | yes in 2D: where objects near the cursor would meet if extended; an object rested on sends its whole extension, so its crossing with another object snaps too (`snap.rs` `osnap_with`, `snap/tracking.rs`) |
| Parallel | yes, with parallel path | yes: rest on a line, then a path parallel to it runs through the base point (`snap/tracking.rs`) |
| Temporary overrides typed at a prompt (`END`, `MID`, `INT` …) | yes | **no**: the text arrives as plain input |
| Shift/Ctrl+right-click snap menu | yes | yes: From, Mid Between 2 Points, Point Filters, the snap modes, None, Osnap Settings; no Temporary track point yet (`context_menu.rs`) |
| Tab to cycle candidate snaps | yes | yes, through the closest snap of each running mode under the cursor (`pointmod::snap_candidates`) |
| AutoSnap marker, tooltip, magnet, aperture box | yes | marker per mode and name label; aperture from APERTURE; no magnet, no aperture box |
| OSNAPZ, 3D object snaps | yes | no |

## Tracking, polar, ortho, grid and snap (≈ 70%)

| Feature | AutoCAD | CADCraft |
|---|---|---|
| Ortho (F8) | yes, Shift overrides it while held | yes; holding Shift turns it on or off while held (`Session::snap_cursor`, `snap/tracking.rs`) |
| Polar tracking increment | yes | yes, measured from ANGBASE; dotted path and "Polar: d < a°" tooltip (`snap/tracking.rs`, `canvas.rs` `draw_tracking`) |
| Additional polar angles, relative to last segment | yes | additional angles yes (POLARADDANG with POLARMODE 4); **relative to the last segment no** |
| Polar distance snap (PolarSnap) | yes | **no** |
| **Object snap tracking** (acquire points, alignment paths, intersections of paths) | yes, F11 | yes (#379): resting on a snap point acquires it (`+`), resting again releases it, seven at most; horizontal/vertical paths, or every polar angle with POLARMODE 2; Shift to acquire with POLARMODE 8; crossings of two paths, a path and an object, and a path and a polar path snap; tooltip names each path; a typed distance (direct distance entry) runs along the path the cursor is on, measured from the point the path comes from. Engine: `Session::snap_cursor`, `snap/tracking.rs`; AUTOSNAP bits 8 and 16 |
| Grid (rectangular, major lines, adaptive, limits) | yes | rectangular and isometric (30°, 90° and 150° lines), major lines, adaptive display |
| Snap (rectangular, isometric, polar snap) | yes | rectangular and isometric (SNAPSTYL 1: the snap Y spacing along the isometric axes), from SNAPBASE turned by SNAPANG |
| Isometric drafting (ISODRAFT, isoplanes F5) | yes | yes (#556): ISODRAFT and the status-bar toggle set SNAPSTYL and the plane; ISOPLANE / F5 / Ctrl+E cycle Left, Top, Right (SNAPISOPAIR); ortho, direct distance entry and the crosshair follow the plane's axes; ELLIPSE Isocircle (radius or Diameter, also for elliptical arcs); Drafting Settings snap style radio. Engine: `snap/iso.rs`, `cmd/settings/iso.rs` |
| Drafting Settings dialog | yes | yes (`dialogs.rs:36-80`) |
| Hatch and Gradient / Hatch Edit dialogs | yes | yes (`hatchdlg.rs`): pattern swatches, gradient colours and shapes, origin, pick points or objects, associative, separate, island detection; Hatch Edit adds boundaries from points or closed objects and removes selected loops; opened by HATCH, GRADIENT (HPDLGMODE), HATCHEDIT, right-click or double-clicking a hatch; no Preview or Inherit Properties |

## Dynamic input (≈ 45%)

AutoCAD's heads-up input (F12) has editable pointer-input fields (x, y), dimensional input
fields (length and angle) with Tab between them and lock icons, relative-by-default second
points, dimensional input on grips, a prompt with clickable options and a down-arrow menu.
CADCraft has pointer input boxes since #60 (`crates/ui-egui/src/dyninput.rs`): value boxes
beside the cursor while a command asks for a point; second and later points default to polar
distance and angle (DYNPIFORMAT = 1 for Cartesian); `,` and `<` lock a value and move to the next
box; Tab types the separator; Enter sends a relative entry (DYNPICOORDS = 1 for absolute) and
fills an empty box from the cursor; a plain number stays direct distance entry. Missing:
dimensional input on grips (length/angle fields while stretching), lock icons, the prompt with a
down-arrow options menu (DYNPROMPT), and DYNDIM.

## Grips (≈ 40%)

| Feature | AutoCAD | CADCraft |
|---|---|---|
| Stretch grips per object type | yes | lines (midpoint moves), arcs (start/mid/end), ellipses (axes), polylines (vertices; segment midpoints move or reshape the bulge), dimensions (text and definition points), MLEADER, viewport, wipeout (one grip per logical corner, including coincident corners; an explicit closing marker follows the first vertex), spline (`grips.rs`) |
| Text/MText grips | position, width, column grips | move the whole object |
| Hot-grip modes with Space/Enter cycling | Stretch, Move, Rotate, Scale, Mirror | yes (`canvas.rs:58-67`) |
| Hot-grip options: Base point, Copy, Undo, Reference, eXit | yes | **advertised in the prompt but not parsed**; the engine's JSON grip commands accept `copy`, the UI never passes it |
| Several hot grips at once (Shift-click) | yes | **no** (one handle) |
| Multifunctional grip hover menus (Add Vertex, Remove Vertex, Convert to Arc, Lengthen …) | yes | **no** |
| Grip dimensional input (length/angle fields) | yes | no |
| Dynamic block grips | yes | no (no dynamic blocks) |
| Grip appearance (GRIPSIZE, colours, hover) | yes | GRIPSIZE, GRIPS and GRIPOBJLIMIT honoured (Options ▸ Selection); polyline segment midpoints use rectangles, vertices use squares; no grip colours |

## Selection (≈ 40%)

| Feature | AutoCAD | CADCraft |
|---|---|---|
| Pick, implied window (left→right) and crossing (right→left) | yes | yes, coloured and dashed rectangles (`lib.rs:576-589`, `canvas.rs:823-836`), filled as SELECTIONAREA, WINDOWAREACOLOR, CROSSINGAREACOLOR and SELECTIONAREAOPACITY say (Visual Effect Settings) |
| Keywords W, C, WP, CP, F, BOX, AU, SI, M, G (group), CL (class) | yes | yes except CLass: W, C, F, WP, CP (#226); BOX, AUto, SIngle, Multiple, Group, Add, Remove, Undo (#428); ALL, Last, Previous |
| Press-drag selection windows (PICKDRAG) | yes | clicks or drag (2, default), or two clicks only (0); Options ▸ Selection ▸ Select using two clicks only, saved in the profile; drag-only mode (1) is unavailable |
| Lasso | yes | **no** |
| Selection cycling (SELECTIONCYCLING) | yes | yes (#1143): SELECTIONCYCLING 2 lists the objects under a click (type, layer; the row under the pointer highlights its object), 1 the badge; Shift+Space steps the highlight through them (#1145); no Ctrl+W |
| Selection preview, hover highlight | yes | selected and hovered objects redrawn in the selection colour, fills half see-through (SELECTIONEFFECT 1; 0 dashed), selection effect color; HIGHLIGHT (not saved), SELECTIONPREVIEW, PREVIEWEFFECT, PREVIEWFILTER (Visual Effect Settings) |
| PICKADD, PICKFIRST, PICKAUTO | yes | PICKADD (Shift), PICKFIRST |
| Stability of outlined fill highlights | yes | closed contours keep the same start, direction and ordering on repaints and GPU rebuilds; dashed filled glyphs, hatch/solid regions, arrowheads and wipeout outlines no longer shift with pointer movement |
| QSELECT, SELECTSIMILAR, FILTER | yes | QSELECT dialog; SELECTSIMILAR with SELECTSIMILARMODE (#428, SEttings asks for the bit code; no dialog); no FILTER |
| Object isolate/hide (ISOLATEOBJECTS, HIDEOBJECTS) | yes | yes, temporary (OBJECTISOLATIONMODE 0; #428): Tools > Isolate, UNISOLATEOBJECTS; no status-bar button |
| Groups selectable as one (PICKSTYLE) | yes | yes (PICKSTYLE 1, #417), Object grouping on Options ▸ Selection; associative hatch selection (bit 2) is stored, not used |

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
| Menu bar | 11 menus, 491 leaf items | same 11 menus; 245/491 items covered (233 by `cargo xtask parity`, which counts engine commands only, plus 12 UI-only items: see [parity-checklist.md](parity-checklist.md)); once a menu is open, pointing at another title opens that menu (#1567) |
| Tool Bar, File tabs, Start tab, layout tabs | yes | yes |
| Tool Sets palette (Drafting, Modeling, … with flyouts) | yes | yes for 2D groups |
| Properties Inspector, Quick Properties | yes | yes (every control acts or is visibly unavailable, #182); block references show read-only Position X/Y/Z from their stored insertion point; coordinate editing is still missing |
| Layer Properties Manager (filters, states, VP overrides) | yes | yes |
| Blocks palette, Content | yes | Blocks window (click to insert, #28) |
| Status bar toggles | ~25 | most toggles; Annotation Visibility and Workspace do nothing |
| ViewCube | interactive 3D | clickable for 2D (Top/Home → extents); dragging does nothing (#7) |
| Navigation bar, SteeringWheels | yes | no |
| Workspaces | Drafting & Annotation, 3D Modeling | no |
| Browser panel layout storage | not applicable | Versioned, validated panel placement and visibility saved through the browser host; floating palettes remain embedded. Requires available browser storage and a completed host save; named workspaces and drawing persistence are separate. |
| Options / Preferences dialog | many tabs | **no** (issue #23) |
| Customisation (CUI, aliases editor, tool palettes) | yes | Command Aliases dialog (ALIASEDIT, #1381); no CUI or tool palettes |
| Themes | Dark, Light | Dark, Light, System (#179) |
| Accessibility | VoiceOver basics | AccessKit through eframe; screen-reader coverage untested; magnifier issue #39; animation effects follow the OS setting (View ▸ Accessibility ▸ Animation Effects: System, On, Off) |

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

- Shortcut table: 28 entries (Cmd shortcuts, F1, F2, F3, F5, F7–F12) plus 23 command `.key()`
  shortcuts. AutoCAD for Mac's defaults are similar in number; F4 (3D osnap) and F6 (dynamic UCS)
  are missing.
- Delete erases the selection on macOS (#64); Escape cancels.
- Right-click on the canvas is Enter while a command runs; holding it (250 ms, AutoCAD's
  time-sensitive right-click) opens the command menu: Enter, Cancel, Recent Input, the prompt's
  options, Snap Overrides, Pan/Zoom. Otherwise it opens a shortcut menu (Repeat, Recent Commands,
  Clipboard ▸, Isolate ▸, the selection's editing commands, Draw Order ▸, Group, the selected
  object type's edit commands, Select Similar, Deselect All, Undo/Redo, Pan/Zoom; #325, #512).
  The command line, drawing tabs, layout tabs, Layers palette rows and the Properties palette
  have menus too (#512). Still missing: in-place layout tab rename, Move or Copy, Select All
  Layouts.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-27 | minor | WIPEOUT grips treat implicit and explicit closure alike; moving the first vertex carries the closing marker, and moving endpoints together keeps both corners editable. Open/closed polyline flags and coincident polyline vertices remain independent (#2256) |
| 2026-10-26 | minor | Browser host stores bounded, validated panel layout separately from drawing state; malformed or unsupported records retain defaults. Native workspace storage is unchanged. |
| 2026-10-22 | minor | Polyline segment midpoint grips use rectangles, distinct from square vertex grips, with matching hover and pick bounds (#1950) |
| 2026-10-13 | minor | Filled-region highlight boundaries are traversed in deterministic coordinate order, fixing dash-phase jumps on CPU hover/selection redraws and GPU highlight rebuilds; CPU dash-position, pixel and GPU mesh regressions |
| 2026-10-14 | minor | Selection cycling (#1143): the list and badge of SELECTIONCYCLING, Shift+Space through overlapping objects (#1145) |
| 2026-10-14 | minor | Hatch Edit dialog adds boundaries from picked points or closed objects and removes selected loops |
| 2026-10-14 | minor | Holding Shift overrides ortho while it is held (on when ORTHOMODE is off, off when it is on), as in AutoCAD |
| 2026-10-16 | minor | ALIASEDIT opens the Command Aliases dialog (#1381) |
| 2026-10-18 | minor | In-window menu bar: once a menu is open, pointing at another title closes it and opens that one (#1567) |
| 2026-10-13 | minor | Options ▸ Selection adds Select using two clicks only; PICKDRAG 0 disables drag-window creation/completion, 2 preserves clicks or drag, and the preference survives restart; UI pointer and profile regressions |
| 2026-10-12 | minor | Selected and hovered objects redrawn in the selection colour (text and arrowheads included); Options ▸ Selection laid out as AutoCAD's: Object grouping, Show grips, grip object limit, selection preview, Visual Effect Settings (selection area, preview filter) |
| 2026-10-12 | minor | Animation effects (dialog and menu fades, smoothed wheel scrolling and ⌘/Ctrl+wheel zoom) follow the operating system's setting; View ▸ Accessibility ▸ Animation Effects chooses System, On or Off |
| 2026-10-12 | minor | Hatch and Gradient / Hatch Edit dialogs (HATCH, GRADIENT, HATCHEDIT, double-click a hatch; HPDLGMODE) |
| 2026-10-12 | minor | Block references now show their stored insertion X/Y/Z in the Properties Inspector, including nonzero Z; fields are read-only |
| 2026-10-11 | minor | Isometric drafting (#556): isometric snap and grid, ISODRAFT, ISOPLANE (F5), ortho along isoplane axes, ELLIPSE Isocircle |
| 2026-10-11 | minor | Object snap tracking, Extension, Parallel, Apparent Intersection, additional polar angles (#379): tracking 30% → 70%, object snaps 40% → 45%; total 43% → 48% |
| 2026-10-10 | minor | Dynamic input 15% → 45% (pointer boxes, #60); press-drag windows (#63); total 40% → 43% |
| 2026-10-10 | major | First UI-parity audit: point entry, snaps, tracking, dynamic input, grips, selection, command line, chrome, navigation, keys |
