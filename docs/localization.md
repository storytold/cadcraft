# Interface languages

Choose **Edit → Interface Language** and select **Auto (system language)**, **English**,
**Українська**, **Português (Brasil)**, **Deutsch**, **Español**, **Čeština** (Czech), **Русский**, **Français**, **简体中文**, **Italiano**, **한국어** (Korean), **Svenska** (Swedish), **العربية** (Arabic), **Magyar** (Hungarian), **Türkçe** (Turkish), **Ελληνικά** (Greek), or **Suomi** (Finnish).
The interface updates without restarting, including the native macOS menus.
The choice is saved with the other interface preferences (`cadcraft.prefs`, beside the theme):
eframe storage on the desktop, browser local storage on the web. Installations without a saved
choice use Auto.

Auto uses the first supported language in the operating system's preferred locales, or the
browser's `navigator.languages` on the web. BCP 47 and POSIX tags select their primary language:
`uk-UA` → `uk`, `de_AT.UTF-8` → `de`, `es-MX` → `es`, `pt_BR.UTF-8` → `pt-BR`,
`cs-CZ` and `cs_CZ.UTF-8` → `cs`, `it-IT` and `it_CH.UTF-8` → `it`, `ko-KR` and `ko_KR.UTF-8` → `ko`,
`sv-SE`, `sv-FI` and `sv_SE.UTF-8` → `sv`, `hu-HU` and `hu_HU.UTF-8` → `hu`, `tr-TR` and `tr_TR.UTF-8` → `tr`, `fi-FI` and `fi_FI.UTF-8` → `fi`. Unsupported languages fall back to English.
Set `CADCRAFT_LOCALE=cs-CZ` to override the desktop's locale for a launch.
An explicit saved language choice takes precedence over that override.

Automation can select the language through the existing control channel:

```json
{"id":1,"method":"engine.execute","params":{"command":"ui.language","params":{"lang":"cs"}}}
```

The `ui.set` method also accepts `{"interfaceLanguage":"cs"}`. Supported values are `auto`,
`en`, `uk`, `pt-BR`, `de`, `es`, `cs`, `ru`, `fr`, `zh`, `it`, `ko`, `sv`, `ar`, `hu`, `tr`, `el`, and `fi`; invalid values are rejected without changing the current
selection. Resetting palettes keeps the language choice.

## Translation coverage

The catalogs cover command and menu labels, tool groups and tooltips, the Start page,
file and model-space tabs, status controls, object snaps, drafting prompts, property labels,
dialogs, and the About window's controls. The command reference can be searched by English
command ID, English label, or translated label. Ukrainian has enforced coverage;
missing entries in the other catalogs are reported by tests and fall back to English.

Czech has 1,202 catalog rows, covering the reference catalog and every marked UI
literal, including Options. Four `menu` entries distinguish menu headings from object/command
labels: **Zobrazení / Pohled**, **Kreslení / Kreslit**, **Kótování / Kóta**, and **Modifikace / Upravit**.
The translation is AI-assisted work by @Losminek, reviewed against Czech desktop and CAD UI
conventions and an independent review pass; it has not yet been proofread by a native speaker.

Korean has **1,235 rows**: 1,229 ordinary labels, four contextual menu headings, and
two single-form count templates. It covers the union of the existing catalogs, including
the Ukrainian reference, the current Czech catalog, Options, prompts, dialogs, and tooltips.
The translations are original, AI-assisted Korean CAD terminology (for example, 도면층,
치수, 모깎기, 모따기, 간격띄우기 and 객체 스냅). Independent native proofreading and
platform-specific layout/IME validation remain follow-up work.

Brazilian Portuguese has 1,231 catalog rows and also covers the reference catalog and every marked UI
literal: commands, menus, tool groups and tooltips, object snaps, drafting prompts, dialogs and
Preferences; its gaps are now zero. Its plural rule is `one|other` (0 and 1 are `one`), as CLDR defines
for `pt`, and the catalog's accented letters (`Á Â Í Ó Ú à á â ã ç é ê í ó ô õ ú`) render in the
bundled fonts. The original catalog was contributed by @WladmirJunior; the remaining rows were completed
by @RicSchonfelder with AI assistance and have not yet been proofread by a native speaker.

