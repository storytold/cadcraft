# CADCraft roadmap

Status as of **2026-10-07**. CADCraft targets full parity with AutoCAD (2D drafting first, then
annotation, layouts and plotting, DWG, parametrics and 3D), plus things AutoCAD doesn't have:
agent control over MCP, a scriptable CLI, a web build and a free licence.

## At a glance

| Question | Answer |
|---|---|
| **How close is an alpha?** | **≈ 75% of the way.** About **50 Opus 5.5 wall-clock hours** remain (≈ 15–20 hours elapsed with 3–4 agents in parallel). |
| **How far is 100% AutoCAD parity?** | **≈ 29% parity overall** (weighted by how much each area matters). About **570 Opus 5.5 wall-clock hours** remain (≈ 150–190 hours elapsed in parallel). |
| **2D drafting parity** (what most AutoCAD users do every day: M1–M8) | **≈ 55%.** About **245 hours** remain. |
| **The biggest single gap** | 3D modelling (M11): solids, surfaces, meshes, visual styles and rendering — ≈ 200 hours on its own. |

### What "alpha" means here

A person can do real 2D drafting work in CADCraft and trust it with their files:

- Installable, signed builds for macOS, Windows, Linux, FreeBSD and the web from the release pipeline.
- Drawings from AutoCAD open correctly, and CADCraft's DXF/DWG files open cleanly in AutoCAD and other readers.
- Every common 2D draw, modify, annotate, layer, block, layout and plot command works, both with the mouse and at the command line.
- No crashes and no lost work: autosave and recovery, plus undo that always works.

### Alpha checklist

| Item | State | Hours left |
|---|---|---|
| 2D draw and modify commands, snaps, tracking, grips | done (minor options missing) | — |
| Dimensions, text, MTEXT, leaders, tables, TrueType fonts | done | — |
| Hatch with pick points, blocks, attributes | done | — |
| Layouts, viewports, MSPACE/PSPACE, page setup, PLOT to PDF | done | — |
| Parametric constraints, constraint bars and dimensional constraints on the canvas, Parameters Manager | done | — |
| DXF/DWG round trip of everything above | done for our data; MULTILEADER is written as LEADER + MTEXT; `*D` dimension blocks duplicate on re-save | 6 |
| Confirmed opening in AutoCAD (black-box check) | not yet confirmed | 4 |
| Layer Properties Manager (VP overrides, states, filters), QSELECT, Quick Properties | done | — |
| Autosave, crash recovery, RECOVER/AUDIT | partial | 6 |
| Options dialog and preferences | partial | 6 |
| Plot preview | missing | 4 |
| Scripted smoke test of all 290 commands through the UI, then fixing what it finds | not started | 12 |
| Release pipeline: first green signed build on every platform | workflows written; first run being started | 6 |
| Getting-started docs and the web build hosted | partial | 4 |
| **Total** | | **≈ 50** |

## Where we are

| Measure | Value |
|---|---|
| Menu breadth (`cargo xtask parity`, [docs/parity.md](docs/parity.md)) | **233 / 491 reference menu items (47%)** |
| Registered commands | 290 (133 with interactive prompts) |
| **Estimated overall feature parity (weighted by how much each area matters)** | **≈ 29%** |
| Performance | 200k entities: pick 0.001 ms, pick after an edit ≈ 4 ms (incremental R-tree), GPU canvas ≈ 4 ms per frame |
| Tests | geometry, colour, document, fonts, render, DXF/DWG round trips, constraints solver, engine (commands, prompts, snaps, selection, undo, scripts, hostile input), MCP agent tasks, xtask |
| Gates | `cargo xtask ci`: fmt, clippy -D warnings, tests, asset attribution, layering, wasm — green |

Menu breadth counts menu items that exist; the weighted parity estimate also counts depth (options,
edge cases, dialogs), which is why it is lower.

## Milestones

