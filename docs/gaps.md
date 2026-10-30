# Where CADCraft falls short of AutoCAD

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-28 · **Change:** minor (BEDIT in the Block Editor switches blocks: F3 updated) · **Target:** Autodesk AutoCAD 2027

Every known shortfall, one row each, ranked within each section by user impact. This is the work
list: agents pick from here (top of a section first), and remove or update a row in the same PR
that closes it. Numbers and weights live in [target-app-parity.md](target-app-parity.md); hours
are Opus 5.5 agent-hours, calibrated as explained there. "Est." means a judgement from the cited
evidence. Rows marked **[alpha blocker]** are what fails the [alpha gate](roadmap.md#alpha-gate);
do them first.

## Top 10 across everything

| # | Gap | Why it ranks here | Est. | Doc |
|---|---|---|---|---|
| 1 | **[alpha blocker]** **Saving DWG drops what we don't model** (DXF now writes back unknown entities and objects, foreign xdata, extension dictionaries, reactors, layer states and colour books, #528, and native MULTILEADERs, #459 (not yet tested through DWG); through DWG, acadrust drops custom-class entities and objects, xdata on table records and blocks, entity colour books and reactors) | Opening a colleague's DWG and saving it still deletes some content: the one thing a CAD user can't forgive | 6–10 | [file formats](file-format-parity.md) |
| 2 | **[alpha blocker]** **No real-file test corpus and no check in AutoCAD** | All 36 file tests are synthetic; nobody has verified a CADCraft DWG/DXF opens cleanly in AutoCAD | 10–15 + human | [file formats](file-format-parity.md#tests) |
| 3 | **Ellipse and spline geometry is tessellated** (intersections, offsets, closest points; polyline offset ignores bulges) | Coordinates are subtly wrong; offsets of ellipses come out as polylines | 30–50 | [geometry](geometry-parity.md) |
| 4 | **[alpha blocker]** **No dynamic blocks or REFEDIT; xrefs can't be edited in place or clipped** (xrefs, #525, groups, #417, and the Block Editor for static blocks landed) | Most production drawings use them | 55–90 | [features](#features) |
| 5 | **No autosave in the web build** (the desktop app has SAVETIME autosave, Drawing Recovery, AUDIT and RECOVER since #406) | Browser users lose unsaved work when the tab or the browser goes down | 3–5 | [features](#features) |
| 6 | **No CI runs the Rust tests on pull requests** (`cargo fmt` on main is fixed) | open PRs land without a gate; regressions go unseen | 2–3 | [stability](#stability) |

## Features

| # | Gap | Evidence | User impact | Est. |
|---|---|---|---|---|
| F1 | **[alpha blocker]** Prompt options that still print "not available yet": -ATTEDIT one-at-a-time Position, Height, Angle and Style, colour books in -ATTEDIT Color, -RENAME of materials, plot styles and viewport configurations. SPLINEDIT Join, Edit vertex Add and Elevate order (#559), PEDIT Edit vertex and the other SPLINEDIT options (#558) done: Edit vertex Tangent directions last for the PEDIT session only (LWPOLYLINE stores none). MTEXT Dynamic/Static columns done (#547). SPLINE, MTEXT and MLEADER options done (#408): SPLINE Object fits the polyline's vertices (no spline frame is kept) (STRETCH polygon, CP and WP, and fence selection done; TRIM/EXTEND, PLINE arc mode and Length, XLINE done (#397); OFFSET, ROTATE and FILLET options done earlier) | `cmd/blocks/attedit.rs`, `cmd/props/rename.rs` | Low: the most-used commands are done | 1–3 |
| F2 | XREF editing in place and clipping: XCLIP is missing. REFEDIT, REFSET and REFCLOSE edit a block reference in place (working set, faded rest of the block, everything else locked, save or discard; #813); left: editing an xref in place (its source drawing is not written back; XOPEN opens it), picking nested references, a dimmed look for the locked drawing and a banner with Save and Discard, attribute sync of other references (ATTSYNC). XATTACH, the External References palette, -XREF (Bind/Detach/Path/Unload/Reload/Overlay), XBIND, XOPEN, overlays, relative paths and loading at open are done (#525). Unloaded state and xref-layer overrides last only for the session | `cmd/blocks/xref.rs` | Medium: xrefs can't be edited in place or clipped | 15–25 |
| F3 | **[alpha blocker]** (dynamic-block authoring is beta) No dynamic-block authoring: parameters and actions can't be added in the Block Editor. Dynamic blocks from other files are read and their references edited (visibility, distances, stretch and move, lookups, grips). BEDIT (with the Edit Block Definition dialog, which also starts new blocks and, in the Block Editor, switches to another block after asking to save), BSAVE, BSAVEAS and BCLOSE edit a static block definition, and every reference updates; REFEDIT edits a reference in place; no authoring palettes yet | issue #185 offers help | High: standard libraries are dynamic | 40–65 |
| F4 | Autosave leftovers: no autosave in the web build (browser storage); autosave files are DXF, not DWG; no separate RECOVER mode in the file readers (SAVETIME, Drawing Recovery, AUDIT and RECOVER landed with #406; AUDITCTL `.adt` log landed) | `engine/src/autosave.rs`, `engine/src/audit.rs` | Low-medium (web users) | 2–4 |
| F5 | Associative arrays (ASsociative, Levels, polar ROWs, ARRAYEDIT, ARRAYCLOSE) landed (#448); left: grip editing of arrays (count/spacing grips), ARRAYPATH is not associative, ARRAYEDIT has no item deletion (CTRL+click) and no Key point base points, levels only shift Z (no 3D view) | `cmd/array.rs`, `cmd/array/edit.rs` | Medium | 4–6 |
| F7 | Dimensions: no DIMSTYLE manager dialog; no smart DIM; DIMALTTD/DIMALTTZ, DIMUPT and DIMTXTDIRECTION not modelled; jog height and break size not style settings; DIMBREAK breaks don't follow moved objects; with DIMTFILL 1 the written `*D` block and EXPLODE keep no background mask (a WIPEOUT over the text box would), and a true-color DIMTFILLCLR is written to DXF as the nearest ACI (style variables, DIMJOGGED, DIMJOGLINE, DIMBREAK, DIMEDIT, DIMINSPECT and QDIM modes done, #507; the DIMTFILL/DIMTFILLCLR text background done) | `doc/src/tables.rs`, `cmd/annotate_dimedit.rs`, `cmd/annotate_qdim.rs` | Medium | 10–15 |
| F8 | TOLERANCE leftovers (feature control frames landed, #420: model, our own symbol drawing, DXF, command-line prompts, explode, grips): no Geometric Tolerance dialog, no LEADER/QLEADER tolerance annotation, no in-place editing of a frame's contents | `cmd/tolerance.rs` | Low-medium (mechanical) | 2–4 |
| F10 | MLEADEREDIT (add/remove leader), MLEADERALIGN and MLEADERCOLLECT landed, but a multileader has one landing and one block (a collection keeps the other blocks as block references beside it; MLEADERALIGN has no Options-menu preview); MLEADERSTYLE has 5 fields (block content and spline/no-leader types are per command, not in the style; block content has no attributes) | `annotate.rs`, `cmd/mleader_content.rs`, `tables.rs:417` | Medium | 8–12 |
| F9 | MTEXT is typed in an on-canvas editor with a formatting toolbar (font, height, bold, italic, underline, overline, colour, alignment, lists, tab and special characters) and a ruler for indents and tab stops (#532); left: the box does not lay the text out by the indents, tabs, columns or font weights while typing (bold shows wider, overline is not drawn), no justification (attachment) or columns controls in the editor, existing MTEXT cannot be edited (no DDEDIT/MTEDIT), stacked fractions and other codes show as placeholders; no fields (FIELD, UPDATEFIELD) and no spell check (SPELL): open PR #551. Paragraph indents, tabs, bullets, numbering and columns landed (#547) | `ui-egui/mtext_editor.rs`, `engine/mtext_edit.rs` | Medium | 10–18 |
| F11 | Tables (cell styles, formulas, TABLEDIT, TABLEEXPORT, table breaking and CSV data links landed, #534): formulas aren't read from or written as AutoCAD FIELD objects (other programs see values); number formats apply to formula results only; cell text doesn't wrap; custom cell styles, breaks and data links travel as CADCraft xdata (not through DWG); data links read CSV only (no Excel ranges, no write-back, not on the web); no cell-style tab in the Table Style dialog | `cmd/table/*`, `io/dxf_table.rs` | Low-medium | 6–10 |
| F12 | Annotative scaling done (#468): scale list (SCALELISTEDIT), CANNOSCALE, OBJECTSCALE, ANNOALLVISIBLE/ANNOAUTOSCALE, viewport scale, DXF, status bar list. Left: per-scale positions (ANNORESET is a no-op), a viewport annotation scale apart from its viewport scale, annotative objects inside blocks and old LEADERs, AutoCAD files' per-object scale contexts (read as not annotative) | `cmd/annoscale.rs`, `doc/src/anno.rs` | Low-medium | 4–8 |
| F14 | **[alpha blocker]** Plotting leftovers: binary `.ctb`/`.stb`/`.pc3` files can't be read; model-space PDF plots keep the default margins (a device paper's margins apply to layouts and PNG plots); no system printers (H3); the page setup's "Plot transparency" is not saved in DXF/DWG (the public DXF Reference has no plot layout flag for it), so it lasts for the session and in JSON only. The Plot dialog (#160), Plot Preview and PUBLISH (#186), plot styles (CTB and STB, [plot-styles.md](plot-styles.md), #410) with their end, join and fill styles, plotter devices with paper sizes and printable areas ([plotters.md](plotters.md), #564), and plotted transparency (page setup and Plot dialog option, PLOTTRANSPARENCYOVERRIDE) landed | `io/src/plotter.rs`, `cmd/plotters.rs` | Medium | 4–8 |
| F13 | Hatch: 23 patterns (AutoCAD ~85; ours must stay original); the Hatch and Gradient and Hatch Edit dialogs have no Preview or Inherit Properties (HPDLGMODE 0 keeps HATCH on the command line); no MPolygon; HATCHEDIT and -HATCHEDIT add boundaries from picked points or closed objects and remove chosen loops, but a hatch copied with its boundary isn't linked to the copy; HPGAPTOL closes gaps between curve ends only | `doc/src/library.rs`, `cmd/hatch.rs`, `cmd/hatch/edit.rs` | Medium | 6–10 |
| F15 | Viewports: a clipped viewport and its clip boundary move and scale separately. Polygonal, object and clipped viewports, VPCLIP, MVIEW ON/OFF/Lock/2/3/4 and the status-bar scale list landed (#453), MVIEW Restore in #633 | `cmd/layout/viewports.rs` | Low | 2–4 |
| F16 | Groups: no group bounding box or single grip when a group is selected (GROUPDISPLAYMODE), copying a whole group does not make a new group, no Object Grouping dialog (GROUP, GROUPEDIT, UNGROUP, -GROUP, PICKSTYLE group selection and the DXF/DWG round trip landed, #417); PICKSTYLE's associative-hatch bit is stored but unused | `cmd/groups.rs` | Low | 2–4 |
| F17 | Attributes (ATTSYNC, global -ATTEDIT, BATTMAN editing and DATAEXTRACTION/EATTEXT landed, #433): no verify, preset or lock-position modes, and multiline attributes (read, drawn and saved since 2026-10-12) can't be created or switched in BATTMAN; data extraction covers block references and attribute values (CSV or a table) only — no wizard, nested blocks, other objects' properties, `.dxe` templates or spreadsheet output | `cmd/blocks/battman.rs`, `cmd/blocks/extract.rs` | Low | 4–6 |
| F19 | MATCHPROP Settings is a keyword prompt (no dialog) and has no material, shadow or centre-object settings; TEXTTOFRONT and HATCHTOBACK act on the current space only; OVERKILL doesn't compare polyline segments with other objects (picker and Settings, DRAWORDER Above/Under, TEXTTOFRONT, HATCHTOBACK, OVERKILL tolerance/ignore/overlap/end-to-end options landed, #439) | `props/matchprop.rs`, `draworder.rs`, `overkill.rs` | Low | 2–4 |
| F20 | OPTIONS dialog done (#23): Display, Open and Save, Drafting, Selection, User Preferences and Profiles tabs, and the user profile (object snap modes, drafting toggles, Dynamic Input, PICKBOX, APERTURE, GRIPSIZE, CURSORSIZE, PICKFIRST, PICKADD, QPMODE, model and paper background, snap marker size, system cursor) survives restarts. Left: right-click customization, the autosave interval and default save version on Open and Save (the features themselves landed, #406, #421), AutoSnap magnet and tooltip toggles, colours of the other drawing-area elements, named profiles with import and export | `ui-egui/src/options/` | Low | 2–4 |
| F21 | MLINE, REGION and HELIX are objects since #510, with gaps: MLEDIT tees trim square; UNION/SUBTRACT/INTERSECT only combine regions whose boundaries don't meet; regions are saved as closed polylines (no ACIS data) | `draw2/mline.rs`, `inquiry/regions.rs`, `dxf_region.rs` | Low | 6–10 |
| F22 | Inquiry leftovers (COUNT across spaces and nested blocks, the QuickCalc palette and COMPARE landed, #563; MASSPROP on regions, #510): COMPARE compares the tab shown, not every layout at once (COMPAREEXPORT, the comparison as a new drawing, landed); COMPAREEXPORT puts the two sets of objects in model-space groups, where AutoCAD puts them in blocks; QuickCalc keeps its history and variables for the session only | `file/compare.rs`, `palettes/quickcalc.rs` | Low | 3–5 |
| F23 | No Sheet Set Manager, eTransmit, Share, Traces, Markup Import, Activity Insights | menu items uncovered | Low-medium (team workflows) | 40–70 |
| F24 | No 3D: UCS, 3D views, orbit, solids, surfaces, meshes, sections, visual styles, materials, lights, rendering (150 menu items) | [parity-checklist](parity-checklist.md) | Medium (3D users) | 300–450 |

## UI/UX

Detail and evidence: [ui-parity.md](ui-parity.md).

Outlined fill highlights now keep deterministic contour starts and directions across pointer
repaints and GPU rebuilds, preventing the dashed preview from appearing to flicker (U6).

Options ▸ Selection can now require two separate clicks for a selection window (PICKDRAG 0)
or allow clicks and drag (2, default), saved in the user profile. Drag-only mode (1) remains unavailable.
| # | Gap | User impact | Est. |
|---|---|---|---|
| U4 | Tab snap cycling offers the closest snap of each mode, not every candidate. Typed snap overrides (`END`, `MID` … `NON`), the Shift/Ctrl+right-click snap menu and Tab cycling landed (#393), the menu's Temporary track point in #441 | Low | 1 |
| U5 | Grip menus open on right-click only (no hover menu, no Ctrl to cycle the options); no multifunctional grips on dimensions, MLEADER, hatches or text; Add Vertex on an arc segment straightens it. Hot-grip options (Base point, Copy, Undo, Reference, eXit), multiple hot grips and the polyline/line/arc grip menus landed (#425) | Low-medium | 2–4 |
| U6 | Selection: lasso; the CLass keyword; isolation saved with the drawing (OBJECTISOLATIONMODE 1) and the status-bar isolate button. Press-drag windows (#63), the W, C, F, WP and CP keywords (#226), BOX, AUto, SIngle, Multiple, Group, Add, Remove and Undo, SELECTSIMILAR, ISOLATEOBJECTS/HIDEOBJECTS/UNISOLATEOBJECTS (#428) and selection cycling (#1143) landed | Medium | 3–5 |
| U7 | Right-click menus: no in-place layout tab rename or Move or Copy dialog, no Select All Layouts, no Paste to command line; palette menus are on the Layers rows and the Properties background only. The command menu (held right-click: Enter, Cancel, Recent Input, options, Snap Overrides, Pan/Zoom), the selection menu with object edits, command line, drawing tab, layout tab and palette menus landed (#512) | Low | 1–2 |
| U10 | SETVAR covers about 150 of ~1,000 variables (UNDO options, MREDO and UNDOCTL, and header variables such as CIRCLERAD, OFFSETDIST, POLYSIDES, HPNAME and the dimension variables in SETVAR ? landed, #461; typing a bare variable name as a command runs SETVAR on it) | Low-medium | 3–5 |
| U11 | VIEW Orthographic, UCS and background settings; view twist is kept but not shown (Zoom Realtime and Dynamic, ZOOMFACTOR, named views (VIEW) and tiled model viewports with saved configurations (VPORTS) landed in #463, MVIEW Restore in #633, and the Named Viewports dialog, which restores or deletes a saved configuration) | Low | 1–2 |
| U12 | Workspaces, navigation bar, CUI customisation, ViewCube drag (#7) (the Command Aliases dialog landed, #1381) | Low-medium | 13–23 |
| U13 | `.dxf` from Finder fails to open on macOS (#158) (window controls fixed, #161) | Medium | 1–2 |

## File formats

Detail: [file-format-parity.md](file-format-parity.md).

| # | Gap | User impact | Est. |
|---|---|---|---|
| FF1 | **[alpha blocker]** Round-trip what we don't model. DXF done (#528): unknown entities and objects (with their CLASS records), foreign xdata (hyperlinks included), extension dictionaries, persistent reactors, DICTIONARYVAR and XRECORDs, layer states, colour book colours, references remapped on save; IMAGE, VIEW/UCS, groups, block descriptions, true colours, transparency (#201, #204, #208, #209, #213, #223); WIPEOUT and gradients (#147, #162). Left: through DWG, acadrust drops custom-class entities and objects, xdata on table records and blocks, entity colour book names and reactors; UCS elevation; MTEXT background colours; unknown entities follow only layer edits, and renamed layers and blocks lose their foreign data | High | 6–10 |
| FF2 | Multileader leftovers (native MULTILEADER read/write landed, #459): several landings per multileader (extra leader roots are folded into the lines), per-object overrides (leader colour and linetype, arrowhead symbol, text frame), annotative scale contexts; check the text placement of our MULTILEADER output in AutoCAD | Medium | 3–5 |
| FF3 | Save-version leftovers (DXF/DWG 2000–2018, binary DXF, SAVEAS `version`/`binary` and the default save version landed, #421): no R14 DWG or R12 DXF output; the Save dialog can't pick the version per file (the file picker doesn't report the chosen type; File > Default Save Format sets it); no command-line SAVEAS format prompt | Low | 2–4 |
| FF4 | Code-page leftovers (pre-2007 DXF read in their `$DWGCODEPAGE` landed, #505): Johab (ANSI_1361) and DOS 855/857/860–865/869 read as ANSI_1252; saving declares `ANSI_1252` instead of the file's code page (the text is pure ASCII, so still correct); acadrust approximates DOS code pages in pre-2007 DWG (DOS437 as 866, DOS850 as 1252) | Low | 1–2 |
| FF5 | **[alpha blocker]** Real-file corpus (`storytold/cadcraft-corpus`), open → save → reopen tests, black-box check in AutoCAD | High | 10–15 + human |
| FF6 | **[alpha blocker]** DWG throughput: a 10 MB DWG took ~2 min to fail (#57) | High for real projects | 6–10 |
| FF7 | Table record and object handles from other programs' files are renumbered on the first save (entity handles are kept; `*D` blocks no longer duplicate and once reopened, CADCraft's own files keep every handle across saves, #495) | Low | 4–6 |
| FF8 | SHX fonts (shape and big fonts) not read | Medium-high (text metrics differ) | 8–12 |
| FF9 | `.pc3` plotter configuration files; linetype shape (SHX) elements are skipped (templates and `.dwt`, user `.lin`/`.pat` and `.pgp` alias files landed, #552; `.ctb`/`.stb`, #410) | Low-medium | 5–10 |
| FF10 | PDF import and underlays (PDFATTACH); raster image attach (IMAGEATTACH). PDF export has real text in embedded TrueType subsets, PDF layers, several sheets per file and hyperlinks (#567, #571) | Medium | 20–35 |
| FF11 | DWF/DWFx, DGN, WMF, EPS, ACIS/STL and other exchange formats | Low-medium | 50–100 |
| FF12 | DWG in the web build | Medium | 6–10 |
| FF13 | R12 binary DXF, pre-R13 DWG | Low | 10–20 |
| FF14 | SVG import leftovers (shapes, paths, groups and Inkscape layers landed, #636): no CSS classes (`<style>`), `use` references, fills and gradients, text or embedded images; rounded rectangle corners (`rx`/`ry`) are ignored. Opening such a file names what was left out (#627) | Low-medium | 2–4 |

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
| L1 | Catalog covers the UI (989 rows, #36) but not engine diagnostics, CLI/MCP replies or command history | 4–6 |
| L2 | AutoCAD for Mac's other languages (zh-Hans, es, fr, ja, ko, it; de landed 2026-10-11) plus pt, hi, ar, id, vi from the standard's list | 35–55 + native review |
| L3 | Complex-script shaping for Devanagari and other Indic scripts in drawing text; RTL (mirrored) UI. Arabic-script shaping and bidi in drawing text landed (#287) | 20–30 |
| L4 | IME fixes on every platform (PRs #110–#119); vertical text styles | 6–10 |

## Stability

| # | Gap | Evidence | Est. |
|---|---|---|---|
| S1 | No CI runs `cargo xtask ci` on pull requests (only packaging lint and release builds) | `.github/workflows/` | 2–3 |
| S2 | ~~`cargo fmt --check` fails on main~~ fixed on main (8fceaaf) | — | 0 |
| S3 | 40 open PRs, many community bug fixes awaiting review | `gh pr list` | 10–20 |
| S4 | Open correctness issues: extents union far-apart geometry (#166), lines not visible mid-command (#44), browser plot shows raw PDF bytes (#134) (closing a tab and browser saves fixed, #76, #133) | issues | 4–6 |
| S5 | No smoke test of every command through the UI (PR #42 proposes one) | — | 6–10 |

## Platforms

| # | Gap | Est. |
|---|---|---|
| P1 | Web build: plot shows raw PDF bytes (#134), no DWG (File > Open and save downloads fixed, #55, #133) | 3–5 |
| P2 | Windows and Linux get little runtime testing; Linux Mint mouse delay (#25); Windows magnifier (#39) | 6–10 |
| P3 | macOS file associations: opening `.dxf` from Finder fails (#158) | 1–2 |

## Ecosystem and automation

| # | Gap | Est. |
|---|---|---|
| E1 | AutoLISP beyond the core (#758 landed the language, `command`, entity lists, selection sets, `get…` input, APPLOAD, `(load)`, `c:` commands and `acad.lsp`/`acaddoc.lsp` at startup; #1820 gave APPLOAD its `.lsp` file picker): Visual LISP ActiveX (`vlax-`, `vla-`), DCL dialogs, reactors, an APPLOAD Startup Suite, UCS-aware `trans` | 26–46 |
| E3 | Express Tools equivalents (SUPERHATCH; NCOPY, BREAKLINE, TXT2MTXT, BURST and MOCORO exist: BREAKLINE's Block option takes a block of the drawing, not a `.dwg` file, and TXT2MTXT's Settings are command-line prompts rather than a dialog, without the uniform line spacing option) | 24–40 |
| E4 | Plug-in API (owner decision: Rust/WASM plug-ins rather than ObjectARX/.NET) | 30–50 |
| E5 | CUI customisation (the alias editor, ALIASEDIT's Command Aliases dialog, landed, #1381; edits stay for the session) | 13–23 |

## AI features

| # | Gap | Est. |
|---|---|---|
| A1 | Smart Blocks equivalents (placement suggestions, replace, detect and convert, search), using local open models | 25–50 + model licensing |
| A2 | An in-app assistant (AutoCAD has Autodesk Assistant); CADCraft's MCP server already lets any agent drive it, including read-only structured queries of the current selection (`query_entities selected:true`) | 10–20 |
| A3 | Markup Assist (import markups from PDFs/images) | 10–20 |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-25 | minor | Named Viewports (View ▸ Viewports ▸ Named Viewports..., `vports.named`) lists the saved viewport configurations with a preview and the current one's name, and restores or deletes one; in a layout Restore places it as MVIEW Restore does: U11 narrowed. F15 and U11 no longer list MVIEW Restore (#633), and F21 no longer says PURGE skips multiline styles |
| 2026-10-25 | minor | DIMTFILL limits: with DIMTFILL 1 (the drawing's background color) the written `*D` block and EXPLODE keep no background mask (a WIPEOUT over the text box would), and a true-color DIMTFILLCLR is written to DXF as the nearest ACI: F7 updated |
| 2026-10-26 | minor | BEDIT in the Block Editor (typed, Edit Block, or the Edit Block Definition dialog) switches to another block as AutoCAD's does, asking whether to save the changes not saved yet; the JSON form takes `save`: F3 evidence |
| 2026-10-24 | minor | COMPAREEXPORT (typed, or Export on the comparison bar) opens the comparison as a new untitled drawing, `compare_<this> vs <other>.dwg`: this drawing's objects in one group and the other drawing's differing objects in a second group, in the comparison colors (green, red, gray for objects in both), block contents and style colors included: F22 narrowed |
| 2026-10-24 | minor | DIMTFILL and DIMTFILLCLR: a dimension's text background (none, the drawing's background color, or a color) hides what is drawn under the text; SETVAR, DIMOVERRIDE and style fields, DIMSTYLE groups 69 and 70 in DXF from R2007 (CADCraft xdata before), and a SOLID in the `*D` block and from EXPLODE for DIMTFILL 2: F7 narrowed |
| 2026-10-22 | minor | APPLOAD typed or from Tools ▸ Load Application opens a `.lsp` file browser starting in the last-used folder (kept in the preferences); a picked file loads interactively like a typed name (routines that ask for input wait), the web says to drop the file (no file system), and the JSON form `{path}` reads the path as given and returns the commands the file defined, never opening a dialog (#1820): E1 narrowed |
| 2026-10-16 | minor | ALIASEDIT typed or from Tools ▸ Customize opens the Command Aliases dialog (user aliases next to the built-in ones; Add, Edit, Remove; OK, Cancel, Apply), #1381: U12 and E5 narrowed |
| 2026-10-16 | minor | MOCORO (Tools ▸ Express Tools ▸ Move/Copy/Rotate): select objects and a base point once, then Move, Copy, Rotate, Scale, Base and Undo until eXit, one undo step; JSON form `{base, ops}`: E3 narrowed |
| 2026-10-16 | minor | MLEDIT Merged Tee (MT): the first multiline's outer elements stop at the second one's near side, which opens, and its inner elements run on to the second one's inner element: F21 narrowed, hours unchanged |
| 2026-10-15 | minor | BURST (Tools ▸ Express Tools ▸ Explode Attributes to Text): block references explode as EXPLODE does, visible attribute values become text with the attribute's style, height, position, layer and colour, invisible attributes and the definitions go: E3 narrowed |
| 2026-10-13 | minor | Stabilized filled-object highlight dash anchors by canonicalizing boundary traversal order; renderer, CPU pointer-frame and GPU rebuild tests cover the fix (U6) |
| 2026-10-14 | minor | HATCHEDIT adds boundary loops from internal points or closed objects and removes selected loops, in the dialog, command line and JSON command: F13 narrowed |
| 2026-10-15 | minor | `entities` / MCP `query_entities` accept `selected:true`, combined with existing filters and paging; selection, drawing and undo/redo stay unchanged. No parity estimate change. |
| 2026-10-15 | minor | BREAKLINE (Tools ▸ Express Tools ▸ Break-line Symbol): one polyline between two points with a break symbol, Block/Size/Extension options, size times DIMSCALE: E3 narrowed |
| 2026-10-15 | minor | TXT2MTXT (Tools ▸ Express Tools ▸ Convert Text to Mtext): selected TEXT, and MTEXT, combined into one MTEXT top-down or in selection order with optional word wrap, or one MTEXT per TEXT; SEttings remembered for the session: E3 narrowed |
| 2026-10-13 | minor | Selection window gesture preference (PICKDRAG 0/2) added to Options ▸ Selection and saved in the profile (U6), with Portuguese and Ukrainian labels |
| 2026-10-12 | minor | Object transparency (entity, ByLayer, ByBlock) is drawn: on screen (TRANSPARENCYDISPLAY, on by default), in PNG/SVG exports, and in plots when the page setup's "Plot transparency" is on (PDF alpha, PNG blending, SVG opacity; PLOTTRANSPARENCYOVERRIDE): F14 narrowed |
| 2026-10-12 | minor | Hatch and Gradient dialog (HATCH, GRADIENT; HPDLGMODE) and Hatch Edit dialog (HATCHEDIT, right-click, double-click a hatch): F13 narrowed to Preview, Inherit Properties and boundary add/remove in edit mode |
| 2026-10-12 | minor | A bare system variable name typed at the command line (ORTHOMODE, GRIDMODE…) runs SETVAR on that variable, as in AutoCAD; U10 narrowed |
| 2026-10-13 | minor | STRETCH takes crossing and window polygons (CP, WP), BOX and a fence (F), typed and as JSON (`polygon`, `fence`): F1 narrowed |
| 2026-10-12 | minor | Property-based and hostile-input tests for geometry numerics landed (deterministic xorshift generator, no new dependency; known answers at 1e6–1e7 coordinates): S6 removed |
| 2026-10-12 | trivial | F13: HATCHEDIT refuses a bad scale or angle before editing (#724) |
| 2026-10-12 | minor | F3: dynamic block references show their grips (distance arrows, position squares, visibility/lookup lists, per visibility state) and drag them (`blocks.reference.grips`, `blocks.reference.grip`) |
| 2026-10-17 | minor | AUDITCTL system variable and .adt log generation in AUDIT/RECOVER landed: F4 narrowed (no .adt log removed) |
| 2026-10-12 | minor | Multiline attributes read, drawn and saved (they lost their text before): F17 narrowed to creating them |
| 2026-10-13 | minor | F3: opening a dynamic block's own drawing (authoring elements in model space) offers the Block Editor on `<Current Drawing>`, as AutoCAD does |
| 2026-10-12 | minor | Edit Block Definition dialog (block list, preview, description; a new name starts a new block): F3 text updated, hours unchanged |
| 2026-10-12 | minor | Block Editor (BEDIT, BSAVE, BSAVEAS, BCLOSE) for static blocks landed: rows 4 (65–100 → 55–90 h) and F3 (50–75 → 40–65 h) narrowed to REFEDIT and dynamic blocks |
| 2026-10-12 | minor | In-place MTEXT editor with formatting toolbar and indent/tab ruler landed (#532): F9 narrowed to the editor leftovers, fields and spell check |
| 2026-10-12 | minor | SCRIPT command reading `.scr` files landed (runs a file through the same script engine as CLI/MCP; nested scripts refused): E2 removed |
| 2026-10-11 | minor | Refreshed for merged features: code pages (#505), tracking leftovers (#441), dynamic input/grip input (#519, #425) and SPLINEDIT Join/Add/Elevate (#559) closed their top-10 rows (top 10 now has 6 rows until the next review re-ranks); MULTILEADER (#459) and xrefs (#525) narrowed rows 1 and 4; F1 narrowed to the remaining stubs (#558, #559, #547); F5, F7, F9, F12, F14, F15, F17, F20, F22, U5, U7, U10, FF2, FF4, FF10 narrowed; F18, U2, U8, U9 removed (done); U13, S3, S4, P1 updated for merged fixes |
| 2026-10-11 | minor | German interface catalog landed: L1 row count 936 → 989, L2 narrowed (de done; 40–60 → 35–55 h) |
| 2026-10-11 | minor | SPLINE Knots/Tangency/toLerance/Object, MTEXT Height/Justify/Line spacing/Rotation/Style/Width/Columns, MLEADER spline and no-leader types and block content landed (#408): top-10 row 5, F1 and F10 narrowed |
| 2026-10-11 | minor | Object snap tracking, Extension, Parallel and true Apparent Intersection landed (#379): top-10 row 3 and U1 narrowed to the leftovers |
| 2026-10-10 | minor | Merged main: closed or narrowed rows for the Ukrainian catalog, UNITS dialog, shortcut display, dynamic input pointer boxes, press-drag windows, SETVAR DIM*, WIPEOUT and gradients, fmt |
| 2026-10-10 | minor | Alpha blockers marked (rows failing the core-workflow gate) |
| 2026-10-10 | major | First gaps list: ranked top 10 and every known shortfall by dimension, with evidence and estimates |
