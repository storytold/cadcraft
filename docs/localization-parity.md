# Localization parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** minor (Ukrainian catalog landed, #36: first string catalog) · **Target:** Autodesk AutoCAD 2027 for Mac (26.0)

How far CADCraft's interface and drawing text are from AutoCAD's languages. Summarised in
[ROADMAP.md](../ROADMAP.md#languages); work items in [gaps.md](gaps.md#localization).

## What the target ships

Measured from the installed AutoCAD 2027 for Mac bundle by listing only (folder names, nothing
read): `Contents/Resources` has `Base.lproj` (English), `de`, `es`, `fr`, `it`, `ja`, `ko` and
`zh_CN` localizations, and `Resources/Support` has matching `@de@` … `@zh_CN@` folders. That is
**8 interface languages** on the Mac. AutoCAD for Windows also has language packs for Traditional
Chinese, Brazilian Portuguese, Russian, Polish, Czech and Hungarian (Autodesk's published
language-pack list, not measured here). AutoCAD's spell checker ships about 20 dictionaries
(`*.dct` names in `Resources/Support`).

Drawing text in AutoCAD: TrueType and SHX big fonts (CJK), Unicode `\U+` escapes, complex-script
shaping and right-to-left text through TrueType, vertical SHX text styles.

## What CADCraft has

- **A display-time string catalog** (`crates/ui-egui/src/i18n/`, landed with #36): a TSV per
  language (context, English source, translation, `@plural` rows), Edit → Interface Language with
  Auto (system language), live switching, a saved choice, and tests for coverage, placeholders
  and plurals ([localization.md](localization.md)). The Ukrainian catalog has **936 rows**,
  covering menus, command labels, tool groups and tooltips, the Start page, tabs, status
  controls, object snaps, drafting prompts, property labels and dialogs. Engine diagnostics,
  CLI/MCP responses and command history stay English, and uncatalogued text falls back to
  English. For scale: about 1,160 unique capitalised string literals exist in `crates/engine`
  and `crates/ui-egui` (grep, 2026-10-10).
- **Drawing text:** TrueType/OpenType/`.ttc` through `skrifa` with per-character font fallback
  and FONTALT, so CJK characters render (#173). DXF text is written with `\U+` escapes so
  non-ASCII survives in R2000 files (#169). Glyphs are placed one per code point: there is **no
  shaping** (Arabic joining, Devanagari conjuncts), **no bidi** (right-to-left order) and **no
  vertical text**. SHX fonts are not read; our stroke font stands in.
- **Text input:** egui/winit IME. Several upstream IME fixes are queued as PRs (#110–#119:
  macOS Korean, emoji picker, Windows caret placement, X11 and Wayland), so CJK input is not yet
  reliable on every platform.

## Languages

Order and columns from craftrules' progress-docs standard. Every UI percentage is **measured**
from the catalogs (0 rows for every language without a catalog file). Hours are **estimated**:
about 3–5 h per language for an Opus 5.5 agent to translate the ~940 catalog rows and fix layout,
plus native review by a human; extending the catalog to the remaining English-only strings
(engine diagnostics, uncatalogued text) is about 4–6 h, counted once in the first row.

| Language | Code | UI strings translated | Dialogs / tooltips / help | Script support | Native review | Status | Estimate to `full` |
|---|---|---|---|---|---|---|---|
| English | en | source (100%) | 100% (help is a command reference, not AutoCAD-sized help) | Latin: yes | n/a | full | — |
| Simplified Chinese | zh-Hans | 0 (0%) | 0% | CJK glyphs render via fallback; IME fixes pending (#110–#119); no vertical text | no | none | 8–12 h (includes extending the catalog, 4–6 h) |
| Spanish | es | 0 (0%) | 0% | Latin: yes | no | none | 3–5 h |
| Hindi | hi | 0 (0%) | 0% | Devanagari: no shaping | no | none | 3–5 h + 15–25 h shaping (shared with Arabic) |
| Arabic | ar | 0 (0%) | 0% | No shaping, no RTL layout or bidi text | no | none | 3–5 h + shaping + 15–25 h RTL UI |
| French | fr | 0 (0%) | 0% | Latin: yes | no | none | 3–5 h |
| Portuguese | pt | 0 (0%) | 0% | Latin: yes | no | none | 3–5 h |
| Indonesian | id | 0 (0%) | 0% | Latin: yes | no | none | 3–5 h |
| Japanese | ja | 0 (0%) | 0% | CJK glyphs render; IME pending; no vertical text | no | none | 4–6 h |
| German | de | 0 (0%) | 0% | Latin: yes (long strings will need layout checks) | no | none | 3–5 h |
| Korean | ko | 0 (0%) | 0% | Hangul renders; macOS Korean IME fixes pending (#118, #119) | no | none | 4–6 h |
| Vietnamese | vi | 0 (0%) | 0% | Latin with stacked diacritics: renders per code point, untested | no | none | 3–5 h |
| Ukrainian | uk | 936 catalog rows (≈ 100% of the catalog; ≈ 80% of all UI strings, estimated: engine diagnostics and uncatalogued text stay English) | dialogs and tooltips yes; command reference searchable in Ukrainian | Cyrillic: yes (bundled egui fonts) | translated by a contributor; no second review | partial | 2–4 h |

Other languages shipped: Ukrainian (partial). AutoCAD for Mac ships 7 non-English languages
(zh-Hans, es, fr, ja, de, ko, it); CADCraft ships none of those, and Ukrainian, which AutoCAD
for Mac doesn't ship.

**Total to put all twelve at `full`:** about 60–100 Opus 5.5 hours (catalog extension 4–6 h,
eleven translations 35–55 h, complex-script shaping and RTL 30–45 h), plus native-speaker review for
each language, which needs humans. Translations parallelise fully once the catalog exists.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | minor | Ukrainian catalog landed on main (#36): catalog infrastructure exists, Ukrainian partial; total hours 70–110 → 60–100 |
| 2026-10-10 | major | First localization measurement against AutoCAD 2027 for Mac's bundle localizations |