| # | Milestone | Status | Estimate (Opus 5.5 wall-clock hours) |
|---|---|---|---|
| M0 | Skeleton + vertical slice: workspace, AutoCAD-style UI, command line, draw/modify basics, DXF, CLI, MCP, web | **done** | — |
| M1 | Drafting core: every Draw-menu 2D command and option, dynamic input fields, object snap tracking, temporary snap overrides | ~85% (all 2D Draw items exist) | 8 |
| M2 | Modify: grips (stretch/move/rotate/scale/mirror, multifunctional), PEDIT, SPLINEDIT, LENGTHEN, BLEND, ALIGN, associative arrays, trim/extend for all curve types, MATCHPROP UI, Quick Properties | ~65% (PEDIT, LENGTHEN, ALIGN, BLEND, grips API, TRIM on splines) | 18 |
| M3 | Layers & properties: full Layer Properties Manager (filters, VP overrides, states), linetype manager, lineweight display, transparency, QSELECT dialog, Properties for every object type | ~75% (Layer Properties Manager with VP overrides/states/filters, QSELECT, Quick Properties) | 10 |
| M4 | Annotation: DIM* commands, DIMSTYLE manager, associative dimensions, MLEADER + styles, in-place MTEXT editor, fields, tables + table styles, annotative scaling, TrueType fonts, SHX reader | ~50% (TrueType text incl. `.ttc`, per-character font fallback and FONTALT for unread SHX fonts, MTEXT formatting codes, full DIMSTYLE variables + overrides, all arrowheads, associative dimensions, TABLE editing; DXF round trip; not yet: native MULTILEADER objects, jogged/ordinate polish, tolerance frames, fields, annotative scaling, SHX) | 30 |
| M5 | Hatch & blocks: pick-point boundary detection, islands, gradients, BLOCK/WBLOCK/INSERT dialogs, attributes (ATTDEF/ATTEDIT/BATTMAN), block editor, dynamic block parameters, xrefs, groups, Blocks palette | ~35% (pick-point boundaries with islands, gradients, BLOCK/INSERT, attributes, nested blocks) | 70 |
| M6 | Layouts & plotting: paper space, viewports (rect/polygonal/object, scale, lock, per-VP layers), page setups, PLOT to PDF/PNG/SVG, plot styles (CTB/STB), PUBLISH | ~55% (layouts, viewports, MSPACE/PSPACE through viewports, page setup, PLOT/EXPORTPDF) | 25 |
| M7 | Files: DWG read/write (done via acadrust), DXF fidelity (all object types, round-trip of unknown data), RECOVER/AUDIT, PURGE, templates, autosave, ETRANSMIT, PDF/raster underlays | ~55% (DXF round trip of dimension styles, associativity, tables and constraints) | 55 |
| M8 | Inquiry & utilities: MEASUREGEOM, MASSPROP, QuickCalc, Find/Replace, spell check, COUNT, DWG Compare, Settings/OPTIONS, CUI-style customisation, alias editor | ~20% | 30 |
| M9 | Parametric: geometric + dimensional constraints, AutoConstrain, Parameters Manager (constraint solver, see plan/adr/0001) | ~60% (own solver crate, all GC*/DC* commands, AUTOCONSTRAIN, PARAMETERS, conflict detection, re-solve after edits, DXF persistence; not yet: inference while drawing, Smooth as true G2) | 12 |
| M10 | Performance: GPU canvas (wgpu batches), R-tree spatial index, incremental regen, 1M-entity drawings at 60 fps | ~70% (GPU canvas, incremental R-tree: pick 50 ms → 0.001 ms at 200k entities, ≈ 4 ms after an edit) | 12 |
| M11 | 3D: UCS, orbit, visual styles, solids (box…loft, booleans, fillet edges), meshes, surfaces, sections, rendering | 0% | 200 |
| M12 | Automation: an embedded safe AutoLISP-compatible interpreter, action recorder, sheet sets, CLI/MCP parity tests | ~25% | 60 |
| M13 | 1.0 polish: preferences, workspaces, themes, localisation, accessibility, signed packages for every platform, docs | ~10% | 40 |
| | **Remaining total** | | **≈ 570 hours** |

At roughly 570 more hours of Opus 5.5 wall-clock work (with parallel agents this compresses to
about 150–190 hours of elapsed time), CADCraft would reach broad AutoCAD parity. 2D drafting parity
(M1–M8) is the first ≈ 250 hours.

## Current focus

1. Finish the alpha checklist above (release builds, AutoCAD open check, smoke test of every command, autosave/recovery, plot preview).
2. Layer filter groups, constraint bar polish (close buttons, hover highlight).
3. Native MULTILEADER objects in DXF; stop `*D` block duplication on re-save.
4. Then 3D foundations (UCS, orbit, solids via truck/csgrs per plan/adr/0001).
