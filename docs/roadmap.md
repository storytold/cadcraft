# CADCraft roadmap detail

> **Last reviewed:** 2026-11-19 · **Last updated:** 2026-11-25 · **Change:** minor (alpha gate evidence and the blocks checklist item updated for what merged from 2026-11-21 to 2026-11-25; no verdict changed; previously minor: alpha gate evidence updated for what merged on 2026-11-19 and 2026-11-20; no row changes its verdict until the reporters confirm; previously major: alpha gate revised from user reports: six rows, Windows first; current focus re-ranked into three tiers; percentages and hours not re-scored; previously minor: re-audit against main 41d9b5e95) · **Target:** Autodesk AutoCAD 2027

Forward-looking plan: milestones with status, the current focus, and what comes next with
estimates. The summary is [ROADMAP.md](../ROADMAP.md); the work items behind every line are in
[gaps.md](gaps.md); numbers come from [target-app-parity.md](target-app-parity.md). Hours are
Opus 5.5 agent-hours.

## Current focus

Work is ranked in three tiers. A higher tier goes first whenever it has work. Within a tier, a fix
already written and waiting in a pull request lands before new work starts, first-party or not.

1. **Urgent: security fixes, leaks, crashes and lost work.** The open security-audit findings
   (#1919–#1935) are checked against the input limits that landed on 2026-11-11, then closed or
   fixed; #1276 fixes the crafted-handle overwrite (#1921). The hostile-input fixes #2820, #2821 and #2765 landed on
   2026-11-20 (#3304, #3305). Crashes and hangs with a reproduction come next: #1603 (web, large DXF),
   #2174 (DWG with 3D content), #1724, #1631 and #3222 (startup).
2. **Parity with AutoCAD.** The failing [alpha gate](#alpha-gate) rows in their order, then the
   [beta checklist](#beta-checklist), apart from the checklist items named under tier 3. 602 pull requests are open (2026-11-25; 661 on 2026-11-20); about 230 of them were
   mergeable, pass CI and are small, and many already fix a gate row.
3. **Lower priority.** Work that affects fewer people and that nothing breaks without: dynamic-block
   authoring, editing an xref in place, exact ellipse and spline geometry, prompt options that need
   3D, and menu breadth as a number.

Done from the previous focus list: precision input (U1–U3; #379, #393, #441, #60, #519), the prompt
options of the most-used commands (#397, #408, #547, #558, #559), autosave, recovery, AUDIT and
RECOVER on the desktop (#406, #416), the Block Editor and REFEDIT, xref attach, plot styles with a plot
dialog and preview (#824, #818, #525, #410, #160, #186).

## Alpha gate

What a drafter needs before CADCraft can be tried on real work. CADCraft is **pre-alpha** because five
of the six rows fail. Revised on 2026-11-19 from user reports: of the issues open or closed in the
last 30 days, about 525 describe the reporter's own drawings, hardware or workflow (98 of them relayed
from Discord), and the ones that name a platform name Windows 135 times, Linux 57, macOS 36 and the
web build 14. Main platform: Windows; macOS, Linux, FreeBSD and the web build stay supported. The
evidence has limits: there is no telemetry, and someone whose copy never starts rarely files an issue.

| Row | Passes? | Evidence | Passes when |
|---|---|---|---|
| It starts on the hardware people have | **no** | Black window or a crash at startup on older Intel graphics, older macOS and some virtual machines (#1724, #1631, #3222, #1780); `CADCRAFT_STENCIL=0` starts without a stencil buffer (#3336) but must be set by hand, and no reporter has confirmed it yet; no automatic fallback renderer | It reaches a usable drawing area on Windows 10 and 11 with old integrated graphics, in a virtual machine, and on the supported macOS and Linux versions, choosing a fallback renderer by itself |
| It opens the DWG and DXF files people already have, correctly and in reasonable time | **no** | Large DWGs are refused at the shared DWG library's size limit (#3102, #2633, #1088); a typed OPEN reads in the background (#3343), but an open can't be cancelled and shows no progress; a layout's paper units are explicit and kept since #3309 (#2836 closed; #1672 open); objects from other applications in a DXF are drawn from their proxy graphics (#2697, #3435, #3437) and dimensions kept as read draw from their `*D` block (#3439); a DWG with 3D content crashes or opens empty (#2174); the corpus holds four files, all written by CADCraft, and CI does not run it | A corpus of files made by AutoCAD and other CAD programs opens in CI with its objects, layouts, viewports and units right, compared with what the program that wrote it shows; a large file opens or says clearly why not; content we don't model is kept and never crashes |
| Saving loses nothing, and other CAD programs open the result | **no** | DXF keeps what we don't model (#528); other applications' objects are kept as proxies (#2917, #2919, #2654); through DWG, custom-class objects without data for the version written are still dropped; an edited dimension keeps its values on save (#3335); saves keep every layout's plot settings (#3390; Plot transparency is still not saved), table cell text in DXF 2007 and later (#3392; #1162 closed), hatch boundary path flags (#3389), fit text (#3391), registered applications (#2918) and the `*D` block of a kept dimension (#3439; #1161 closed); fixes for files AutoCAD or BricsCAD refuse landed on 2026-11-20 (#2904, #1687, #3234, #1728, #2882), and a file saved from a Rhino drawing still crashes AutoCAD 2021 as DWG (#3303); never checked against a corpus in AutoCAD | The corpus round-trips with every object kept and its geometry and properties unchanged, a save names anything it could not keep, and AutoCAD opens the saved files without a recovery prompt |
| Input feels like AutoCAD for Windows | **partial, blocking until confirmed** | Running snaps, polar, ortho, coordinate entry, tracking, FROM/M2P/TK and dynamic input work, and Tab cycles every object snap candidate under the cursor (#1673); landed on 2026-11-20 and not yet confirmed by the people who reported them: a click counts where the button goes down (#3307; #1001, #2092), a quick right-click and Space act as Enter (#3307; #1387), a double-click edits TEXT, attribute definitions and dimension text in place (#3345; #842 stays open for table cells) | A click counts while the pointer moves; right-click and Space repeat or confirm as set in Options; a double-click edits text, multiline text and dimension text in place |
| Draft, dimension and annotate a sheet, with styles | partial, not blocking | All core DIM types, associativity, the Dimension Style Manager (#1460), TEXT/MTEXT, MLEADER, tables, the in-place MTEXT editor, the Array window (#3394), blocks with attributes (ATTDIA's Edit Attributes dialog on INSERT, #3411), xrefs, the Block Editor and REFEDIT work; a few prompt options still print "not available yet" ([gaps.md](gaps.md) F1); ellipse and spline intersections are approximate | No "not available yet" in the most-used draw, modify and annotate commands, apart from options that need 3D |
| Lay out and print a sheet at scale, and the interface can always be restored | **partial, blocking** | Layers, layouts, viewports, page setups, PDF output, plot styles, the Plot dialog, preview and PUBLISH work; a print can come out off scale (#3216); printers are reached only through PDF, PNG and SVG files (F14, H3); MENUBAR, TOOLBAR and Options ▸ Reset bring a hidden menu bar and closed panels back (#3308; #1240 closed), and metric sheets are in millimetres (#3309) | A metric sheet prints 1:1 on a system printer, and one command or key restores the menu bar and default panels from any state |

Moved to the beta checklist on 2026-11-19, with the reports behind each: dynamic-block authoring (none),
editing an xref in place (one), exact ellipse and spline intersections (one).

Ready for real work is ≈ 38% (not re-scored since 2026-10-10), also under the ~40% alpha bar. The
≈ 125–190 h estimate was made for the previous gate and is not re-estimated for this one.

## Beta checklist

Beta means: close to feature complete for mainstream 2D work, opens and saves DWG reliably,
remaining gaps are known bugs and edge features (≈ 75% ready for real work).

| Item | State | Est. |
|---|---|---|
| DWG/DXF: nothing lost on save; MULTILEADER native; version choice; code pages; real-file corpus green; verified in AutoCAD | partial (DXF lossless #528, MULTILEADER #459, version choice #421, code pages #505 landed; corpus and AutoCAD check not started) | 50–75 |
| 2D geometry exact for ellipses and splines; polyline offset correct; relative tolerances | not started | 50–80 |
| Precision input at AutoCAD level (tracking, dynamic input, overrides, point modifiers, grips, selection keywords, context menus) | partial | 90–140 |
| Every Draw/Modify prompt option works; associative arrays | partial | 40–60 |
| Annotation depth: DIMSTYLE manager and variables, missing DIM commands, in-place MTEXT, fields, MLEADER editing, annotative scaling, tables | partial | 90–130 |
| Xrefs, block editor, dynamic blocks, groups, attribute tools | partial (xrefs, Block Editor, REFEDIT, groups, attribute tools and dynamic block references landed (rotation, flip, array, scale, alignment and lookup actions since #3424), XCLIP included; authoring and in-place xref editing not started) | 90–135 |
| Plot styles, plot preview, PUBLISH, non-rectangular viewports | partial | 30–45 |
| Autosave, recovery, AUDIT/RECOVER, Options dialog, templates, SHX, user `.lin`/`.pat` | partial | 40–60 |
| AutoLISP core subset and SCRIPT | done (AutoLISP #758, SCRIPT #693); Visual LISP, DCL and reactors to come | 30–50 |
| Catalog plus AutoCAD for Mac's languages | partial (eighteen catalogs shipped, six of AutoCAD for Mac's seven other languages; Japanese and the catalog extension not started) | 30–45 |
| Stability: CI gate, issue backlog, command smoke sweep, geometry fuzzing | partial | 30–50 |
| Platforms: web open/save, Windows/Linux runtime checks; performance at 1M entities and large DWG | partial | 20–35 |
| **Total to beta** | | **≈ 600–930** |

About 70% of this parallelises across 4–6 agents (one per row), so elapsed time is roughly
150–250 hours.

## Milestones

Status re-scored on 2026-10-10 by depth, not presence (the 2026-10-07 percentages, reported by the
agents that built each milestone, are in the progress log of [ROADMAP.md](../ROADMAP.md)).

| # | Milestone | Status | Remaining (h) |
|---|---|---|---|
| M0 | Skeleton and vertical slice: workspace, AutoCAD-style UI, command line, draw/modify basics, DXF, CLI, MCP, web | done | — |
| M1 | Drafting core: every 2D Draw command and option, dynamic input, object snap tracking, snap overrides | ≈ 55% (percentage from 2026-10-10 and not re-scored: tracking, dynamic input and overrides have landed since) | 35–55 |
| M2 | Modify: grips, PEDIT, SPLINEDIT, LENGTHEN, BLEND, ALIGN, associative arrays, trim/extend for all curves, MATCHPROP UI | ≈ 50% | 45–70 |
| M3 | Layers and properties | ≈ 70% (open layer bugs #71–#75, #87) | 10–15 |
| M4 | Annotation: DIM*, DIMSTYLE manager, MLEADER, in-place MTEXT, fields, tables, annotative scaling, SHX | ≈ 40% | 90–130 |
| M5 | Hatch and blocks: hatch depth, block editor, dynamic blocks, xrefs, groups, attributes | ≈ 30% | 105–160 |
| M6 | Layouts and plotting: viewports, page setups, plot styles, preview, PUBLISH | ≈ 35% | 35–50 |
| M7 | Files: DWG/DXF fidelity and versions, RECOVER/AUDIT, templates, autosave, ETRANSMIT, underlays | ≈ 30% | 170–280 (incl. other formats) |
| M8 | Inquiry and utilities, OPTIONS, customisation | ≈ 25% | 45–70 |
| M9 | Parametric | ≈ 50% | 20–30 |
| M10 | Performance: 1M entities at 60 fps, large DWG loads | ≈ 60% (internal numbers only) | 20–40 |
| M11 | 3D: UCS, orbit, visual styles, solids, surfaces, meshes, sections, rendering | ≈ 2% | 300–450 |
| M12 | Automation: AutoLISP, SCRIPT, action recorder, sheet sets, Express Tools | ≈ 15% (percentage from 2026-10-10 and not re-scored; MCP, CLI, control channel, AutoLISP core #855 and SCRIPT #693 done) | 130–220 |
| M13 | 1.0 polish: preferences, workspaces, localisation, accessibility, signed packages, docs | ≈ 15% (signed packages for every platform exist; themes done; string catalog with eighteen languages, live language switching, #36; percentage from 2026-10-10 and not re-scored) | 110–170 |
| — | Geometry precision (cross-cutting, see [geometry-parity.md](geometry-parity.md)) | ≈ 40% | 70–110 |
| — | AI features | ≈ 0% in-app | 40–80 |
| | **Remaining to full parity** | | **≈ 1,300–2,100** |

Milestone rows overlap a little with the cross-cutting rows; the total is the dimension sum in
[target-app-parity.md](target-app-parity.md), not the column sum.

## After beta

3D foundations (UCS, 3D views, orbit, then solids per `plan/adr/0001`), sheet sets, the full
AutoLISP surface, AI features with openly licensed local models, and every remaining language.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-25 | minor | Alpha gate evidence updated for what merged from 2026-11-21 to 2026-11-25: proxy graphics (#2697, #3435, #3437) and kept dimensions (#3439) in the opens row; layout plot settings, table cell text, hatch flags, fit text and registered applications kept on save (#3390, #3392, #3389, #3391, #2918); Tab through every snap candidate (#1673); the Array window and ATTDIA dialog (#3394, #3411); beta checklist blocks item (dynamic block reference actions, #3424). No verdict changed; pull-request count 661 → 602 |
| 2026-11-20 | minor | Alpha gate evidence updated for what merged on 2026-11-19 and 2026-11-20: the stencil switch (#3336), typed OPEN in the background (#3343), paper units (#3309), dimension values kept on save (#3335), clicks while moving and right-click or Space as Enter (#3307), in-place text editing (#3345), the interface reset (#3308). No verdict changed: the input row waits for confirmation from the reporters; pull-request count 714 → 661 |
| 2026-11-19 | major | Alpha gate revised from user reports: six rows (starts on common hardware; opens existing files; saves without loss; input feel; draft and annotate with styles; print at scale and a restorable interface), main platform Windows; dynamic-block authoring, xref editing in place and exact curve geometry counted under beta only; current focus re-ranked into three tiers; percentages and hours not re-scored |
| 2026-10-24 | minor | Re-audit against main 41d9b5e95: alpha-gate evidence, CI gate (#1987), beta-checklist states, open-PR and catalog counts re-checked against main 41d9b5e95; percentages and hours not re-scored |
| 2026-10-13 | minor | Re-audit against main e707ab4d: alpha-gate evidence for drafting, annotation, blocks and DWG rows; beta-checklist states for DWG/DXF, blocks and languages; current-focus items 4–7; milestone notes M1, M12, M13; percentages and hours not re-scored |
| 2026-10-12 | minor | Block Editor for static blocks landed: "Use blocks" evidence updated; status and hours unchanged (REFEDIT and dynamic blocks remain) |
| 2026-10-11 | minor | Alpha-gate evidence refreshed for merged work (#459, #525, #528, #160, #186, #564); statuses and hours unchanged |
| 2026-10-10 | minor | Merged main: fmt fixed, Ukrainian catalog noted in M13, ready for real work 38% |
| 2026-10-10 | minor | Alpha gate added (5 core workflows, 4 failing); stage pre-alpha |
| 2026-10-10 | major | Created from the old ROADMAP's milestone table and current focus; milestones re-scored by depth; beta checklist added |
