# CADCraft roadmap

**Stage: alpha** · next: beta, ~38 points (37% → 75% ready for real work) and ~600–930 Opus 5.5 hours away

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (full re-measure against AutoCAD 2027 for Mac 26.0; restructured to the craftrules progress-docs standard) · **Target:** Autodesk AutoCAD 2027

CADCraft targets full parity with AutoCAD (2D drafting first, then annotation, layouts and
plotting, DWG, parametrics and 3D), plus what AutoCAD doesn't have: agent control over MCP, a
scriptable CLI, a web build, Linux and FreeBSD builds, and a free licence.

**Why alpha:** the core 2D workflow works end to end (draw, modify, dimension, hatch, blocks,
layouts, plot to PDF, open and save DXF/DWG), which is the alpha bar, and ready-for-real-work sits
at the bottom of the alpha band (≈ 37%). It is not beta: saving drops content we don't model,
DWG always saves as R2000, object snap tracking and dynamic input are not real yet, and there are
no xrefs, dynamic blocks or autosave.

## Headline numbers

| Number | Value | Kind |
|---|---|---|
| Feature breadth (AutoCAD for Mac menu items with a live command) | **50%** (245 / 491; 2D menus only ≈ 65%) | measured ([parity-checklist.md](docs/parity-checklist.md) + UI-only commands) |
| Ready for real work | **≈ 37%** | estimated ([target-app-parity.md](docs/target-app-parity.md)) |
| Remaining to beta | **≈ 600–930 h** (≈ 150–250 h elapsed with 4–6 agents) | estimated |
| Remaining to full parity | **≈ 1,300–2,100 h** (≈ 330–550 h elapsed) | estimated |

