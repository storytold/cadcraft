# Asset attribution

Every non-code asset in this repository (images, icons, fonts, example drawings, presets) is listed
here with its author, source and licence. `cargo xtask assets` (part of `cargo xtask ci`) fails if
an asset file is missing from this table.

**Policy (mandatory):** CADCraft contains **no Autodesk, Adobe or Avid iconography, images,
artwork, fonts, hatch patterns, linetypes, templates or presets.** Every asset is original work by
CADCraft contributors or third-party material under an open licence (OSI open source, public domain
/ CC0, or Creative Commons that allows redistribution). Screenshots of Autodesk software are never
committed. Font files are not added here: shared fonts live in
[storytold/craft-fonts](https://github.com/storytold/craft-fonts), an optional build input. The one
exception is the ArtCraft brand in `docs/brand/`: ArtCraft trademarks, not open source, used under
`docs/brand/LICENSE-brand.txt`.

Generated-in-code assets are original and have no file to list:
- the UI icon set (`crates/ui-egui/src/icons.rs`);
- the "CADCraft Stroke" single-stroke drafting font (`crates/fonts/src/stroke.rs`);
- the standard linetype and hatch-pattern libraries (`crates/doc/src/library.rs`; industry-common
  names, our own dash/spacing values);
- the colour index palette, generated from its structure (`crates/color/src/lib.rs`);
- the sample drawings (`crates/engine/src/sample.rs`).

| Asset | Author | Source | Licence | Notes |
|---|---|---|---|---|
| `assets/app-icon/hicolor/128x128/apps/ai.storyteller.cadcraft.png` | CADCraft contributors | generated from `assets/app-icon/cadcraft-1024.png` | MIT OR Apache-2.0 | Linux hicolor icon |
| `assets/app-icon/hicolor/16x16/apps/ai.storyteller.cadcraft.png` | CADCraft contributors | generated from `assets/app-icon/cadcraft-1024.png` | MIT OR Apache-2.0 | Linux hicolor icon |
| `assets/app-icon/hicolor/24x24/apps/ai.storyteller.cadcraft.png` | CADCraft contributors | generated from `assets/app-icon/cadcraft-1024.png` | MIT OR Apache-2.0 | Linux hicolor icon |
| `assets/app-icon/hicolor/256x256/apps/ai.storyteller.cadcraft.png` | CADCraft contributors | generated from `assets/app-icon/cadcraft-1024.png` | MIT OR Apache-2.0 | Linux hicolor icon |
| `assets/app-icon/hicolor/32x32/apps/ai.storyteller.cadcraft.png` | CADCraft contributors | generated from `assets/app-icon/cadcraft-1024.png` | MIT OR Apache-2.0 | Linux hicolor icon |
| `assets/app-icon/hicolor/48x48/apps/ai.storyteller.cadcraft.png` | CADCraft contributors | generated from `assets/app-icon/cadcraft-1024.png` | MIT OR Apache-2.0 | Linux hicolor icon |
| `assets/app-icon/hicolor/512x512/apps/ai.storyteller.cadcraft.png` | CADCraft contributors | generated from `assets/app-icon/cadcraft-1024.png` | MIT OR Apache-2.0 | Linux hicolor icon |
| `assets/app-icon/hicolor/64x64/apps/ai.storyteller.cadcraft.png` | CADCraft contributors | generated from `assets/app-icon/cadcraft-1024.png` | MIT OR Apache-2.0 | Linux hicolor icon |
| `assets/app-icon/cadcraft.icns` | CADCraft contributors | generated (packaging/icons.sh) | MIT OR Apache-2.0 | macOS icon |
| `assets/app-icon/cadcraft.ico` | CADCraft contributors | generated (packaging/icons.sh) | MIT OR Apache-2.0 | Windows icon |
| `assets/app-icon/cadcraft-64.png` | CADCraft contributors | generated | MIT OR Apache-2.0 | |
| `assets/app-icon/cadcraft-256.png` | CADCraft contributors | generated | MIT OR Apache-2.0 | |
| `assets/app-icon/cadcraft-1024.png` | CADCraft contributors | generated | MIT OR Apache-2.0 | |
| `assets/app-icon/cadcraft-macos-512.png` | CADCraft contributors | generated (macOS margin variant) | MIT OR Apache-2.0 | |
| `packaging/macos/dmg/background.svg` | @XusBadia | original work for this repository (2026-10-08), "CADCraft macOS DMG window background"; derived from the CADCraft app icon (`assets/app-icon/`, referenced via `<image>`, not copied); text is outlined Inter / JetBrains Mono glyph paths (OFL fonts the project already uses); no third-party material modified | MIT OR Apache-2.0 (same as the code: `LICENSE-MIT`, `LICENSE-APACHE`) | Finder window background of the macOS DMG; `packaging/macos/dmg/generate.py` renders it to `background.tiff` (see `packaging/macos/dmg/README.md`) |
| `packaging/macos/dmg/background.tiff` | @XusBadia | Rendered from `packaging/macos/dmg/background.svg` by `packaging/macos/dmg/generate.py` | MIT OR Apache-2.0 (same as the code: `LICENSE-MIT`, `LICENSE-APACHE`) | 1x + 2x HiDPI TIFF for the macOS DMG window; includes the app icon (see its row) and LittleCMS's built-in sRGB profile ("No copyright, use freely") |
| `examples/apartment.dxf` | CADCraft contributors | sample drawing made with cadcraft-cli commands | MIT OR Apache-2.0 | Original |
| `docs/images/ui-apartment.png` | CADCraft contributors | screenshot of CADCraft itself (examples/apartment.dxf) | MIT OR Apache-2.0 | Original; no Autodesk UI |
| `docs/images/ui-layout.png` | CADCraft contributors | screenshot of CADCraft itself (a layout of examples/apartment.dxf) | MIT OR Apache-2.0 | Original; no Autodesk UI |
| `docs/images/ui-bracket.png` | CADCraft contributors | screenshot of CADCraft itself (sample drawing generated in code) | MIT OR Apache-2.0 | Original; no Autodesk UI |
| `docs/brand/artcraft-logo-white.png` | ArtCraft Team | storytold/craftrules `assets/brand/` | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |
| `docs/brand/artcraft-logo-white.svg` | ArtCraft Team | storytold/craftrules `assets/brand/` | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |
| `docs/brand/artcraft-logo.png` | ArtCraft Team | storytold/craftrules `assets/brand/` | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |
| `docs/brand/artcraft-logo.svg` | ArtCraft Team | storytold/craftrules `assets/brand/` | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |
| `docs/brand/artcraft-mark-black.png` | ArtCraft Team | storytold/craftrules `assets/brand/` | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |
| `docs/brand/artcraft-mark-black.svg` | ArtCraft Team | storytold/craftrules `assets/brand/` | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |
| `docs/brand/artcraft-mark.png` | ArtCraft Team | storytold/craftrules `assets/brand/` | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |
| `docs/brand/artcraft-mark.svg` | ArtCraft Team | storytold/craftrules `assets/brand/` | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |

## Interface translations

| Asset | Author | Source | Licence | Notes |
|---|---|---|---|---|
| `crates/ui-egui/src/i18n/uk.tsv` | @dmatviichuk | Original Ukrainian translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
