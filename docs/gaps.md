# Where CADCraft falls short of AutoCAD

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (first gaps list, from a full audit of main at bd2412f against AutoCAD 2027) · **Target:** Autodesk AutoCAD 2027

Every known shortfall, one row each, ranked within each section by user impact. This is the work
list: agents pick from here (top of a section first), and remove or update a row in the same PR
that closes it. Numbers and weights live in [target-app-parity.md](target-app-parity.md); hours
are Opus 5.5 agent-hours, calibrated as explained there. "Est." means a judgement from the cited
evidence.

## Top 10 across everything

| # | Gap | Why it ranks here | Est. | Doc |
|---|---|---|---|---|
| 1 | **Saving drops whatever we don't model** (Unknown entities, IMAGE, WIPEOUT, gradient data, MULTILEADER written as LEADER+MTEXT, groups, named views/UCS, foreign xdata) | Opening a colleague's DWG and saving it silently deletes content: the one thing a CAD user can't forgive | 15–25 | [file formats](file-format-parity.md) |
| 2 | **No real-file test corpus and no check in AutoCAD** | All 36 file tests are synthetic; nobody has verified a CADCraft DWG/DXF opens cleanly in AutoCAD | 10–15 + human | [file formats](file-format-parity.md#tests) |
| 3 | **Object snap tracking, extension, parallel, apparent-intersection snaps are fake** | Drafters place most points with tracking; the F11 button does nothing | 8–12 | [ui](ui-parity.md#tracking-polar-ortho-grid-and-snap--30) |
| 4 | **DWG always saves as R2000; DXF only R2000 ASCII; no code pages** | Can't exchange at 2018 format; pre-2007 CJK/Cyrillic files are garbled | 10–15 | [file formats](file-format-parity.md) |
| 5 | **Stubbed prompt options** in TRIM/EXTEND, OFFSET, ROTATE, PLINE, SPLINE, MTEXT, MLEADER, FILLET | Scripts and muscle memory hit "not available yet" in the most-used commands | 15–25 | [features](#features) |
| 6 | **Ellipse and spline geometry is tessellated** (intersections, offsets, closest points; polyline offset ignores bulges) | Coordinates are subtly wrong; offsets of ellipses come out as polylines | 30–50 | [geometry](geometry-parity.md) |
| 7 | **No xrefs, block editor, dynamic blocks or groups** | Most production drawings use them | 80–120 | [features](#features) |
| 8 | **No autosave, crash recovery, AUDIT or RECOVER** | Work is lost if anything goes wrong; damaged files can't be repaired | 10–15 | [features](#features) |
| 9 | **Dynamic input is read-only; no FROM/M2P/TK/point filters; grip options ignored** | Precision entry at the cursor is how AutoCAD is taught today | 15–25 | [ui](ui-parity.md) |
| 10 | **No CI runs the Rust tests on pull requests, and `cargo fmt` fails on main** | 47 open PRs land without a gate; regressions go unseen | 3–5 | [stability](#stability) |

## Features

| # | Gap | Evidence | User impact | Est. |
|---|---|---|---|---|
| F1 | Prompt options print "not available yet": TRIM/EXTEND cuTting edges, Fence, Crossing, mOde, Project, eRase; OFFSET Erase, Layer, Multiple, Undo; ROTATE Reference; PLINE Angle, CEnter, Direction, Radius, Second pt; SPLINE Method, Knots, Object, Tangency, toLerance; MTEXT Height/Justify/Line spacing/Rotation/Style/Width/Columns; MLEADER options | `cmd/modify.rs:1537,1697-1730,1807-1843`, `cmd/draw.rs:542,623,1106-1128,1297-1320`, `modify2.rs:594,786,809` | High: the commands people use most | 15–25 |
| F2 | No xrefs (XATTACH, XREF palette, REFEDIT, BIND, XCLIP, overlay, paths); DXF xref blocks read with an empty path | `dxf_read.rs:923` | High: multi-file projects | 30–45 |
| F3 | No block editor (BEDIT) or dynamic blocks (parameters, actions, visibility, lookup); dynamic blocks from other files lose their behaviour | issue #185 offers help | High: standard libraries are dynamic | 50–75 |
| F4 | No autosave (SAVETIME), crash recovery files, AUDIT, RECOVER, drawing recovery manager | no SAVETIME anywhere | High: lost work | 10–15 |
| F5 | Arrays are not associative: no ARRAYEDIT; the interactive rectangular array always makes 3×4 at 1.5× bounds | `modify.rs:1356-1360` | Medium-high | 10–15 |
| F6 | FILLET/CHAMFER/JOIN/LENGTHEN/EXTEND don't handle ellipses and splines; FILLET Trim/Multiple and polyline segment pairs; CHAMFER Angle/Method | [geometry-parity](geometry-parity.md#trim-extend-fillet-chamfer-break-join-lengthen--45) | Medium | 10–15 |
| F7 | Dimensions: no DIMSTYLE manager dialog; 43 of ~80 DIMSTYLE variables (DIMTOFL, DIMATFIT, DIMTMOVE, DIMLTYPE, DIMLWD, DIMFXL, DIMTOLJ, DIMALTU …); no smart DIM; no DIMJOGGED, DIMJOGLINE, DIMBREAK, oblique DIMEDIT, inspection; QDIM continuous only; SETVAR DIMSCALE ignored (#66, PR #129) | `doc/src/tables.rs:223-266`, `annotate.rs:64,364` | High for detailers | 25–35 |
| F8 | No TOLERANCE (GD&T feature control frames) | menu "Dimension > Tolerance..." uncovered | Medium (mechanical) | 4–6 |
| F9 | MTEXT editor is a dialog box, not in place; no columns, bullets, numbering, tabs, indents; no fields (FIELD, UPDATEFIELD); no spell check (SPELL) | `ui-egui/dialogs.rs:188-240` | Medium-high | 30–45 |
| F10 | MLEADER: options ignored, no block content, no MLEADEREDIT/ALIGN/COLLECT; MLEADERSTYLE has 5 fields | `annotate.rs:1195-1230`, `tables.rs:417` | Medium | 10–15 |
| F11 | Tables: no cell styles, formulas, data links, table breaking | `cmd/table.rs` | Medium | 10–15 |
| F12 | Annotative scaling: the flag is stored, nothing scales; no scale list, no "Annotative Object Scale" commands | 4 uncovered menu items | Medium-high in layouts | 10–15 |
| F13 | Hatch: 23 patterns (AutoCAD ~85; ours must stay original); gradients render as a flat colour; associativity not updated when the boundary moves; origin always 0,0; island style always normal; no gap tolerance, separate hatches, hatch dialog, MPolygon | `doc/src/library.rs`, `render/lib.rs:430-437`, `cmd/hatch.rs:198,201` | Medium | 15–25 |
| F14 | Plot styles (CTB/STB) not applied; no plot preview, plotter devices, PUBLISH/batch plot; Print opens no plot dialog (#160, PR #200) | `palettes.rs:666` | High for sheet output | 25–35 |
| F15 | Viewports: no polygonal, object or clipped viewports; no viewport scale control in the UI | menu items uncovered | Medium | 6–10 |
| F16 | Groups (GROUP, GROUPEDIT, PICKSTYLE) missing; ACAD_GROUP written empty | `dxf_write.rs` | Medium | 4–6 |
| F17 | Attributes: BATTMAN lists only; no ATTSYNC, EATTEXT, global ATTEDIT | `blocks.rs:300` | Medium | 6–10 |
| F18 | Layer bugs: edits change geometry on locked layers (#87, PR #196); delete/purge removes layers used by blocks (#72, PR #189); rename misses paper space and blocks (#75, PR #191); linetype rename breaks assignments (#74, PR #195); rejected freeze changes colour (#71, PR #188) | issues | High (correctness) | 3–5 (PRs open) |
| F19 | MATCHPROP has no interactive picker or settings dialog; DRAWORDER front/back only; OVERKILL exact duplicates only | `props.rs:333`, `modify.rs:1258` | Medium | 5–8 |
| F20 | OPTIONS/Preferences dialog missing (#23); UNITS has no dialog (#30, PR #38) | `props.rs:419` | Medium | 8–12 |
| F21 | MLINE is two polylines (no MLINE entity, MLSTYLE); REGION makes polylines (no region entity); HELIX is a polyline | `draw2.rs:116,123`, `hatch.rs:267` | Medium-low | 8–12 |
| F22 | Inquiry: MASSPROP approximate; COUNT counts model-space inserts only; no QuickCalc palette (CAL is an evaluator); no DWG Compare | `utility.rs:203,310-352`, `inquiry.rs:305` | Low-medium | 15–25 |
| F23 | No Sheet Set Manager, eTransmit, Share, Traces, Markup Import, Activity Insights | menu items uncovered | Low-medium (team workflows) | 40–70 |
| F24 | No 3D: UCS, 3D views, orbit, solids, surfaces, meshes, sections, visual styles, materials, lights, rendering (150 menu items) | [parity-checklist](parity-checklist.md) | Medium (3D users) | 300–450 |
| F25 | Constraint solver: no DOF report, Smooth not G2, no ellipse/spline/block geometry, no inference while drawing; DCANGULAR can succeed unsatisfied (#73, PR #190) | [geometry-parity](geometry-parity.md#constraint-solver--50) | Low-medium | 20–30 |

## UI/UX

Detail and evidence: [ui-parity.md](ui-parity.md).

| # | Gap | User impact | Est. |
|---|---|---|---|
| U1 | Object snap tracking (acquire points, alignment paths); Extension and Parallel snaps; true apparent intersection | High | 8–12 |
| U2 | Dynamic input: editable pointer and dimension fields, Tab between them, grip dimensional input (PR #60 open for pointer input) | High | 8–12 |
| U3 | FROM, M2P/MTP, TK, `.x/.y/.xy` point filters, `*` WCS prefix, angle override `<a` | High | 4–6 |
| U4 | Typed snap overrides (`END`, `MID` … at a prompt), Shift/Ctrl+right-click snap menu, Tab snap cycling | High | 4–6 |
| U5 | Hot-grip options (Base point, Copy, Undo, Reference) are shown but ignored; multiple hot grips; multifunctional grip menus (Add/Remove Vertex, Convert to Arc) | Medium-high | 6–10 |
| U6 | Selection keywords W, C, WP, CP, F, BOX, AU, SI, M, G; lasso (PR #63); selection cycling; SELECTSIMILAR; ISOLATEOBJECTS/HIDEOBJECTS | Medium-high | 6–10 |
| U7 | Right-click context menus (canvas, selection, command, command line, palettes, tabs) | Medium-high | 6–10 |
| U8 | Snaps missing on some objects: MID on circles/ellipses/splines; NOD on dimension points; INS on ATTDEF/MLEADER; TAN on ellipses/splines; GCEN on splines | Medium | 4–6 |
| U9 | Polar: additional angles, relative to last segment, PolarSnap; isometric snap/grid and ISODRAFT (dead toggle) | Medium | 4–6 |
| U10 | UNDO Mark/Back/BEgin/End/Auto; SETVAR coverage (~60 of ~1,000 variables) | Medium | 6–10 |
| U11 | Zoom Realtime and Dynamic, ZOOMFACTOR; named views (VIEW); tiled model viewports | Medium | 6–8 |
| U12 | Workspaces, navigation bar, CUI customisation, alias editor, ViewCube drag (#7) | Low-medium | 15–25 |
| U13 | Window controls misaligned on macOS (#161); shortcuts shown as macOS symbols on Windows web (#53, PR #54); `.dxf` from Finder fails to open (#158) | Medium | 2–4 |

## File formats

Detail: [file-format-parity.md](file-format-parity.md).

| # | Gap | User impact | Est. |
|---|---|---|---|
| FF1 | Round-trip what we don't model: keep Unknown entities and objects, foreign xdata, extension dictionaries; write IMAGE (with IMAGEDEF), WIPEOUT (PR #147), gradients (PR #162), VIEW/UCS/VPORT tables, groups | High | 15–25 |
| FF2 | Native MULTILEADER objects (read and write) | High | 6–10 |
| FF3 | Write DXF 2004–2018 and binary DXF; save DWG at a chosen version (2018 default) instead of always R2000; SAVEAS version choice (PR #198 adds DWG to the dialog) | High | 8–12 |
| FF4 | Code pages (`$DWGCODEPAGE`, ANSI_932/936/949/950/125x) when reading pre-2007 files | Medium-high (Asia, Eastern Europe) | 3–5 |
| FF5 | Real-file corpus (`storytold/cadcraft-corpus`), open → save → reopen tests, black-box check in AutoCAD | High | 10–15 + human |
| FF6 | DWG throughput: a 10 MB DWG took ~2 min to fail (#57) | High for real projects | 6–10 |
| FF7 | `*D` dimension blocks duplicate on re-save; handles regenerate every save | Medium | 3–5 |
| FF8 | SHX fonts (shape and big fonts) not read | Medium-high (text metrics differ) | 8–12 |
| FF9 | Templates (DWT), user `.lin`/`.pat` files, `.ctb`/`.stb`, `.pc3`, `.pgp` | Medium | 20–30 |
| FF10 | PDF: text as real fonts, layers (OCG), multi-sheet; PDF import and underlays; raster image attach | Medium | 30–50 |
| FF11 | DWF/DWFx, DGN, WMF, EPS, ACIS/STL and other exchange formats | Low-medium | 50–100 |
| FF12 | DWG in the web build | Medium | 6–10 |
| FF13 | R12 binary DXF, pre-R13 DWG | Low | 10–20 |

## Hardware

Detail: [hardware-parity.md](hardware-parity.md).

| # | Gap | Est. |
|---|---|---|
| H1 | 3Dconnexion SpaceMouse (needs the device and 3D views) | 6–10 + human |
| H2 | Multi-monitor: floating palettes on other screens, per-monitor DPI (PR #110 for Windows 11 DPI) | 4–6 |
| H3 | Plotters and system printers (PC3 devices) beyond PDF files | 6–10 |
| H4 | Pen tablets / digitizer (TABLET) | 3–5 + human |
| H5 | Screen magnifier tracking (#39, PR #187) | 1–2 |

## Localization

Detail: [localization-parity.md](localization-parity.md).

| # | Gap | Est. |
|---|---|---|
| L1 | No string catalog: every string is an English literal (PR #36 starts one, with Ukrainian) | 8–12 |
| L2 | AutoCAD for Mac's 7 other languages (zh-Hans, es, fr, ja, de, ko, it) plus pt, hi, ar, id, vi from the standard's list | 40–60 + native review |
| L3 | Complex-script shaping (Arabic, Devanagari) and bidi/RTL in drawing text and UI | 30–45 |
| L4 | IME fixes on every platform (PRs #110–#119); vertical text styles | 6–10 |

## Stability

| # | Gap | Evidence | Est. |
|---|---|---|---|
| S1 | No CI runs `cargo xtask ci` on pull requests (only packaging lint and release builds) | `.github/workflows/` | 2–3 |
| S2 | `cargo fmt --check` fails on main | PR #194 | 0.5 |
| S3 | 47 open PRs, many community bug fixes awaiting review | `gh pr list` | 10–20 |
| S4 | Open correctness issues: extents union far-apart geometry (#166), lines not visible mid-command (#44, PR #56), browser save/plot deliver nothing (#133, #134), closing a tab switches drawings (#76, PR #192) | issues | 6–10 |
| S5 | No smoke test of every command through the UI (PR #42 proposes one) | — | 6–10 |
| S6 | No property-based or fuzz tests for geometry; hostile-input tests cover parsing only | [geometry-parity](geometry-parity.md#tests) | 4–6 |

## Platforms

| # | Gap | Est. |
|---|---|---|
| P1 | Web build: File > Open does nothing (#55, PR #199), saves don't download (#133, PR #193), plot shows raw PDF bytes (#134), no DWG | 6–10 |
| P2 | Windows and Linux get little runtime testing; Linux Mint mouse delay (#25); Windows magnifier (#39) | 6–10 |
| P3 | macOS file associations: opening `.dxf` from Finder fails (#158) | 1–2 |

## Ecosystem and automation

| # | Gap | Est. |
|---|---|---|
| E1 | AutoLISP interpreter (safe subset first: `defun`, `command`, `entget`/`entmod`, selection sets, `getpoint` …), APPLOAD, `acad.lsp` startup | 60–100 |
| E2 | SCRIPT command reading `.scr` files (scripts run today through CLI/MCP/control only) | 1–2 |
| E3 | Express Tools equivalents (TXT2MTXT, BREAKLINE, BURST, NCOPY exists, SUPERHATCH, Move/Copy/Rotate) | 30–50 |
| E4 | Plug-in API (owner decision: Rust/WASM plug-ins rather than ObjectARX/.NET) | 30–50 |
| E5 | CUI customisation and alias (`.pgp`) editor | 15–25 |

## AI features

| # | Gap | Est. |
|---|---|---|
| A1 | Smart Blocks equivalents (placement suggestions, replace, detect and convert, search), using local open models | 25–50 + model licensing |
| A2 | An in-app assistant (AutoCAD has Autodesk Assistant); CADCraft's MCP server already lets any agent drive it | 10–20 |
| A3 | Markup Assist (import markups from PDFs/images) | 10–20 |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | First gaps list: ranked top 10 and every known shortfall by dimension, with evidence and estimates |