Hungarian has **1,326 rows**: 1,320 ordinary labels, four contextual menu headings, and two
single-form count templates. It covers the reference catalog and every marked UI literal,
including Options, prompts, dialogs, and tooltips; only **Méretezés / Méret** (Dimension) differs
between the menu heading and the object label. The interface addresses the user informally, and
a count takes one form because a Hungarian numeral is followed by the singular. The translation
was written from the meaning of CADCraft's English strings; no other product's localization was
used as a source. It has not yet been proofread by an independent native speaker.

Turkish has **1,330 rows**: the union of the existing catalogs, four contextual menu headings
(**Ölçülendirme** for the Dimension menu, **Ölçü** for the object) and two single-form count
templates, since a Turkish noun stays singular after a number ("3 commit"). No marked UI literal
falls back to English. Prompts read "… girin" for Enter and "… belirtin" for Specify, and clickable
keywords (`Top`, `Zero`, `Bottom`, `_tan to`, `nX`/`nXP`) keep the spelling the engine accepts.
The translation is original, AI-assisted work by @ogzhnklvz, written from the English strings with
Turkish desktop-CAD vocabulary (Katman, Tarama, Yuvarlatma, Pah, Öteleme, Nesne Kenetleme,
Görünüm Penceresi); the running app was reviewed by a native speaker, and line-by-line
proofreading is still to do. The bundled
egui fonts cover `Ç Ğ İ Ö Ş Ü ç ğ ı ö ş ü`.

Finnish has 1,384 catalog rows: the union of the existing catalogs, including the Ukrainian
reference, four contextual menu headings and two count templates. Its plural rule is `one|other`
like English; after any number but 1 Finnish uses the partitive singular (`1 osallistuja`,
`2 osallistujaa`). The menu headings **Muokkaa** (Edit) and **Muuta** (Modify) are kept apart, and
**Mitoitus** heads the Dimension menu while a dimension object is **Mitta**. A Finnish system
locale (`fi-FI`) selects Finnish; `sv-FI` keeps Swedish. The bundled fonts cover `ÄÖÅäöå` and
`ŠšŽž`. The translation is AI-assisted (Claude Code), written from the English strings in
ordinary Finnish CAD vocabulary; it has not yet been proofread line by line.

Drawing titles, layer/layout/block/style names, parameter names and expressions, text in a
drawing, and command-line input are user data and retain their original spelling. Clickable
command keywords retain their English spelling so the text shown matches the input accepted
by the engine. CLI/MCP/control responses, engine diagnostics and command history remain
English. Uncatalogued text also falls back to English. Number formatting and the operating
system's own file-picker controls follow the existing behavior.

The bundled egui fonts cover Ukrainian (`Ґґ Єє Іі Її`), German (`ÄÖÜ äöü ß`), Czech
(`ÁČĎÉĚÍŇÓŘŠŤÚŮÝŽ áčďéěíňóřšťúůýž`, including typographic quotes `„…“`), and Hungarian
(`ÁÉÍÓÖŐÚÜŰ áéíóöőúüű`, including `„…”`).
The translations add no font assets and work on the web without system fonts.

Korean needs a font that covers **every Hangul character in its catalog** and the language
picker label. The CJK font loaded in every language (Noto Sans CJK, Microsoft YaHei, PingFang)
usually covers Hangul already, and then nothing more is loaded. Otherwise, only when Korean is
chosen, desktop builds look for a `Hang`/`Kore` face from the optional `CRAFT_FONTS_DIR` input,
then installed fonts: Malgun Gothic/Gulim on Windows, Apple SD Gothic Neo/AppleGothic on macOS,
and Nanum Gothic or Noto CJK on Linux/FreeBSD. Collection faces are checked individually, and
file reads/face scans are bounded. No system fonts or new font assets are redistributed.

