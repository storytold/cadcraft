# Localization parity

> **Last reviewed:** 2026-10-24 · **Last updated:** 2026-10-24 · **Change:** minor (catalog row counts and English-fallback counts re-measured for all fifteen catalogs; previously: Brazilian Portuguese row updated and terminology reviewed; previously: Swedish catalog completed with the 95 strings added since; previously: Swedish catalog added: complete catalog, menu headings, native-speaker terminology review; previously: complete current Korean catalog with Hangul font coverage) · **Target:** Autodesk AutoCAD 2027 for Mac (26.0)

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
  and plurals ([localization.md](localization.md)). The fifteen catalogs have between 1,004
  (French) and 1,382 rows (Swedish; Ukrainian, the reference, has 1,357), counted 2026-10-24,
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
  scripts, and **no vertical text**. SHX fonts are not read; our stroke font stands in, and wide East Asian characters in it fill the square cells a big font would give them (#861).
- **Text input:** egui/winit IME. The upstream IME fixes (macOS Korean, emoji picker, Windows
  caret placement, X11 and Wayland) were proposed as PRs #110–#119, which were closed in favour
  of the consolidated, still open PR #668, so CJK input is not yet reliable on every platform.

## Languages

Order and columns from craftrules' progress-docs standard. Every UI percentage is **measured**
from the catalogs (0 rows for every language without a catalog file). Hours are **estimated**:
about 3–5 h per language for an Opus 5.5 agent to translate the ~1,300 catalog rows and fix layout,
plus native review by a human; extending the catalog to the remaining English-only strings
(engine diagnostics, uncatalogued text) is about 4–6 h, counted once in the first row.

| Language | Code | UI strings translated | Dialogs / tooltips / help | Script support | Native review | Status | Estimate to `full` |
|---|---|---|---|---|---|---|---|
| English | en | source (100%) | 100% (help is a command reference, not AutoCAD-sized help) | Latin: yes | n/a | full | — |
| Simplified Chinese | zh-Hans | 1,337 catalog rows; 84 strings fall back to English | partial | CJK glyphs render via the CJK interface fonts loaded in every language (#861); IME fixes pending (#668); no vertical text | contributed by @mayloglog (#684); no native review recorded | partial | 2–4 h + native review |
| Spanish | es | 1,243 catalog rows; 115 strings fall back to English | dialogs and tooltips yes | Latin: yes | contributed by @PedroGuille (#610) and extended by @diegozpy (#680); no native review recorded | partial | 1–2 h |
| Hindi | hi | 0 (0%) | 0% | Devanagari: no shaping | no | none | 3–5 h + 15–25 h shaping (shared with Arabic) |
| Arabic | ar | 1,229 catalog rows; 124 strings fall back to English | partial | Drawing text shaped and laid out right to left (#287); interface fonts for Arabic on Linux, Windows and macOS; the UI is not mirrored (reading labels right to left is open PR #996) | contributed by @hasan-sdaif (#798); no native review recorded | partial | 1–2 h + 15–25 h RTL UI |
| French | fr | 1,004 catalog rows; 296 strings fall back to English | partial | Latin: yes | contributed by @pechnikov2026-star (#624); no native review yet | partial | 2–4 h |
| Russian | ru | 1,005 catalog rows; 295 strings fall back to English | partial | Cyrillic: yes (bundled egui fonts) | contributed by @pechnikov2026-star (#624); no native review yet | partial | 2–4 h |
| Portuguese (Brazil) | pt-BR | 1,243 catalog rows; 112 strings added since fall back to English (covered the reference catalog and every marked UI literal when it landed) | dialogs and tooltips yes; command reference searchable in Portuguese | Latin: yes (bundled egui fonts; accented letters tested) | catalog by @WladmirJunior, completed with AI assistance by @RicSchonfelder; terminology reviewed by a native speaker (@ffishman) | complete when it landed; 112 strings added since fall back to English | line-by-line proofreading + shared English-only UI work |
| Indonesian | id | 0 (0%) | 0% | Latin: yes | no | none | 3–5 h |
| Japanese | ja | 0 (0%) | 0% | CJK glyphs render; IME pending; no vertical text | no | none | 4–6 h |
| German | de | 1,031 catalog rows; 284 strings fall back to English | dialogs and tooltips yes; command reference searchable in German | Latin: yes (bundled egui fonts cover ä ö ü Ä Ö Ü ß; long strings will need layout checks) | no native review | partial | 1–2 h |
| Greek | el | 1,330 catalog rows; 34 strings fall back to English | Menus, dialogs, tooltips, drawing prompts, command labels and contextual headings; English fallback for uncatalogued UI | Greek script; bundled egui glyph check added | Original AI-assisted translation by @Camthalion77, independent native review pending | complete when it landed; 34 strings added since fall back to English | native proofreading and platform layout review |
| Korean | ko | 1,235 catalog rows; 124 strings fall back to English | dialogs, tooltips, Options and translated command-reference labels | UI requires catalog-covering installed or optional craft-fonts Hang/Kore face; English fallback without one; IME fixes still pending | original AI-assisted translation; independent native proofreading pending | complete when it landed; 124 strings added since fall back to English | native review + platform layout/IME validation |
| Vietnamese | vi | 0 (0%) | 0% | Latin with stacked diacritics: renders per code point, untested | no | none | 3–5 h |
| Ukrainian | uk | 1,357 catalog rows; nothing falls back to English (the reference catalog) | dialogs and tooltips yes; command reference searchable in Ukrainian | Cyrillic: yes (bundled egui fonts) | translated by a contributor; no second review | partial | 2–4 h |
| Czech | cs | 1,215 catalog rows; 128 strings added since fall back to English (covered the reference catalog and every marked UI literal when it landed) | dialogs and tooltips yes; contextual menu headings; command reference searchable in Czech | Latin: yes (bundled egui proportional and monospace fonts; Czech letters and quotes tested) | AI-assisted by @Losminek; independent review pass; not yet proofread by a native speaker | complete when it landed; 128 strings added since fall back to English | native review + shared English-only UI work |
| Swedish | sv | 1,382 catalog rows; 6 strings added since fall back to English (covered the reference catalog and every marked UI literal when it landed) | dialogs and tooltips yes; contextual menu headings; command reference searchable in Swedish | Latin: yes (bundled egui proportional and monospace fonts; å ä ö é and Swedish quotes tested) | AI-assisted (Claude Code) by @holmbom; terminology and the running app reviewed by a native speaker | complete when it landed; 6 strings added since fall back to English | line-by-line proofreading + shared English-only UI work |
| Italian | it | 1,352 catalog rows; 6 strings added since fall back to English (covered the reference catalog and every marked UI literal when it landed) | dialogs and tooltips yes; contextual menu headings (Visualizza / Vista, Edita / Modifica); command reference searchable in Italian | Latin: yes (bundled egui fonts; accented letters tested) | AI-assisted by Emanuele Bottelli (Claude), written from the English strings with Italian desktop-CAD vocabulary; no native proofreading pass yet | complete when it landed; 6 strings added since fall back to English | native review + shared English-only UI work |
| Hungarian | hu | 1,326 catalog rows; 34 strings added since fall back to English (covered the reference catalog and every marked UI literal when it landed) | dialogs and tooltips yes; contextual menu headings; command reference searchable in Hungarian | Latin: yes (bundled egui proportional and monospace fonts; ő ű Ő Ű and the other accented letters tested) | translated by @Skaywalker from the English strings; independent native proofreading pending | complete when it landed; 34 strings added since fall back to English | native proofreading + shared English-only UI work |
| Turkish | tr | 1,330 catalog rows; 34 strings added since fall back to English (covered the reference catalog and every marked UI literal when it landed) | dialogs and tooltips yes; contextual menu headings (Ölçülendirme / Ölçü); command reference searchable in Turkish | Latin: yes (bundled egui proportional and monospace fonts; Ç Ğ İ Ö Ş Ü ç ğ ı ö ş ü tested) | AI-assisted by @ogzhnklvz, written from the English strings with Turkish desktop-CAD vocabulary; the running app reviewed by a native speaker | complete when it landed; 34 strings added since fall back to English | line-by-line proofreading + shared English-only UI work |

Languages shipped (`LANGUAGES` in `crates/ui-egui/src/i18n/mod.rs`): Ukrainian, Brazilian Portuguese, German, Spanish, Czech,
Russian, French, Simplified Chinese, Italian, Korean, Swedish, Arabic, Greek, Hungarian and Turkish, fifteen catalogs. Row counts are the
non-comment lines of each `.tsv`; "fall back to English" is the count `cargo xtask i18n` prints, both measured 2026-10-24. Engine
diagnostics and uncatalogued text remain English. AutoCAD for Mac ships 7 non-English languages (zh-Hans, es, fr, ja, de, ko, it); CADCraft
ships six of them (all but Japanese), and additionally Ukrainian, Brazilian Portuguese, Czech, Russian, Swedish, Arabic, Greek, Hungarian
and Turkish, which AutoCAD for Mac doesn't ship.

**Total to put the languages at `full`:** about 62–102 Opus 5.5 hours, summed from the table's estimates (catalog extension 4–6 h,
four languages not started 13–21 h, closing the gaps of the fourteen lagging catalogs 15–30 h,
complex-script shaping and RTL 30–45 h), plus native-speaker review for
each language, which needs humans. Translations parallelise fully once the catalog exists.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-24 | minor | Re-audit against main 41d9b5e95: catalog row counts and English-fallback counts re-measured for all fifteen catalogs (main 41d9b5e95) |
| 2026-10-19 | minor | Brazilian Portuguese: row brought up to date (it still read 0%); native-speaker terminology pass on 69 rows (Bind as Integrar, prompt keywords and SPDX kept in English, Thaw Layer, Snap type, Revolve fixed; snap, viewport, grip, offset, layer on/off and multiline wording made consistent with Brazilian usage) |
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
