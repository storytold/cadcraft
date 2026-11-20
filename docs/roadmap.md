# CADCraft roadmap detail

> **Last reviewed:** 2026-11-19 · **Last updated:** 2026-11-19 · **Change:** major (alpha gate revised from user reports: six rows, Windows first; current focus re-ranked into three tiers; percentages and hours not re-scored; previously minor: re-audit against main 41d9b5e95) · **Target:** Autodesk AutoCAD 2027

Forward-looking plan: milestones with status, the current focus, and what comes next with
estimates. The summary is [ROADMAP.md](../ROADMAP.md); the work items behind every line are in
[gaps.md](gaps.md); numbers come from [target-app-parity.md](target-app-parity.md). Hours are
Opus 5.5 agent-hours.

## Current focus

Work is ranked in three tiers. A higher tier goes first whenever it has work. Within a tier, a fix
already written and waiting in a pull request lands before new work starts, first-party or not.

1. **Urgent: security fixes, leaks, crashes and lost work.** The open security-audit findings
   (#1919–#1935) are checked against the input limits that landed on 2026-11-11, then closed or
   fixed; #1276 fixes the crafted-handle overwrite (#1921). Hostile-input fixes are waiting in
   #2820, #2821 and #2765. Crashes and hangs with a reproduction come next: #1603 (web, large DXF),
   #2174 (DWG with 3D content), #1724, #1631 and #3222 (startup).
2. **Parity with AutoCAD.** The failing [alpha gate](#alpha-gate) rows in their order, then the
   [beta checklist](#beta-checklist), apart from the checklist items named under tier 3. 714 pull requests are open (2026-11-19); about 230 of them are
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
| It starts on the hardware people have | **no** | Black window or a crash at startup on older Intel graphics, older macOS and some virtual machines (#1724, #1631, #3222, #1780); `CADCRAFT_STENCIL=0` is waiting in #1596; no automatic fallback renderer | It reaches a usable drawing area on Windows 10 and 11 with old integrated graphics, in a virtual machine, and on the supported macOS and Linux versions, choosing a fallback renderer by itself |
| It opens the DWG and DXF files people already have, correctly and in reasonable time | **no** | Large DWGs are refused at a size limit or freeze the window (#3102, #2633, #1088); layouts come in at the wrong scale or in the wrong units (#1672, #2836); a DWG with 3D content crashes or opens empty (#2174); the corpus holds four files, all written by CADCraft, and CI does not run it | A corpus of files made by AutoCAD and other CAD programs opens in CI with its objects, layouts, viewports and units right, compared with what the program that wrote it shows; a large file opens or says clearly why not; content we don't model is kept and never crashes |
| Saving loses nothing, and other CAD programs open the result | **no** | DXF keeps what we don't model (#528); other applications' objects are kept as proxies (#2917, #2919, #2654); through DWG, custom-class objects without data for the version written are still dropped; fixes for files AutoCAD or BricsCAD refuse are waiting (#2904, #1687, #3234); never checked against a corpus in AutoCAD | The corpus round-trips with every object kept and its geometry and properties unchanged, a save names anything it could not keep, and AutoCAD opens the saved files without a recovery prompt |
| Input feels like AutoCAD for Windows | **partial, blocking** | Running snaps, polar, ortho, coordinate entry, tracking, FROM/M2P/TK and dynamic input work; a click while the mouse moves can be lost (#1001, #2092), right-click and Space don't always act as Enter (#1387), a double-click doesn't edit TEXT in place (#842); fixes are waiting in pull requests | A click counts while the pointer moves; right-click and Space repeat or confirm as set in Options; a double-click edits text, multiline text and dimension text in place |
| Draft, dimension and annotate a sheet, with styles | partial, not blocking | All core DIM types, associativity, the Dimension Style Manager (#1460), TEXT/MTEXT, MLEADER, tables, the in-place MTEXT editor, blocks with attributes, xrefs, the Block Editor and REFEDIT work; a few prompt options still print "not available yet" ([gaps.md](gaps.md) F1); ellipse and spline intersections are approximate | No "not available yet" in the most-used draw, modify and annotate commands, apart from options that need 3D |
| Lay out and print a sheet at scale, and the interface can always be restored | **partial, blocking** | Layers, layouts, viewports, page setups, PDF output, plot styles, the Plot dialog, preview and PUBLISH work; a print can come out off scale (#3216); printers are reached only through PDF, PNG and SVG files (F14, H3); a hidden menu bar or closed panels can't always be brought back (#1240) | A metric sheet prints 1:1 on a system printer, and one command or key restores the menu bar and default panels from any state |

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
| Xrefs, block editor, dynamic blocks, groups, attribute tools | partial (xrefs, Block Editor, REFEDIT, groups, attribute tools and dynamic block references landed, XCLIP included; authoring and in-place xref editing not started) | 90–135 |
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
| 2026-11-19 | major | Alpha gate revised from user reports: six rows (starts on common hardware; opens existing files; saves without loss; input feel; draft and annotate with styles; print at scale and a restorable interface), main platform Windows; dynamic-block authoring, xref editing in place and exact curve geometry counted under beta only; current focus re-ranked into three tiers; percentages and hours not re-scored |
| 2026-10-24 | minor | Re-audit against main 41d9b5e95: alpha-gate evidence, CI gate (#1987), beta-checklist states, open-PR and catalog counts re-checked against main 41d9b5e95; percentages and hours not re-scored |
| 2026-10-13 | minor | Re-audit against main e707ab4d: alpha-gate evidence for drafting, annotation, blocks and DWG rows; beta-checklist states for DWG/DXF, blocks and languages; current-focus items 4–7; milestone notes M1, M12, M13; percentages and hours not re-scored |
| 2026-10-12 | minor | Block Editor for static blocks landed: "Use blocks" evidence updated; status and hours unchanged (REFEDIT and dynamic blocks remain) |
| 2026-10-11 | minor | Alpha-gate evidence refreshed for merged work (#459, #525, #528, #160, #186, #564); statuses and hours unchanged |
| 2026-10-10 | minor | Merged main: fmt fixed, Ukrainian catalog noted in M13, ready for real work 38% |
| 2026-10-10 | minor | Alpha gate added (5 core workflows, 4 failing); stage pre-alpha |
| 2026-10-10 | major | Created from the old ROADMAP's milestone table and current focus; milestones re-scored by depth; beta checklist added |