Web builds cannot read installed fonts and embed no Korean face (to stay within hosting size
limits), so the web interface stays English when Korean is chosen. Without a font covering the
catalog, the saved `ko` preference is kept but the interface falls back to English; the language
command reports that a font is needed, and the picker uses the readable label **Korean** wherever
Hangul can't be shown. This change does not fix platform IME issues or localize engine output.

## Greek (`el`) interface

The Greek translation covers all **1,330** distinct source keys collected from the existing
language catalogs: **1,324** ordinary interface labels, **4** contextual menu headings and
**2** integer-count templates. The interface can be selected as **Ελληνικά** under
Edit → Interface Language, or automatically when the system language is `el-GR` or `el-CY`.
Greek integer plurals distinguish 1 from all other counts.

The terminology follows native Greek CAD vocabulary; specialist terms retain the English
name in parentheses when it helps professionals match a known CAD command (for example,
**Επίπεδο (Layer)**, **Πολυγραμμή (Polyline)** and **Διαγράμμιση (Hatch)**).
Common actions such as Open and Save appear simply in Greek. Existing command identifiers,
drawing content, names of user objects and file formats remain unchanged. The translation is
original AI-assisted work contributed by @Camthalion77 with language review; it is not sourced
from Autodesk localization files. Independent native proofreading and visual checks remain
follow-up work.

## Adding translations

The implementation follows the display-time catalog pattern used by
[VectorCraft #205](https://github.com/storytold/vectorcraft/pull/205),
[PdfCraft #106](https://github.com/storytold/pdfcraft/pull/106), and
[FilmCraft #238](https://github.com/storytold/filmcraft/pull/238). Its code is local to CADCraft.

- `crates/ui-egui/src/i18n/*.tsv` are UTF-8 with three tab-separated columns:
  `context`, `English source`, and `translation`. Ordinary labels have an empty context.
  Optional `menu` entries translate menu headings via `i18n::menu` / `i18n::tr_menu`;
  without one, the ordinary translation is used.
- `@plural` entries hold the language's forms for integer counts: Ukrainian `one|few|many`,
  German and Spanish `one|other`, Brazilian Portuguese `one|other` (0 and 1 are `one`),
  Czech `one|few|other` (1; 2–4; everything else, including 0, 11, 21 and 22).
  Korean and Hungarian have one form (`other`) for all integer counts.
  The API takes `u64`, so fractional counts are not accepted.
- Register a new catalog in `i18n::LANGUAGES`, add its preference variant and code/name,
  and set its integer plural rule and form count on the `Language` row.
- Use `crate::tl!("English label")` for a literal, `i18n::t(label)` for a known dynamic UI
  label, and `crate::tf!("Version {version}", version = value)` for named placeholders.
  Translate only at the display boundary; never look up arbitrary user names or identifiers.
- Windows retain explicit IDs based on their English source, and tool groups retain their
  source names as keys, so switching languages preserves their state.
- Add a translation attribution row in `ATTRIBUTION.md`, update the language lists, and add
  locale, plural, font-glyph and live-switch tests. Add the catalog to `REPORTED` in
  `i18n/tests.rs`: only Ukrainian, the reference translation, is in `REQUIRED`, so adding a
  command needs a row in one language, not in all of them.
- Catalogs are embedded and parsed once. Missing or malformed rows cannot crash the app;
  tests reject invalid rows, duplicate keys, changed placeholders, and mismatched ellipses.
- `cargo test -p cadcraft-ui-egui i18n` checks menu/command/tool/snap coverage, every marked
  UI literal, locale resolution, preferences, integer plurals, bundled glyphs and actual
  egui renders with language switching. `cargo xtask assets` checks asset attribution;
  `cargo xtask ci` runs the workspace quality gates.

Translations are original work under `MIT OR Apache-2.0`, written from the meaning of
CADCraft's English strings. Do not copy proprietary localization resources.
