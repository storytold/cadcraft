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
| `epaint_default_fonts::EGUI_ICONS`, `epaint_default_fonts::EMOJI_ICON` (dependency font inputs) | John Slegers | [egui 173cc737](https://github.com/storytold/egui/tree/6d518fc799681571963842f95b55a6b68431b15a/crates/epaint_default_fonts/fonts), [emoji-icon-font](https://github.com/jslegers/emoji-icon-font) | MIT (Copyright (c) 2014 John Slegers; full upstream text in `NOTICE`) | egui-icons.ttf is the five-glyph subset; emoji-icon-font.ttf is the full monochrome fallback. Registered by the production default font definitions; dependency assets unmodified here, no font file copied into this repository. |
| `krilla 0.8.2 / icc/sGrey-v2-magic.icc` (dependency-embedded) | Clinton Ingram | [craft-libs 9f89fe8 vendor/krilla/icc/sGrey-v2-magic.icc](https://github.com/storytold/craft-libs/blob/9f89fe8f6d72ab4db4cc06f186381a6a254463ab/vendor/krilla/icc/sGrey-v2-magic.icc) (Krilla 0.8.2 upstream 3ffdf0588cf98050aad6edba51ca70162e1fb5b5) | CC0-1.0 | Embedded by the pinned PDF writer (`craft-pdf` 0.1.0, Krilla 0.8.2); unmodified dependency asset, no file copied into this repository. SHA-256 `4110169287e5f1b5149ca9f1da1814d32943905bba8030e849d1a3b729a19481`. |
| `krilla 0.8.2 / icc/sGrey-v4.icc` (dependency-embedded) | Clinton Ingram | [craft-libs 9f89fe8 vendor/krilla/icc/sGrey-v4.icc](https://github.com/storytold/craft-libs/blob/9f89fe8f6d72ab4db4cc06f186381a6a254463ab/vendor/krilla/icc/sGrey-v4.icc) (Krilla 0.8.2 upstream 3ffdf0588cf98050aad6edba51ca70162e1fb5b5) | CC0-1.0 | Embedded by the pinned PDF writer (`craft-pdf` 0.1.0, Krilla 0.8.2); unmodified dependency asset, no file copied into this repository. SHA-256 `00c0f94e09127520a17dc0e1d9264b5702081d96dbdb1549ee88e3631ce42a9d`. |
| `krilla 0.8.2 / icc/sRGB-v2-magic.icc` (dependency-embedded) | Clinton Ingram | [craft-libs 9f89fe8 vendor/krilla/icc/sRGB-v2-magic.icc](https://github.com/storytold/craft-libs/blob/9f89fe8f6d72ab4db4cc06f186381a6a254463ab/vendor/krilla/icc/sRGB-v2-magic.icc) (Krilla 0.8.2 upstream 3ffdf0588cf98050aad6edba51ca70162e1fb5b5) | CC0-1.0 | Embedded by the pinned PDF writer (`craft-pdf` 0.1.0, Krilla 0.8.2); unmodified dependency asset, no file copied into this repository. SHA-256 `af4efe28f6d311799865f325ba39184a2e978b113ca74124d60af7de22b105f4`. |
| `krilla 0.8.2 / icc/sRGB-v4.icc` (dependency-embedded) | Clinton Ingram | [craft-libs 9f89fe8 vendor/krilla/icc/sRGB-v4.icc](https://github.com/storytold/craft-libs/blob/9f89fe8f6d72ab4db4cc06f186381a6a254463ab/vendor/krilla/icc/sRGB-v4.icc) (Krilla 0.8.2 upstream 3ffdf0588cf98050aad6edba51ca70162e1fb5b5) | CC0-1.0 | Embedded by the pinned PDF writer (`craft-pdf` 0.1.0, Krilla 0.8.2); unmodified dependency asset, no file copied into this repository. SHA-256 `c56e1685d888f5edb92fe07f2750f387f8fe8e91b32ff8fb0b56bfbbb9458353`. |
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
| `crates/ui-egui/src/i18n/de.tsv` | the CADCraft contributors | Original German translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/es.tsv` | @PedroGuille | Original Spanish translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/pt_br.tsv` | @WladmirJunior | Original Brazilian Portuguese translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/bs.tsv` | Nine (@nine-codes) with Claude Code (AI-assisted) | Original Bosnian translations of CADCraft English strings, shared with storytold/craft-i18n `locales/bs/` | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied; not yet reviewed by a native speaker |
| `crates/ui-egui/src/i18n/cs.tsv` | @Losminek (AI-assisted) | Original Czech translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/ru.tsv` | @pechnikov2026-star | Original Russian translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/fr.tsv` | @pechnikov2026-star | Original French translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/zh.tsv` | @mayloglog | Original Simplified Chinese translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/sv.tsv` | Erik Holmbom (@holmbom) with Claude Code (AI-assisted) | Original Swedish translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied; terminology and the running app reviewed by a native speaker |
| `crates/ui-egui/src/i18n/it.tsv` | Emanuele Bottelli (AI-assisted) | Original Italian translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/ko.tsv` | @huk082 (AI-assisted) | Original Korean translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied; no font assets added |
| `crates/ui-egui/src/i18n/ar.tsv` | @hasan-sdaif | Original Arabic translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/hu.tsv` | @Skaywalker | Original Hungarian translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/tr.tsv` | @ogzhnklvz (AI-assisted) | Original Turkish translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied; written from the English strings, no other catalog used as a source; the running app reviewed by a native speaker |
| `crates/ui-egui/src/i18n/el.tsv` | @Camthalion77 with ChatGPT (AI-assisted) | Original Greek translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied; independent native proofreading pending |