Hours are Opus 5.5 agent wall-clock hours, calibrated on this repo's own build: the first version
(51k lines, 290 commands) took ≈ 11.5 agent-hours at presence quality, while depth work runs at
0.3–0.7 h per option or bug fix (≈ 60 such fixes landed on 2026-10-10). Details:
[target-app-parity.md](docs/target-app-parity.md#remaining-effort-and-how-it-was-calibrated).

## By dimension

| Dimension | Parity | Remaining (h) | Doc |
|---|---:|---|---|
| Features (depth, weighted by use) | 41% | 735–1,115 | [target-app-parity.md](docs/target-app-parity.md#by-feature-area-the-features-dimension) |
| UI/UX fidelity | 40% | 110–170 | [ui-parity.md](docs/ui-parity.md) |
| File formats | 30% | 170–280 | [file-format-parity.md](docs/file-format-parity.md) |
| Hardware | 45% | 15–25 | [hardware-parity.md](docs/hardware-parity.md) |
| Localization | 3% | 70–110 | [localization-parity.md](docs/localization-parity.md) |
| Performance | 60% | 20–40 | [hardware-parity.md](docs/hardware-parity.md#performance-on-hardware-internal-numbers-only) |
| Stability | 35% | 30–50 | [gaps.md](docs/gaps.md#stability) |
| Platforms | 75% | 15–25 | [gaps.md](docs/gaps.md#platforms) |
| Ecosystem and automation | 10% | 120–200 | [gaps.md](docs/gaps.md#ecosystem-and-automation) |
| AI features | 20% | 40–80 | [gaps.md](docs/gaps.md#ai-features) |

## Features

| Area | Parity | Remaining (h) | Doc |
|---|---:|---|---|
| Draw (2D) | 60% | 15–25 | [gaps.md](docs/gaps.md#features) |
| Modify | 50% | 40–60 | [gaps.md](docs/gaps.md#features) |
| Geometry precision | 40% | 70–110 | [geometry-parity.md](docs/geometry-parity.md) |
| Layers and properties | 70% | 10–15 | [gaps.md](docs/gaps.md#features) |
| Dimensions | 45% | 25–35 | [gaps.md](docs/gaps.md#features) |
| Text and MTEXT | 35% | 30–45 | [gaps.md](docs/gaps.md#features) |
| Leaders, tables, fields | 25% | 25–35 | [gaps.md](docs/gaps.md#features) |
| Hatch and gradients | 35% | 15–25 | [gaps.md](docs/gaps.md#features) |
| Blocks, attributes, xrefs, groups | 25% | 80–120 | [gaps.md](docs/gaps.md#features) |
| Layouts, viewports, plotting | 35% | 35–50 | [gaps.md](docs/gaps.md#features) |
| Inquiry and utilities | 30% | 30–45 | [gaps.md](docs/gaps.md#features) |
| Parametric constraints | 50% | 20–30 | [geometry-parity.md](docs/geometry-parity.md) |
| 3D modelling and rendering | 2% | 300–450 | [gaps.md](docs/gaps.md#features) |
| Collaboration and cloud | 5% | 40–70 | [gaps.md](docs/gaps.md#features) |

## Languages

AutoCAD 2027 for Mac ships 8 interface languages; CADCraft ships English only. Detail:
[localization-parity.md](docs/localization-parity.md).

| Language | Status | UI translated |
|---|---|---:|
| English | full | 100% |
| Simplified Chinese | none | 0% |
| Spanish | none | 0% |
| Hindi | none | 0% |
| Arabic | none | 0% |
| French | none | 0% |
| Portuguese | none | 0% |
| Indonesian | none | 0% |
| Japanese | none | 0% |
| German | none | 0% |
| Korean | none | 0% |
| Vietnamese | none | 0% |

Other languages shipped: none (Ukrainian is in review, PR #36).

## Upcoming

Ranked; detail and the full beta checklist in [docs/roadmap.md](docs/roadmap.md).

1. Gate and land: fix fmt on main, run `cargo xtask ci` on every PR, land the 47 open PRs (15–25 h).
2. Stop losing data on save; native MULTILEADER; save DWG/DXF at a chosen version (30–45 h).
3. Real-file DWG/DXF corpus and a black-box check in AutoCAD (10–15 h + owner).
4. Precision input: object snap tracking, extension/parallel snaps, FROM/M2P/TK, snap overrides,
   editable dynamic input (25–35 h).
5. Unstub prompt options in TRIM/EXTEND, OFFSET, ROTATE, PLINE, SPLINE, MTEXT, MLEADER (15–25 h).
6. Autosave, recovery, AUDIT, RECOVER (10–15 h).
7. Exact ellipse/spline geometry and correct polyline offsets (30–50 h).
8. Xrefs, block editor, groups, then dynamic blocks (80–120 h).

## Progress log

| Date | Entry |
|---|---|
| 2026-10-10 | Full re-measure against AutoCAD 2027 for Mac in the craftrules progress-docs format: menu breadth 245/491 (50%), ready for real work ≈ 37%, stage alpha. Remaining estimates rose (≈ 570 h → 1,300–2,100 h) on new evidence: behaviour audits found tracking/extension/parallel snaps and dynamic input are not real, saves drop unmodelled content, DWG saves as R2000, and the old estimate had no localization, ecosystem, stability, platform or AI rows. New docs: target-app-parity, gaps, roadmap, architecture, localization, file-format, hardware, UI and geometry parity; `docs/parity.md` became `docs/parity-checklist.md`. |
| 2026-10-10 | About 60 community PRs merged in a day: DXF fidelity (transparency, LWPOLYLINE elevation, frozen VP layers, dimension text rotation, arc-length dims, non-ASCII escapes), CJK font fallback, Save/Don't Save on close, Light/System themes, F2 history, trackpad pan, Linux XWayland drag-and-drop, correctness fixes across draw/modify/constraints. Release v0.4.0. |
| 2026-10-08 | Releases v0.1.0–v0.3.0: Flatpak, AppImage updates, signed macOS universal, Windows x64/x86/arm64, Linux, FreeBSD and web builds. |
| 2026-10-07 | Old assessment: menu breadth 233/491 (47%), ≈ 29% weighted parity, 2D drafting ≈ 55%, ≈ 570 h remaining; milestones as reported then: M1 ~85%, M2 ~65%, M3 ~75%, M4 ~50%, M5 ~35%, M6 ~55%, M7 ~55%, M8 ~20%, M9 ~60%, M10 ~70%, M11 0%, M12 ~25%, M13 ~10%. |
| 2026-10-07 | First version built in one session (≈ 6.9 h elapsed, ≈ 11.5 agent-hours): M0 vertical slice, dimensions, hatch, blocks, DWG bridge, layouts and PDF plotting, GPU canvas, R-tree, constraints, Layer Properties Manager, QSELECT, release pipeline. |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | Restructured to the progress-docs standard (stage, two numbers, dimensions, features, languages, upcoming, log); full re-measure; milestone detail moved to docs/roadmap.md, parity assessment to docs/target-app-parity.md |
| 2026-10-07 | major | Alpha checklist, milestone table, ≈ 29% weighted parity estimate |
