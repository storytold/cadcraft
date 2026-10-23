# Localization parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-19 · **Change:** minor (Finnish catalog added; previously: Swedish catalog completed with the 95 strings added since; previously: Swedish catalog added: complete catalog, menu headings, native-speaker terminology review; previously: complete current Korean catalog with Hangul font coverage) · **Target:** Autodesk AutoCAD 2027 for Mac (26.0)

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
  and plurals ([localization.md](localization.md)). The Ukrainian and German catalogs have
  **989 rows** each,
  covering menus, command labels, tool groups and tooltips, the Start page, tabs, status
  controls, object snaps, drafting prompts, property labels and dialogs. Engine diagnostics,
  CLI/MCP responses and command history stay English, and uncatalogued text falls back to
  English. For scale: about 1,160 unique capitalised string literals exist in `crates/engine`
  and `crates/ui-egui` (grep, 2026-10-10).
- **Drawing text:** TrueType/OpenType/`.ttc` through `skrifa` with per-character font fallback
  and FONTALT, so CJK characters render (#173). DXF text is written with `\U+` escapes so
  non-ASCII survives in R2000 files (#169). Lines with right-to-left text get the Unicode
  Bidirectional Algorithm and OpenType shaping with `harfrust` (Arabic, Persian and Urdu joining,
  lam-alef, ZWNJ, marks; Hebrew), in TEXT and MTEXT including wrapping (#287). Left-to-right lines
  are still placed one glyph per code point: **no shaping** for Devanagari and other Indic
  scripts, and **no vertical text**. SHX fonts are not read; our stroke font stands in.
- **Text input:** egui/winit IME. Several upstream IME fixes are queued as PRs (#110–#119:
  macOS Korean, emoji picker, Windows caret placement, X11 and Wayland), so CJK input is not yet
  reliable on every platform.

## Languages

Order and columns from craftrules' progress-docs standard. Every UI percentage is **measured**
from the catalogs (0 rows for every language without a catalog file). Hours are **estimated**:
about 3–5 h per language for an Opus 5.5 agent to translate the ~990 catalog rows and fix layout,
plus native review by a human; extending the catalog to the remaining English-only strings
(engine diagnostics, uncatalogued text) is about 4–6 h, counted once in the first row.

| Language | Code | UI strings translated | Dialogs / tooltips / help | Script support | Native review | Status | Estimate to `full` |
|---|---|---|---|---|---|---|---|
| English | en | source (100%) | 100% (help is a command reference, not AutoCAD-sized help) | Latin: yes | n/a | full | — |
| Simplified Chinese | zh-Hans | 0 (0%) | 0% | CJK glyphs render via fallback; IME fixes pending (#110–#119); no vertical text | no | none | 8–12 h (includes extending the catalog, 4–6 h) |
| Spanish | es | 0 (0%) | 0% | Latin: yes | no | none | 3–5 h |
| Hindi | hi | 0 (0%) | 0% | Devanagari: no shaping | no | none | 3–5 h + 15–25 h shaping (shared with Arabic) |
| Arabic | ar | 0 (0%) | 0% | Drawing text shaped and laid out right to left (#287); the UI is not mirrored | no | none | 3–5 h + 15–25 h RTL UI |
| French | fr | ≈ 900 catalog rows (gaps reported, English fallback) | partial | Latin: yes | contributed by @pechnikov2026-star (#624); no native review yet | partial | 2–4 h |
| Russian | ru | ≈ 900 catalog rows (gaps reported, English fallback) | partial | Cyrillic: yes (bundled egui fonts) | contributed by @pechnikov2026-star (#624); no native review yet | partial | 2–4 h |
| Portuguese | pt | 0 (0%) | 0% | Latin: yes | no | none | 3–5 h |
| Indonesian | id | 0 (0%) | 0% | Latin: yes | no | none | 3–5 h |
| Japanese | ja | 0 (0%) | 0% | CJK glyphs render; IME pending; no vertical text | no | none | 4–6 h |
| German | de | 989 catalog rows (≈ 100% of the catalog; ≈ 80% of all UI strings, estimated: engine diagnostics and uncatalogued text stay English) | dialogs and tooltips yes; command reference searchable in German | Latin: yes (bundled egui fonts cover ä ö ü Ä Ö Ü ß; long strings will need layout checks) | no native review | partial | 1–2 h |
| Greek | el | 1,330 catalog rows (all collected existing source keys) | Menus, dialogs, tooltips, drawing prompts, command labels and contextual headings; English fallback for uncatalogued UI | Greek script; bundled egui glyph check added | Original AI-assisted translation by @Camthalion77, independent native review pending | complete current catalog; uncatalogued UI remains English | native proofreading and platform layout review |
| Korean | ko | 1,235 rows (1,229 ordinary, 4 menu, 2 count); full existing catalog union and marked UI coverage | dialogs, tooltips, Options and translated command-reference labels | UI requires catalog-covering installed or optional craft-fonts Hang/Kore face; English fallback without one; IME fixes still pending | original AI-assisted translation; independent native proofreading pending | complete current catalog; English-only interface gaps remain | native review + platform layout/IME validation |
| Vietnamese | vi | 0 (0%) | 0% | Latin with stacked diacritics: renders per code point, untested | no | none | 3–5 h |
| Ukrainian | uk | 989 catalog rows (≈ 100% of the catalog; ≈ 80% of all UI strings, estimated: engine diagnostics and uncatalogued text stay English) | dialogs and tooltips yes; command reference searchable in Ukrainian | Cyrillic: yes (bundled egui fonts) | translated by a contributor; no second review | partial | 2–4 h |
| Czech | cs | 1,202 catalog rows; reference catalog and marked UI literals covered | dialogs and tooltips yes; contextual menu headings; command reference searchable in Czech | Latin: yes (bundled egui proportional and monospace fonts; Czech letters and quotes tested) | AI-assisted by @Losminek; independent review pass; not yet proofread by a native speaker | complete current catalog; English-only interface gaps remain | native review + shared English-only UI work |
| Swedish | sv | 1,330 catalog rows; reference catalog, marked UI literals and menu headings covered (no gaps reported) | dialogs and tooltips yes; contextual menu headings; command reference searchable in Swedish | Latin: yes (bundled egui proportional and monospace fonts; å ä ö é and Swedish quotes tested) | AI-assisted (Claude Code) by @holmbom; terminology and the running app reviewed by a native speaker | complete current catalog; English-only interface gaps remain | line-by-line proofreading + shared English-only UI work |
| Finnish | fi | 1,384 catalog rows (union of existing catalogs, 4 menu headings, 2 count templates) | dialogs and tooltips yes; contextual menu headings (Muokkaa / Muuta, Mitoitus / Mitta); command reference searchable in Finnish | Latin: yes (bundled egui fonts; ä ö å š ž and quotes tested) | AI-assisted (Claude Code) by @Janiz-zzz; native line-by-line proofreading pending | complete current catalog; English-only interface gaps remain | native review + shared English-only UI work |
| Italian | it | 1,227 catalog rows; reference catalog and marked UI literals covered | dialogs and tooltips yes; contextual menu headings (Visualizza / Vista, Edita / Modifica); command reference searchable in Italian | Latin: yes (bundled egui fonts; accented letters tested) | AI-assisted by Emanuele Bottelli (Claude), written from the English strings with Italian desktop-CAD vocabulary; no native proofreading pass yet | complete current catalog; English-only interface gaps remain | native review + shared English-only UI work |
| Hungarian | hu | 1,326 catalog rows; reference catalog, marked UI literals and menu headings covered (no gaps reported) | dialogs and tooltips yes; contextual menu headings; command reference searchable in Hungarian | Latin: yes (bundled egui proportional and monospace fonts; ő ű Ő Ű and the other accented letters tested) | translated by @Skaywalker from the English strings; independent native proofreading pending | complete current catalog; English-only interface gaps remain | native proofreading + shared English-only UI work |
| Turkish | tr | 1,330 catalog rows; reference catalog, marked UI literals and menu headings covered (no gaps reported) | dialogs and tooltips yes; contextual menu headings (Ölçülendirme / Ölçü); command reference searchable in Turkish | Latin: yes (bundled egui proportional and monospace fonts; Ç Ğ İ Ö Ş Ü ç ğ ı ö ş ü tested) | AI-assisted by @ogzhnklvz, written from the English strings with Turkish desktop-CAD vocabulary; the running app reviewed by a native speaker | complete current catalog; English-only interface gaps remain | line-by-line proofreading + shared English-only UI work |

Other languages shipped: Ukrainian, Brazilian Portuguese, German, Czech, Russian, French, Italian, Korean, Swedish, Hungarian, Turkish and Finnish.
Czech, Swedish, Hungarian and Brazilian Portuguese cover the reference catalog and every marked UI literal; engine diagnostics and uncatalogued text
remain English. AutoCAD for Mac ships 7
non-English languages (zh-Hans, es, fr, ja, de, ko, it); of those, CADCraft now ships German and Korean,
and additionally Ukrainian, which AutoCAD for Mac doesn't ship.

**Total to put all twelve at `full`:** about 55–95 Opus 5.5 hours (catalog extension 4–6 h,
ten translations 30–50 h, complex-script shaping and RTL 30–45 h), plus native-speaker review for
each language, which needs humans. Translations parallelise fully once the catalog exists.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-19 | minor | Finnish added: 1,384 rows, one/other plurals, menu headings, locale/preferences/menu/font/render tests; AI-assisted, native proofreading pending |
| 2026-10-15 | minor | Greek (`el`) catalog: 1,330 rows, context/plural/locale/font tests; review pending |
| 2026-10-14 | minor | Swedish completed: 95 rows for strings added since (Hatch and Gradient dialog, Block Editor, REFEDIT, multileader editing, Options selection settings, MTEXT editor, recent files); 1,330 rows, every key of the Ukrainian reference catalog |
| 2026-10-18 | minor | Turkish added: 1,330 rows (the existing catalog union), single count form, four menu headings, prompt keywords kept in English; locale/preferences/menu/live-switch/dialog/font tests; no gaps in the marked UI literals; AI-assisted, the running app reviewed by a native speaker |
| 2026-10-13 | minor | Hungarian added: 1,326 rows, a single count form, menu headings, locale/preferences/menu/font/render tests; no gaps in the marked UI literals; native proofreading pending |
| 2026-10-13 | minor | Swedish added: 1,235 rows, one/other plurals, menu headings, locale/preferences/menu/font/render tests; no gaps in the marked UI literals; terminology and the running app reviewed by a native speaker |
| 2026-10-13 | minor | Korean added: full 1,235-row existing catalog union, single count form, locale/persistence/live-switch tests, and validated Hangul UI fonts; native review and platform verification pending |
| 2026-10-12 | minor | Italian added: 1,227 rows, one/other plurals, two `menu` headings, locale/preference/plural/font tests; AI-assisted, native proofreading pending |
| 2026-10-12 | minor | Brazilian Portuguese catalog completed by @RicSchonfelder: 238 rows added (1,231 total), now covering the reference catalog and every marked UI literal, gaps 0; bundled-font glyph test for the catalog's accented letters |
| 2026-10-11 | minor | Czech added: 1,202 rows, one/few/other plurals, locale/preferences/menu/font/render tests; independent review pass; native proofreading pending |
| 2026-10-11 | minor | German catalog landed: `de` preference and catalog, per-language plural rules; non-English languages 1 → 2; total hours 60–100 → 55–95 |
| 2026-10-10 | minor | Right-to-left drawing text: bidi and Arabic-script shaping in TEXT and MTEXT (#287) |
| 2026-10-10 | minor | Ukrainian catalog landed on main (#36): catalog infrastructure exists, Ukrainian partial; total hours 70–110 → 60–100 |
| 2026-10-10 | major | First localization measurement against AutoCAD 2027 for Mac's bundle localizations |
