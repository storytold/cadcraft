# CADCraft roadmap

**Stage: pre-alpha** · next: alpha, when the six rows of the [alpha gate](docs/roadmap.md#alpha-gate) pass (five fail today; the gate was revised on 2026-11-19 and its hours are not re-estimated yet)

> **Last reviewed:** 2026-11-19 · **Last updated:** 2026-11-25 · **Change:** minor (Upcoming updated for what merged from 2026-11-21 to 2026-11-25; previously minor: Upcoming updated for what merged on 2026-11-19 and 2026-11-20; previously major: alpha gate revised from user reports: six rows, Windows first; Upcoming re-ranked into three priority tiers; previously minor: re-audit against main 41d9b5e95) · **Target:** Autodesk AutoCAD 2027

CADCraft targets full parity with AutoCAD (2D drafting first, then annotation, layouts and
plotting, DWG, parametrics and 3D), plus what AutoCAD doesn't have: agent control over MCP, a
scriptable CLI, a web build, Linux and FreeBSD builds, and a free licence.

**Why pre-alpha:** five of the six rows of the [alpha gate](docs/roadmap.md#alpha-gate) fail. The gate
was revised on 2026-11-19 from what people report: about 525 of the issues open or closed in the
last 30 days come from people describing their own drawings, hardware or workflow, and the ones that
name a platform name Windows most (135; Linux 57, macOS 36, web 14). They report an app that
doesn't start on older graphics hardware, large or foreign DWG files that are refused, freeze or open
wrong, clicks lost while the mouse moves, layouts in the wrong units, and a menu bar or panels that
can't be brought back. Drafting, dimensioning and annotating a sheet nearly passes. Dynamic-block
authoring, editing an xref in place and exact ellipse and spline geometry had almost no reports behind
them and moved to the beta checklist. Ready for real work (≈ 38%, not re-scored since 2026-10-10) is
also under the ~40% alpha bar. Beta is ≈ 600–930 h away.

## Headline numbers

| Number | Value | Kind |
|---|---|---|
| Feature breadth (AutoCAD for Mac menu items with a live command) | **50%** (245 / 491 at the 2026-10-10 re-measure, including UI-only commands; not re-measured since; the generated checklist counts engine commands only and now reads 298 / 491; 2D menus only ≈ 65%) | measured ([parity-checklist.md](docs/parity-checklist.md) + UI-only commands) |
| Ready for real work | **≈ 38%** | estimated ([target-app-parity.md](docs/target-app-parity.md)) |
| Mainstream practitioner (2D drafter) | **≈ 28%** (lower than the full number: file exchange, stability and interaction discounts multiply) | estimated ([target-app-parity.md](docs/target-app-parity.md#mainstream-practitioner)) |
| Essentials user | **≈ 46%** | estimated ([target-app-parity.md](docs/target-app-parity.md#essentials-user)) |
| Remaining to alpha | **≈ 125–190 h** (estimated 2026-10-10 for the previous gate; not re-estimated for the revised one) | estimated |
| Remaining to beta | **≈ 600–930 h** (≈ 150–250 h elapsed with 4–6 agents) | estimated |
| Remaining to full parity | **≈ 1,300–2,100 h** (≈ 330–550 h elapsed) | estimated |

### Readiness by audience

| Audience | Ready % | Opus 5.5 agent wall-clock hours to ~95% | Work that dominates |
|---|---:|---|---|
| Full target (ready for real work) | ≈ 38% | ≈ 1,150–1,850 h (≈ 70% parallelises across 4–6 agents: ≈ 300–480 h elapsed) | 3D kernel (300–450 h), DWG/DXF and other formats, AutoLISP and ecosystem, blocks/xrefs/dynamic blocks, localization |
| Mainstream practitioner (2D drafter) | ≈ 28% | ≈ 570–880 h (≈ 70% parallelises: ≈ 150–240 h elapsed) | lossless DWG exchange and a real-file corpus, precision input (tracking, dynamic input), 2D geometry precision, blocks/xrefs, annotation depth, stability backlog |
| Essentials user | ≈ 46% | ≈ 140–225 h (≈ 50% parallelises across 3 agents: ≈ 60–110 h elapsed) | stubbed options in basic draw/modify commands, opening DWGs people send, launch/stability fixes, plot dialog, context menus and dynamic input |

Hours are subsets (essentials ⊂ mainstream ⊂ full) and use the calibration in [target-app-parity.md](docs/target-app-parity.md#remaining-effort-and-how-it-was-calibrated): ≈ 25 commands
per agent-hour at presence quality in the first build (≈ 11.5 agent-hours), 0.3–0.7 h per option
or bug fix (≈ 60 such fixes on 2026-10-10), 2–5 h per moderate feature, 15–100 h per subsystem.

Hours are Opus 5.5 agent wall-clock hours, calibrated on this repo's own build: the first version
(51k lines, 290 commands) took ≈ 11.5 agent-hours at presence quality, while depth work runs at
0.3–0.7 h per option or bug fix (≈ 60 such fixes landed on 2026-10-10). Details:
[target-app-parity.md](docs/target-app-parity.md#remaining-effort-and-how-it-was-calibrated).

## By dimension

| Dimension | Parity | Remaining (h) | Doc |
|---|---:|---|---|
| Features (depth, weighted by use) | 41% | 735–1,115 | [target-app-parity.md](docs/target-app-parity.md#by-feature-area-the-features-dimension) |
| UI/UX fidelity | 43% | 100–160 | [ui-parity.md](docs/ui-parity.md) |
| File formats | 30% | 170–280 | [file-format-parity.md](docs/file-format-parity.md) |
| Hardware | 45% | 15–25 | [hardware-parity.md](docs/hardware-parity.md) |
| Localization | 5% | 60–100 | [localization-parity.md](docs/localization-parity.md) |
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

AutoCAD 2027 for Mac ships 8 interface languages; CADCraft ships English plus eighteen catalogs
(Ukrainian, Brazilian Portuguese, German, Spanish, Czech, Russian, French, Simplified Chinese, Italian, Korean, Swedish, Arabic, Greek, Hungarian, Turkish, Bosnian, Georgian and Vietnamese). Detail: [localization-parity.md](docs/localization-parity.md).

| Language | Status | UI translated |
|---|---|---:|
| English | full | 100% |
| Simplified Chinese | partial | 1,337 catalog rows |
| Spanish | partial | 1,243 catalog rows |
| Hindi | none | 0% |
| Arabic | partial | 1,229 catalog rows |
| French | partial | 1,004 catalog rows |
| Portuguese (Brazilian) | partial | 1,243 catalog rows |
| Indonesian | none | 0% |
| Japanese | none | 0% |
| Greek | partial | 1,330 catalog rows; independent proofreading pending |
| German | partial | 1,031 catalog rows |
| Korean | partial | 1,235 catalog rows |
| Vietnamese | partial | 1,460 catalog rows (2026-11-07); native review pending |

Other languages shipped: Ukrainian (1,357 rows, the reference catalog, #36), Czech (1,215), Russian (1,005), Italian (1,352), Swedish (1,382), Hungarian (1,326) and Turkish (1,330), counted 2026-10-24; engine messages stay English. Every catalog but Ukrainian's has strings added since it landed that still fall back to English: 6 (Italian, Swedish) to 296 (French) per `cargo xtask i18n` ([localization-parity.md](docs/localization-parity.md)).

## Upcoming

Ranked in three tiers; a higher tier goes first whenever it has work. Within a tier, a fix that is
already written and waiting in a pull request lands before new work starts, whoever wrote it. Detail
and the beta checklist are in [docs/roadmap.md](docs/roadmap.md).

**1. Urgent: security fixes, leaks, crashes and lost work.**
- The open findings of the security audit (#1919–#1935): check each against the input limits that
  landed on 2026-11-11, close what is fixed and fix the rest. A crafted handle that makes new entities
  overwrite existing ones (#1921) is fixed by #1276.
- A saved file that crashes AutoCAD 2021 (#3303); dimension text lost on save (#3299) was fixed on 2026-11-20.
- Crashes and hangs with a reproduction: the web build on a large DXF (#1603), a DWG with 3D content
  (#2174), startup on older graphics hardware (#1724, #1631, #3222).

**2. Parity with AutoCAD: the alpha gate rows, then the beta checklist (apart from its items named under tier 3).**
- It starts on common hardware: an automatic fallback renderer (`CADCRAFT_STENCIL=0` is the manual
  switch since #3336).
- It opens the DWG and DXF files people already have: large files (#3102, #2633, #1088), layouts and
  paper units (#1672, #2836), and a corpus of real files run in CI.
- Saving loses nothing and other CAD programs open the result (#2904, #1687, #3234, #2632).
- Input feels like AutoCAD for Windows: the fixes for clicks while the mouse moves, right-click and
  Space as Enter, and double-click text editing landed (#3307, #3345); waiting for the reporters to
  confirm, and table cells are left (#842).
- The prompt options that still say "not available yet" (gaps F1).
- Layouts print at scale on a system printer (#3216); the menu bar and panels can be restored since
  #3308.
- 602 pull requests are open (2026-11-25; 661 on 2026-11-20). About 230 were mergeable, passed CI and were small; they are
  reviewed and landed in this order, the longest-waiting authors first within a row.

**3. Lower priority: affects fewer people and nothing breaks without it.**
- Dynamic-block authoring, editing an xref in place, exact ellipse and spline geometry, correct
  polyline offsets.
- Prompt options that need 3D (DVIEW's camera options, -RENAME Material); 3D itself comes after 1.0,
  but a file with 3D content must open safely (tier 1).
- Menu breadth against AutoCAD for Mac as a headline number.

Done from 2026-11-21 to 2026-11-25: PDFIMPORT, a PDF page as editable lines, polylines, solid hatches and text (#3375 in #3419); dynamic block references that rotate, flip, array, scale, stretch polar, align and look up, with their grips (#963, #993, #1030, #1098, #1699, #1723, #1752 in #3424; authoring is still open); the proxy graphics of other applications' objects drawn (#2697, #3435, #3437) and dimensions kept as read drawn from their `*D` block (#3439); saves that keep layout plot settings (#3233, #3386, #3390), DXF 2007+ table cell text (#3392), hatch boundary flags (#3389), fit text (#3391) and registered applications (#2918); the Array window for ARRAYRECT and ARRAYPOLAR (#3394); ATTDIA and the Edit Attributes dialog on INSERT (#3411) and constant attribute values drawn (#3462); Edit > Paste from the system clipboard (#3366, #3460, #3461; #3320 open); opening from Finder and quit prompts on macOS (#1347 in #3344, not yet run on a Mac); an already-open drawing selects its tab when opened from the interface (#899 in #3422; the control channel, MCP, scripts and LISP still open a second copy); Tab through every object snap candidate (#1673); the Write Block dialog for WBLOCK (#1778); Generate Boundary (#2804); mitred wide polylines (#3453; the extents leave out the mitre tip, #3443); DIMASSOC 1 (#2977); crossing selection of block references by what they draw (#3445); an option to turn off the update check (#3097).

Done on 2026-11-19 and 2026-11-20: the hostile-input fixes #1276, #2820, #2821 and #2765 (#3304, #3305), clicks while the mouse moves and right-click or Space as Enter (#3307), the interface reset (#3308), paper units (#3309), the UCS command (#3310), -ATTEDIT, -RENAME and -LAYER options (#3311), TRIM/EXTEND depth (#3312), dimension values kept on save (#3335), the stencil switch (#3336), a typed OPEN path (#3343) and double-click text editing (#3345).

Done before that: precision input (#379, #441, #60, #519), prompt options in TRIM/EXTEND,
OFFSET, ROTATE, PLINE, SPLINE, MTEXT and MLEADER (#397, #408, #547), autosave, recovery, AUDIT and
RECOVER on the desktop (#406, #416), xrefs (#525), groups (#417), the Block Editor (#824, #833, #885),
REFEDIT (#818), XCLIP (#2354), dynamic block references (#838–#883), and the Dimension Style Manager
(#1460).

## Progress log

| Date | Entry |
|---|---|
| 2026-10-21 | DXF save keeps each proxy object's and proxy entity's class: its class number (group 91) is renumbered to the rewritten CLASSES instead of naming another class, so AutoCAD's visual styles in an R14 file no longer come back as SCALE or MLEADERSTYLE (#1812) |
| 2026-10-14 | HATCHEDIT adds boundaries from internal points or closed objects and removes selected loops in the dialog, command line and JSON command; associative links and undo are covered by tests. |
| 2026-10-12 | TRIM: a pick after a closed polyline's last cut keeps only the complementary run instead of an extra circuit of the outline (#726) |
| 2026-10-12 | HATCHEDIT refuses an explicit bad `scale` or `angle` before it edits anything, so a rejected request leaves the drawing, undo and redo as they were (#724) |
| 2026-10-10 | Merged main: Ukrainian interface (#36, first string catalog), Dynamic Input pointer boxes with Tab and relative entry (#60), press-drag selection windows (#63), WIPEOUT and hatch gradients kept through DXF/DWG save (#147, #162), SETVAR DIM* as style overrides (#129), UNITS dialog (#38), close-window save prompt (#47), DIMANGULAR on circles (#20), cargo fmt fixed on main. UI/UX 40% → 43% (dynamic input 15% → 45%), localization 3% → 5%; ready for real work ≈ 37% → 38% (weighted 37.2% → 37.7%), mainstream 27% → 28%, essentials 45% → 46%. Beta is now ~37 points away; hours unchanged within rounding. |
| 2026-10-10 | Added mainstream practitioner (≈ 27%) and essentials user (≈ 45%) numbers; ready for real work unchanged at ≈ 37%. User evidence: 2 praise comments, 0 "switched from AutoCAD" reports, 27 of 37 open issues on the core path. |
| 2026-10-10 | Stage re-normalized from alpha to **pre-alpha** under the core-workflow gate: drafting with tracking, blocks, plotting and lossless DWG save fail; dimensioning passes. Alpha is ≈ 125–190 h away. |
| 2026-10-10 | Full re-measure against AutoCAD 2027 for Mac: menu breadth 245/491 (50%), ready for real work ≈ 37%, stage alpha. Remaining estimates rose (≈ 570 h → 1,300–2,100 h) on new evidence: behaviour audits found tracking/extension/parallel snaps and dynamic input are not real, saves drop unmodelled content, DWG saves as R2000, and the old estimate had no localization, ecosystem, stability, platform or AI rows. New docs: target-app-parity, gaps, roadmap, architecture, localization, file-format, hardware, UI and geometry parity; `docs/parity.md` became `docs/parity-checklist.md`. |
| 2026-10-10 | About 60 community PRs merged in a day: DXF fidelity (transparency, LWPOLYLINE elevation, frozen VP layers, dimension text rotation, arc-length dims, non-ASCII escapes), CJK font fallback, Save/Don't Save on close, Light/System themes, F2 history, trackpad pan, Linux XWayland drag-and-drop, correctness fixes across draw/modify/constraints. Release v0.4.0. |
| 2026-10-08 | Releases v0.1.0–v0.3.0: Flatpak, AppImage updates, signed macOS universal, Windows x64/x86/arm64, Linux, FreeBSD and web builds. |
| 2026-10-07 | Old assessment: menu breadth 233/491 (47%), ≈ 29% weighted parity, 2D drafting ≈ 55%, ≈ 570 h remaining; milestones as reported then: M1 ~85%, M2 ~65%, M3 ~75%, M4 ~50%, M5 ~35%, M6 ~55%, M7 ~55%, M8 ~20%, M9 ~60%, M10 ~70%, M11 0%, M12 ~25%, M13 ~10%. |
| 2026-10-07 | First version built in one session (≈ 6.9 h elapsed, ≈ 11.5 agent-hours): M0 vertical slice, dimensions, hatch, blocks, DWG bridge, layouts and PDF plotting, GPU canvas, R-tree, constraints, Layer Properties Manager, QSELECT, release pipeline. |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-25 | minor | Upcoming updated for the merges of 2026-11-21 to 2026-11-25 (PDF import, dynamic block reference actions, proxy graphics, what saves keep, Array window, ATTDIA, system clipboard paste, macOS opening from Finder, WBLOCK dialog, Generate Boundary); #3299 fixed; pull-request count 661 → 602; percentages and hours not re-scored |
| 2026-11-20 | minor | Upcoming updated for the merges of 2026-11-19 and 2026-11-20 (security stacks, input, interface reset, paper units, UCS, prompt options, TRIM/EXTEND, text editing in place); pull-request count 714 → 661; percentages and hours not re-scored |
| 2026-11-19 | major | Alpha gate revised from user reports (six rows, Windows first; block authoring, xref editing in place and exact curve geometry moved to beta); Upcoming re-ranked into three tiers: urgent (security, leaks, crashes, lost work), parity, lower priority; percentages and hours not re-scored |
| 2026-10-24 | minor | Re-audit against main 41d9b5e95: language table, CI gate and open-PR count re-checked against main 41d9b5e95 |
| 2026-10-21 | trivial | Progress log: proxy objects keep their class through a save (#1812) |
| 2026-10-14 | minor | Progress log: HATCHEDIT boundary add/remove |
| 2026-10-12 | trivial | Progress log: closed-polyline TRIM (#726) |
| 2026-10-12 | trivial | Progress log: HATCHEDIT numeric validation (#724) |
| 2026-10-10 | minor | Merged main's changes (Ukrainian catalog, dynamic input pointer boxes, DXF fixes); UI/UX and localization numbers updated |
| 2026-10-10 | minor | Readiness-by-audience table with hours to ~95% for each number |
| 2026-10-10 | minor | Mainstream practitioner and essentials user headline numbers |
| 2026-10-10 | minor | Stage alpha → pre-alpha under the core-workflow gate; banner, why, headline row |
| 2026-10-10 | major | Progress docs restructured (stage, two numbers, dimensions, features, languages, upcoming, log); full re-measure; milestone detail moved to docs/roadmap.md, parity assessment to docs/target-app-parity.md |
| 2026-10-07 | major | Alpha checklist, milestone table, ≈ 29% weighted parity estimate |

## Shared UI workspace integration

Tool Sets, Layers and Properties use shared docking with persisted layouts, keyboard focus and compact toolbar overflow. No milestone percentage change.
