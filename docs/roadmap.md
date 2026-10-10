# CADCraft roadmap detail

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** minor (merged main: fmt fixed, Ukrainian catalog in M13; alpha gate added earlier today) · **Target:** Autodesk AutoCAD 2027

Forward-looking plan: milestones with status, the current focus, and what comes next with
estimates. The summary is [ROADMAP.md](../ROADMAP.md); the work items behind every line are in
[gaps.md](gaps.md); numbers come from [target-app-parity.md](target-app-parity.md). Hours are
Opus 5.5 agent-hours.

## Current focus

1. **Gate and land.** (`cargo fmt` on main is fixed.) Add a PR workflow that runs
   `cargo xtask ci`, then review and land the 47 open PRs, most of them community bug fixes
   (S1–S3). ≈ 15–25 h.
2. **Stop losing data on save** (FF1, FF2, FF7) and save DWG/DXF at a chosen version (FF3).
   ≈ 30–45 h.
3. **A real-file corpus** and a check of our output in AutoCAD (FF5, needs the owner).
4. **Precision input:** object snap tracking, extension/parallel snaps, FROM/M2P/TK, typed snap
   overrides, editable dynamic input (U1–U4). ≈ 25–35 h.
5. **Unstub the prompt options** of the most-used commands (F1). ≈ 15–25 h.
6. **Autosave, recovery, AUDIT, RECOVER** (F4). ≈ 10–15 h.
7. **The rest of the alpha gate:** block editor and REFEDIT, xref attach (F2, F3), and plot styles
   with a plot dialog and preview (F14). ≈ 50–80 h.

Items 2–5 and 7 are the [alpha gate](#alpha-gate) rows.

## Alpha gate

The core workflows a typical AutoCAD professional does every day (craftrules progress-docs
standard). CADCraft is **pre-alpha** because four of the five fail the gate; the failing rows are
the alpha checklist. Main platform: macOS.

| Core workflow | Works end to end? | Evidence | Hours to pass |
|---|---|---|---|
| Draw and modify precisely with object snaps and tracking | **partial, blocking** | Running snaps, polar, ortho and coordinate entry work; object snap tracking, extension and parallel snaps are toggles with no behaviour; no FROM/M2P/TK or typed snap overrides; TRIM/EXTEND standard mode, OFFSET Erase/Layer/Multiple, ROTATE Reference print "not available yet"; ellipse/spline intersections approximate ([ui-parity.md](ui-parity.md), [gaps.md](gaps.md) U1, U3, U4, F1) | 35–50 |
| Dimension and annotate | partial, not blocking | All core DIM types, associativity, DIMSTYLE variables (43 of ~80), TEXT/MTEXT, MLEADER, tables work; no DIMSTYLE dialog, MTEXT editor not in place, MLEADER thin (F7, F9, F10). A drafter can dimension a sheet end to end | 0 for the gate (beta work) |
| Use blocks (insert library blocks, edit them, attributes, xrefs) | **partial, blocking** | Static BLOCK/INSERT/attributes work; no block editor (BEDIT) or REFEDIT, so a block can't be edited in place; no xrefs; dynamic blocks from other drawings lose their behaviour (F2, F3) | 35–55 |
| Layers, layouts and plotting a sheet | **partial, blocking** | Layers, layouts, viewports, page setups and PDF output work; plot styles (CTB/STB) aren't applied, so office-standard monochrome/lineweight plots come out wrong; no plot preview or plot dialog (#160); printers/plotters only via PDF (F14, H3) | 15–25 |
| Save DWG that AutoCAD opens without loss, and reopen others' DWGs | **no** | Saving drops every entity and object we don't model (Unknown, IMAGE, WIPEOUT, gradients, groups, views, foreign xdata); MULTILEADER becomes LEADER+MTEXT; DWG always written as R2000; never checked in AutoCAD; no real-file tests; a 10 MB DWG failed after ~2 min (#57) (FF1–FF6) | 40–60 + owner (real-file corpus, AutoCAD check) |
| **To alpha** | | | **≈ 125–190** |

Ready for real work is ≈ 38%, also under the ~40% alpha bar; closing the gate rows lifts it past
40%.

## Beta checklist

Beta means: close to feature complete for mainstream 2D work, opens and saves DWG reliably,
remaining gaps are known bugs and edge features (≈ 75% ready for real work).

| Item | State | Est. |
|---|---|---|
| DWG/DXF: nothing lost on save; MULTILEADER native; version choice; code pages; real-file corpus green; verified in AutoCAD | not started | 50–75 |
| 2D geometry exact for ellipses and splines; polyline offset correct; relative tolerances | not started | 50–80 |
| Precision input at AutoCAD level (tracking, dynamic input, overrides, point modifiers, grips, selection keywords, context menus) | partial | 90–140 |
| Every Draw/Modify prompt option works; associative arrays | partial | 40–60 |
| Annotation depth: DIMSTYLE manager and variables, missing DIM commands, in-place MTEXT, fields, MLEADER editing, annotative scaling, tables | partial | 90–130 |
| Xrefs, block editor, dynamic blocks, groups, attribute tools | not started | 90–135 |
| Plot styles, plot preview, PUBLISH, non-rectangular viewports | partial | 30–45 |
| Autosave, recovery, AUDIT/RECOVER, Options dialog, templates, SHX, user `.lin`/`.pat` | partial | 40–60 |
| AutoLISP core subset and SCRIPT | not started | 40–70 |
| Catalog plus AutoCAD for Mac's languages | not started | 30–45 |
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
| M1 | Drafting core: every 2D Draw command and option, dynamic input, object snap tracking, snap overrides | ≈ 55% (Draw breadth strong; tracking, dynamic input, overrides missing) | 35–55 |
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
| M12 | Automation: AutoLISP, SCRIPT, action recorder, sheet sets, Express Tools | ≈ 15% (MCP, CLI, control channel done) | 130–220 |
| M13 | 1.0 polish: preferences, workspaces, localisation, accessibility, signed packages, docs | ≈ 15% (signed packages for every platform exist; themes done; string catalog with Ukrainian, live language switching, #36) | 110–170 |
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
| 2026-10-10 | minor | Merged main: fmt fixed, Ukrainian catalog noted in M13, ready for real work 38% |
| 2026-10-10 | minor | Alpha gate added (5 core workflows, 4 failing); stage pre-alpha |
| 2026-10-10 | major | Created from the old ROADMAP's milestone table and current focus; milestones re-scored by depth; beta checklist added |
