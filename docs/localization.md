# Interface languages

Choose **Edit → Interface Language** and select **Auto (system language)**, **English**, or
**Українська**. The interface updates without restarting, including the native macOS menus.
The choice is saved with the other interface preferences (`cadcraft.prefs`, beside the theme):
eframe storage on the desktop, browser local storage on the web. Installations without a saved
choice use Auto.

Auto uses the first supported language in the operating system's preferred locales, or the
browser's `navigator.languages` on the web. Ukrainian tags such as `uk-UA` and `uk_UA.UTF-8`
resolve to `uk`; unsupported languages fall back to English. Set `CADCRAFT_LOCALE=uk-UA` to
override the desktop's locale for a launch. An explicit saved English or Ukrainian choice
takes precedence over that override.

Automation can select the language through the existing control channel:

```json
{"id":1,"method":"engine.execute","params":{"command":"ui.language","params":{"lang":"uk"}}}
```

The `ui.set` method also accepts `{"interfaceLanguage":"uk"}`. Supported values are `auto`,
`en`, and `uk`; invalid values are rejected without changing the current selection. Resetting
palettes keeps the language choice.

## Translation coverage

Ukrainian covers command and menu labels, tool groups and tooltips, the Start page, file and
model-space tabs, status controls, object snaps, drafting prompts, property labels, dialogs,
and the About window's controls. The command reference can be searched by English command
ID, English label, or Ukrainian label.

Drawing titles, layer/layout/block/style names, parameter names and expressions, text in a
drawing, and command-line input are user data and retain their original spelling. Clickable
command keywords retain their English spelling so the text shown matches the input accepted
by the engine. CLI/MCP/control responses, engine diagnostics and command history remain
English. Uncatalogued text also falls back to English. Number formatting and the operating
system's own file-picker controls follow the existing behavior.

The bundled egui fonts cover Ukrainian, including `Ґґ Єє Іі Її`; the translation adds no font
assets and works on the web without system fonts.

## Adding translations

The implementation follows the display-time catalog pattern used by
[VectorCraft #205](https://github.com/storytold/vectorcraft/pull/205),
[PdfCraft #106](https://github.com/storytold/pdfcraft/pull/106), and
[FilmCraft #238](https://github.com/storytold/filmcraft/pull/238). Its code is local to CADCraft.

- `crates/ui-egui/src/i18n/uk.tsv` is UTF-8 with three tab-separated columns:
  `context`, `English source`, and `translation`. Ordinary labels have an empty context.
  `@plural` entries hold Ukrainian `one|few|many` forms for integer counts.
- Register a new catalog in `i18n::LANGUAGES`, add its preference variant and code/name,
  and supply its integer plural rule if it uses `@plural` rows.
- Use `crate::tl!("English label")` for a literal, `i18n::t(label)` for a known dynamic UI
  label, and `crate::tf!("Version {version}", version = value)` for named placeholders.
  Translate only at the display boundary; never look up arbitrary user names or identifiers.
- Windows retain explicit IDs based on their English source, and tool groups retain their
  source names as keys, so switching languages preserves their state.
- Catalogs are embedded and parsed once. Missing or malformed rows cannot crash the app;
  tests reject invalid rows, duplicate keys, changed placeholders, and mismatched ellipses.
- `cargo test -p cadcraft-ui-egui i18n` checks menu/command/tool/snap coverage, every marked
  UI literal, locale resolution, preferences, integer plurals, bundled glyphs and an actual
  English/Ukrainian/English egui render. `cargo xtask ci` runs the workspace quality gates.

Translations are original work under `MIT OR Apache-2.0`, written from the meaning of
CADCraft's English strings. Do not copy proprietary localization resources.
