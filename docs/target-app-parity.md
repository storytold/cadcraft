# Target-app parity: CADCraft vs AutoCAD 2027

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** minor (mainstream-practitioner and essentials-user numbers added; ready for real work unchanged) · **Target:** Autodesk AutoCAD 2027

The authoritative assessment of how close CADCraft is to AutoCAD, and how much Opus 5.5 work
remains. [ROADMAP.md](../ROADMAP.md) summarises it, [gaps.md](gaps.md) lists every shortfall,
and the deep checklists are [geometry-parity.md](geometry-parity.md),
[ui-parity.md](ui-parity.md), [file-format-parity.md](file-format-parity.md),
[hardware-parity.md](hardware-parity.md) and [localization-parity.md](localization-parity.md).

## Headline

| Number | Value | Kind |
|---|---|---|
| **Feature breadth** (AutoCAD for Mac menu items with a live command) | **245 / 491 = 50%** (233 = 47% counted by `cargo xtask parity`, which sees engine commands only; +12 UI-only items) | measured ([parity-checklist.md](parity-checklist.md), source scan) |
| Feature breadth, 2D menus only (excluding 150 3D items) | **≈ 65%** (222 / 341 by `cargo xtask parity`'s rule) | measured (source scan, 2026-10-10) |
| Feature depth, weighted by use (features dimension) | **≈ 41%** | estimated (table below) |
| **Ready for real work** | **≈ 37%** | estimated (dimension table below; weighted 37.2%) |
| Mainstream practitioner (2D drafter, weekly tools) | **≈ 27%** | estimated ([below](#mainstream-practitioner)) |
| Essentials user (core tools, default settings) | **≈ 45%** | estimated ([below](#essentials-user)) |
| Remaining to alpha (gate rows) | **≈ 125–190 Opus 5.5 hours** | estimated |
| Remaining to beta | **≈ 600–930 Opus 5.5 hours** | estimated |
| Remaining to full parity | **≈ 1,300–2,100 Opus 5.5 hours** | estimated |

Stage: **pre-alpha**. Four of the five core workflows fail the [alpha gate](roadmap.md#alpha-gate) (precise drafting with tracking, blocks, plotting, lossless DWG save), and ready for real work is under the ~40% alpha bar. Alpha is ≈ 125–190 h away.

## Target and how it was measured

- **Target:** Autodesk AutoCAD 2027 for Mac, version 26.0 (installed under
  `/Applications/Autodesk/AutoCAD 2027`), and AutoCAD 2027 for Windows from Autodesk's
  documentation where the two differ (Ribbon, Sheet Set Manager, Express Tools, .NET).
- **Clean room:** the installed bundle was inspected by **listing names only**: localizations
  (`Base`, `de`, `es`, `fr`, `it`, `ja`, `ko`, `zh_CN` `.lproj` folders), `Contents/PlugIns`
  module names (AcSolids, AcDim, AcTable, AcSection, AcSubD, AcPublish, AcETransmit, AcXDiff /
  DWG Compare, AcCounting, AcMarkupAssist, AcTrace, AcSmartCenter, AcSpeller, Vlide …, about 130
  modules), and `Resources/Support` file names. Nothing inside a file was read, nothing was
  copied, AutoCAD was not launched in this pass.
- **Menu oracle:** `xtask/data/menu-catalog.txt` (feature names only, 491 leaf items, recorded from
  the running AutoCAD 2027 for Mac) against the command registry.
- **Behaviour reference:** `plan/autocad/01-observed-ui.md` and local reference screenshots (black
  box, never committed); Autodesk's public command reference.
- **CADCraft:** the source on `main` at bd2412f (2026-10-10/11), read by four agents in
  parallel: command-by-command option audits (prompts that print "not available yet"), the
  geometry kernel, DXF/DWG paths, interaction code. Plus the 88 GitHub issues and 47 open PRs.
  Disk was nearly full, so nothing was built or run: `cargo xtask parity` was reproduced by a
  Python scan of `CommandSpec` menus with the xtask's matching rule, which gives exactly the same
  uncovered list as the last generated report (233 / 491). Re-run `cargo xtask parity` to
  confirm.

## Mainstream practitioner

The typical AutoCAD professional is a 2D drafter or detailer: draw, modify, dimension, use
blocks, manage layers, plot sheets, and exchange DWGs with colleagues, every week. Excluded per
the standard: 3D, AutoLISP and plug-ins, cloud/AI (Autodesk Assistant, Smart Blocks, Traces,
Share), Sheet Set Manager administration, specialist hardware, languages other than English.

| Area (weekly use) | Weight | Depth | Weighted |
|---|---:|---:|---:|
| Draw (2D) | 15% | 60% | 9.0 |
| Modify | 18% | 50% | 9.0 |
| Precision input (snaps, tracking, coordinate entry, dynamic input) | 13% | 35% | 4.6 |
| Layers and properties | 8% | 70% | 5.6 |
| Dimensions | 10% | 45% | 4.5 |
| Text and MTEXT | 6% | 35% | 2.1 |
| Leaders and tables | 3% | 25% | 0.8 |
| Hatch | 5% | 35% | 1.8 |
| Blocks, attributes, xrefs | 8% | 25% | 2.0 |
| Layouts and plotting | 8% | 35% | 2.8 |
| Inquiry and utilities (incl. autosave) | 3% | 30% | 0.9 |
| Command line and navigation | 3% | 58% | 1.7 |
| **Average depth** | 100% | | **≈ 44.7%** |

Depths are the area percentages above and in [ui-parity.md](ui-parity.md). File I/O is not an
area here; it is counted once, as the file-exchange discount.

| Discount | Factor | Evidence |
|---|---:|---|
| Interaction fidelity (beyond the precision-input area) | ×0.90 | hot-grip options ignored, no context menus, selection keywords W/C/F/WP/CP rejected (#224), transparent 'ZOOM cancels the running command (#220), ELLIPSE/BREAK keywords ignored (#219) |
| Stability on real machines | ×0.85 | 27 of 37 open issues are core-path bugs; about 60 correctness fixes merged on 2026-10-10 alone; lines not visible mid-command (#44); locked layers edited (#87); no CI runs the Rust tests on PRs |
| Exchanging files with AutoCAD users | ×0.80 | saves drop unmodelled entities, images, layer states, multileader styles, true colours and more (#79, #201, #204, #208, #209, #213, #223); DWG always R2000; never checked in AutoCAD; a 10 MB DWG failed (#57) |

**Mainstream practitioner ≈ 44.7% × 0.90 × 0.85 × 0.80 ≈ 27%** (estimated). It is lower than
ready for real work (≈ 37%) for CADCraft, unlike most apps: the full number credits additively
what CADCraft does well beyond 2D drafting (platforms 75%, performance 60%, hardware 45%), while
here the three blockers a working drafter hits every day multiply.

**User evidence** (GitHub issues, PRs and comments, excluding maintainers, counted 2026-10-10):
97 issues filed (60 closed, 37 open) and 127 PRs from 19 outside authors. Praise: 2 (#65:
"I was looking for Autocad replacement for years"; #18), plus thanks on a translation PR (#218).
"Switched from AutoCAD" reports: 0. Open issues: 27 core-path bugs (file data loss, plotting,
selection, layers, web open/save, large DWG), 6 niche requests (ViewCube drag, preferences, SVG
icon, magnifier, constraint edge case, macOS window controls), 4 other (icon offer, trademark,
"Revitcraft", dynamic-block help offer). Interest is real; the open issues are about the core
path, which matches the discounts.

## Essentials user

A casual user who sticks to the essential features: start a drawing, the common draw and modify tools with default
settings, undo, save, print a PDF. Excluded: advanced options, sheet sets, xrefs, plot styles,
annotation styles, exchange edge cases and everything excluded above.

| Core feature | Weight | Depth | Weighted |
|---|---:|---|---:|
| Launch, Start tab, new/open drawing | 10% | 70% | 7.0 |
| LINE, PLINE, CIRCLE, ARC, RECTANG | 20% | 70% | 14.0 |
| MOVE, COPY, ROTATE, SCALE, MIRROR, ERASE | 15% | 70% | 10.5 |
| OFFSET, TRIM (quick), EXTEND, FILLET | 10% | 50% | 5.0 |
| Running object snaps, ortho, polar | 10% | 55% | 5.5 |
| Basic dimensions (linear, aligned, radius) | 8% | 60% | 4.8 |
| TEXT and MTEXT | 5% | 55% | 2.8 |
| Layers (create, colour, on/off) | 7% | 75% | 5.3 |
| Pan, zoom, picking and window selection | 5% | 65% | 3.3 |
| Undo, redo | 3% | 85% | 2.6 |
| Save and reopen own drawings | 4% | 75% | 3.0 |
| Print or export to PDF | 3% | 45% | 1.4 |
| **Average depth** | 100% | | **≈ 65%** |

| Discount | Factor | Evidence |
|---|---:|---|
| Launch and stability | ×0.90 | startup crash on Intel UHD graphics (#33, fixed in 0.4.0), lines not visible (#44), Linux mouse delay (#25), web open/save broken (#55, #133) |
| Discoverability and UI clarity | ×0.90 | dynamic input is a read-only tooltip, no right-click menus, Print has no plot dialog (#160), some menu items only recently got dialogs (#126) |
| Opening files people send them | ×0.85 | DWGs from others untested on real files; a 10 MB DWG failed (#57); `.dxf` from Finder fails (#158); pre-2007 CJK/Cyrillic DXF garbled (no code pages) |

**Essentials user ≈ 65% × 0.90 × 0.90 × 0.85 ≈ 45%** (estimated).

## By feature area (the features dimension)

Weights are the share of a typical AutoCAD user's working time (2D drafters and detailers are
the large majority; 3D modelling and cloud features are a minority). Percentages are depth, not
presence: an option that prints "not available yet" doesn't count.

| Area | Weight | Parity | Remaining (Opus 5.5 h) | Evidence and main gaps | Doc |
|---|---:|---:|---|---|---|
| Draw (2D) | 13% | 60% | 15–25 | ARC 11/11 methods, CIRCLE 6/6, RECTANG all options, REVCLOUD, WIPEOUT, XLINE; PLINE Angle/CEnter/Direction/Radius missing; SPLINE options ignored; MLINE and REGION are polylines; HELIX approximate | [gaps](gaps.md#features) |
| Modify | 15% | 50% | 40–60 | TRIM/EXTEND standard mode, Fence, Crossing, Project, eRase stubs; OFFSET Erase/Layer/Multiple stubs; ROTATE Reference stub; arrays non-associative (no ARRAYEDIT, interactive rectangular array fixed 3×4); MATCHPROP JSON only | [gaps](gaps.md#features) |
| Geometry precision | 10% | 40% | 70–110 | exact for lines/arcs/circles; ellipse and spline intersections, offsets, closest points tessellated; absolute tolerances; polyline offset doesn't recompute bulges | [geometry-parity](geometry-parity.md) |
| Layers and properties | 7% | 70% | 10–15 | Layer Properties Manager with filters, states, VP overrides; QSELECT; Quick Properties; transparency; linetype manager thin; 19 linetypes of ~45 | [gaps](gaps.md#features) |
| Dimensions | 9% | 45% | 25–35 | all core DIM types and associativity; 43 of ~80 DIMSTYLE variables; no DIMSTYLE manager dialog, smart DIM, DIMJOGGED, DIMBREAK, DIMJOGLINE, oblique, TOLERANCE/GD&T, inspection; alt units thin | [gaps](gaps.md#features) |
| Text and MTEXT | 6% | 35% | 30–45 | TrueType, formatting codes, stacking; editor is a dialog, not in-place; most MTEXT prompt options ignored; no columns, bullets, tabs, fields, spell check, SHX, annotative behaviour | [gaps](gaps.md#features) |
| Leaders, tables, fields | 4% | 25% | 25–35 | basic MLEADER and TABLE; leader options ignored, no MLEADEREDIT/ALIGN/COLLECT, no block content; no formulas, data links, FIELD | [gaps](gaps.md#features) |
| Hatch and gradients | 5% | 35% | 15–25 | pick points with islands; 23 patterns of ~85; gradients render flat; associativity flag not acted on; no origin, gap tolerance, separate hatches, hatch dialog | [gaps](gaps.md#features) |
| Blocks, attributes, xrefs, groups | 9% | 25% | 80–120 | static blocks and attributes; no BEDIT, dynamic blocks, xrefs, REFEDIT, XCLIP, GROUP, ATTSYNC, EATTEXT, DesignCenter | [gaps](gaps.md#features) |
| Layouts, viewports, plotting | 9% | 35% | 35–50 | layouts, rectangular viewports, per-VP layers, page setups, PDF; no plot styles (CTB/STB), plot preview, devices, PUBLISH, polygonal/object viewports, annotative scales | [gaps](gaps.md#features) |
| Inquiry and utilities | 4% | 30% | 30–45 | DIST, AREA, ID, LIST, MEASUREGEOM, CAL, FIND, COUNT; no autosave/recovery, AUDIT, RECOVER, OPTIONS, SPELL, QuickCalc, DWG Compare, ETRANSMIT | [gaps](gaps.md#features) |
| Parametric constraints | 2% | 50% | 20–30 | all 12 geometric and the dimensional constraints, own solver; no DOF report, no G2 Smooth, no ellipse/spline geometry, no inference | [geometry-parity](geometry-parity.md#constraint-solver--50) |
| 3D modelling, visualisation, rendering | 5% | 2% | 300–450 | no UCS, 3D views, orbit, solids, surfaces, meshes, sections, visual styles, materials, lights or rendering; 150 of the 491 menu items | [gaps](gaps.md#features) |
| Collaboration and cloud (Sheet Set Manager, Share, Traces, Markup Import, Activity Insights, DWG Compare) | 2% | 5% | 40–70 | none of these exist | [gaps](gaps.md#features) |
| **Features dimension** | 100% | **≈ 41%** | **735–1,115** | | |

## By dimension (ready for real work)

| Dimension | Weight | Parity | Remaining (h) | Kind | Doc |
|---|---:|---:|---|---|---|
| Features (table above) | 35% | 41% | 735–1,115 | estimated | this file |
| UI/UX fidelity (point entry, snaps, tracking, dynamic input, grips, selection, command line, chrome) | 15% | 40% | 110–170 | estimated, weighted in the doc | [ui-parity](ui-parity.md) |
| File formats (DWG/DXF fidelity first) | 20% | 30% | 170–280 | estimated | [file-format-parity](file-format-parity.md) |
| Stability and correctness | 10% | 35% | 30–50 | estimated: about 60 bug fixes merged on 2026-10-10 alone, 30 open issues, `cargo fmt` failing on main (PR #194), no CI runs the Rust tests on pull requests | [gaps](gaps.md#stability) |
| Performance | 5% | 60% | 20–40 | internal numbers only (200k entities fluent); 1M entities and large DWG loads untested; a 10 MB DWG took ~2 min to fail (#57) | [hardware-parity](hardware-parity.md#performance-on-hardware-internal-numbers-only) |
| Ecosystem and automation (AutoLISP/Visual LISP, .NET/ObjectARX, CUI, APPLOAD, Express Tools, App Store) | 5% | 10% | 120–200 | estimated; MCP, CLI and the control channel are ahead of AutoCAD, but no LISP, CUI or plug-ins | [gaps](gaps.md#ecosystem-and-automation) |
| Hardware | 3% | 45% | 15–25 | estimated | [hardware-parity](hardware-parity.md) |
| Platforms | 3% | 75% | 15–25 | macOS, Windows (x64/x86/arm64), Linux, FreeBSD, web: more than AutoCAD; but the web build can't open or save files reliably (#55, #133, #134) and has no DWG, and Windows/Linux get little runtime testing | [gaps](gaps.md#platforms) |
| Localization | 3% | 3% | 70–110 | measured: 0 translated strings, English only; AutoCAD for Mac ships 8 languages | [localization-parity](localization-parity.md) |
| AI features (Autodesk Assistant, Smart Blocks, Markup Assist) | 1% | 20% | 40–80 | none of AutoCAD's in-app AI; agent control over MCP is a different strength | [gaps](gaps.md#ai-features) |
| **Ready for real work** | 100% | **≈ 37%** | **≈ 1,300–2,100** | | |

The dimension weights describe what decides whether a professional can switch: features and
file compatibility dominate.

## Remaining effort and how it was calibrated

**Calibration.** The Claude Code transcripts of the session that built CADCraft show its whole
first version (about 51,000 lines, 290 commands, 47% menu breadth) took **about 11.5 Opus 5.5
agent-hours**: 6.9 hours of the lead agent (2026-10-07 02:16–09:08 UTC) plus about 4.6 hours of
eight sub-agents, some in parallel. (Git commit dates were rewritten earlier than that session
and can't be used.) That is about 25 commands per agent-hour **at presence quality**. Depth costs
far more: the next day external contributors found and fixed about 60 correctness bugs, each a
small PR with a regression test, and the audits in this pass found dozens of prompt options that
only print "not available yet". The rates used here are: an option or bug fix 0.3–0.7 h; a
moderate feature (object snap tracking, editable dynamic input, a dialog) 2–5 h; a subsystem
(xrefs, dynamic blocks, AutoLISP, plot styles) 15–100 h; the 3D kernel 300–450 h; then a 1.3×
tail factor for verification against AutoCAD.

**Why the total went up.** The 2026-10-07 roadmap put full parity at ≈ 29% with ≈ 570 h left,
from agent-reported milestone percentages. This pass audited behaviour instead of presence and
found new evidence: object snap tracking, extension, parallel and apparent snaps are toggles;
dynamic input is read-only; saves drop every entity the reader doesn't model; DWG always saves as
R2000; tolerances are absolute; and the old estimate had no rows for localization, ecosystem,
platforms, stability or AI. The weighted figure rose (29% → 41% features) because breadth and
depth both grew since 2026-10-07 and the weights are now written down; the hours rose because
the remaining work is depth and 3D.

**Parallelism.** About 70% of the work splits across 4–6 agents by area (each feature area,
file formats, UI precision, localization per language), so elapsed time is roughly a quarter of
the agent-hours. Needs a human: native-speaker review of every translation; a licensed real-file
DWG/DXF corpus (files from people who own them); black-box checks in AutoCAD of what we save;
3D mouse and tablet hardware; an owner decision on plug-in hosting and on DGN.

## History of this assessment

| Date | Feature breadth | Weighted parity | Remaining | Note |
|---|---|---|---|---|
| 2026-10-10 (later) | unchanged | ready for real work ≈ 37% (unchanged: already built from the written dimension weights, no hidden judgement); mainstream practitioner ≈ 27%; essentials user ≈ 45% | unchanged | mainstream and essentials numbers added (craftrules standard) |
| 2026-10-10 | 245/491 (50%), engine-only 233/491 | features ≈ 41%, ready for real work ≈ 37% | 1,300–2,100 h | full re-measure in this format (this file) |
| 2026-10-07 | 233/491 (47%) | ≈ 29% overall, 2D ≈ 55% | ≈ 570 h | old ROADMAP: milestone percentages reported by the building agents |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | minor | Mainstream-practitioner (≈ 27%) and essentials-user (≈ 45%) numbers with written weights, discounts and user evidence |
| 2026-10-10 | minor | Stage alpha → pre-alpha under the core-workflow gate; remaining-to-alpha row |
| 2026-10-10 | major | Created from the old ROADMAP's parity section; full re-measure with weights, two numbers, per-area hours and calibration from the build session's transcripts |
