# Localization parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (first measurement: no string catalog exists yet) · **Target:** Autodesk AutoCAD 2027 for Mac (26.0)

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

- **No string catalog.** Every interface string is an English literal in Rust (about 1,160
  unique capitalised string literals in `crates/engine` and `crates/ui-egui`, measured with a
  grep on 2026-10-10; the real number of user-visible strings is in that range). There is no
  locale detection, no language setting and no translation pipeline.
- **In flight:** PR #36 adds the first catalog with a Ukrainian translation (issue #35). Its
  approach should become the catalog every language uses; once it lands, re-measure the table.
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
(0 strings translated, because no catalog exists). Hours are **estimated**: about 3–5 h per
language for an Opus 5.5 agent to translate ~1,200–2,000 strings and fix layout, plus native
review by a human; the one-off catalog extraction is counted once in the first row that needs it.

| Language | Code | UI strings translated | Dialogs / tooltips / help | Script support | Native review | Status | Estimate to `full` |
|---|---|---|---|---|---|---|---|
| English | en | source (100%) | 100% (help is a command reference, not AutoCAD-sized help) | Latin: yes | n/a | full | — |
| Simplified Chinese | zh-Hans | 0 (0%) | 0% | CJK glyphs render via fallback; IME fixes pending (#110–#119); no vertical text | no | none | 12–20 h (includes extracting the catalog, 8–12 h) |
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
| Ukrainian | uk | 0 (0%) on main; PR #36 open | 0% | Cyrillic: yes | no | none (in review) | 1–2 h after #36 |

Other languages shipped: none. AutoCAD for Mac ships 7 non-English languages (zh-Hans, es,
fr, ja, de, ko, it); CADCraft ships 0.

**Total to put all twelve at `full`:** about 70–110 Opus 5.5 hours (catalog 8–12 h, eleven
translations 35–55 h, complex-script shaping and RTL 30–45 h), plus native-speaker review for
each language, which needs humans. Translations parallelise fully once the catalog exists.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | First localization measurement against AutoCAD 2027 for Mac's bundle localizations |
