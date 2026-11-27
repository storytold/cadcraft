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
- the sample drawings (`crates/engine/src/sample.rs`);
- the synthetic two-hexagon hatch logo (`crates/io/tests/it/hatch_loop_flags.rs`, `logo`):
  original contributor geometry, generated in code from invented points, under MIT OR Apache-2.0.

| Asset | Author | Source | Licence | Notes |
|---|---|---|---|---|
| `licenses/jpeg-encoder-0.6.1/` | jpeg-encoder contributors and Independent JPEG Group | [jpeg-encoder 0.6.1](https://crates.io/crates/jpeg-encoder/0.6.1) | (MIT OR Apache-2.0) AND IJG | Dependency license and SIMD notices; source hashes in SOURCE.json. |
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
| `assets/app-icon/README.md` | CADCraft contributors | original work for this repository | MIT OR Apache-2.0 | How each icon is generated and which mark each channel uses |
| `assets/app-icon/cadcraft-nightly.png` | CADCraft contributors | generated from `assets/app-icon/cadcraft-1024.png` | MIT OR Apache-2.0 | Nightly mark: same glyph, deeper background `#0c657b`; the Linux nightly tar's icon |
| `assets/app-icon/cadcraft-nightly-1024.png` | CADCraft contributors | generated from `assets/app-icon/cadcraft-1024.png` | MIT OR Apache-2.0 | 1024 master the nightly `.icns` is generated from |
| `assets/app-icon/cadcraft-nightly.ico` | CADCraft contributors | generated from `assets/app-icon/cadcraft-nightly-1024.png` | MIT OR Apache-2.0 | Windows nightly icon (16–256 px entries) for `build.rs` |
| `assets/app-icon/cadcraft-nightly.icns` | CADCraft contributors | generated from `assets/app-icon/cadcraft-nightly-1024.png` | MIT OR Apache-2.0 | macOS nightly bundle icon (`CFBundleIconFile cadcraft-nightly`) |
| `assets/app-icon/nightly-provenance.json` | CADCraft contributors | original work for this repository | MIT OR Apache-2.0 | The exact background transform that produced the nightly set |
| `packaging/macos/dmg/background.svg` | @XusBadia | original work for this repository (2026-10-08), "CADCraft macOS DMG window background"; derived from the CADCraft app icon (`assets/app-icon/`, referenced via `<image>`, not copied); text is outlined Inter / JetBrains Mono glyph paths (OFL fonts the project already uses); no third-party material modified | MIT OR Apache-2.0 (same as the code: `LICENSE-MIT`, `LICENSE-APACHE`) | Finder window background of the macOS DMG; `packaging/macos/dmg/generate.py` renders it to `background.tiff` (see `packaging/macos/dmg/README.md`) |
| `packaging/macos/dmg/background.tiff` | @XusBadia | Rendered from `packaging/macos/dmg/background.svg` by `packaging/macos/dmg/generate.py` | MIT OR Apache-2.0 (same as the code: `LICENSE-MIT`, `LICENSE-APACHE`) | 1x + 2x HiDPI TIFF for the macOS DMG window; includes the app icon (see its row) and LittleCMS's built-in sRGB profile ("No copyright, use freely") |
| `examples/apartment.dxf` | CADCraft contributors | sample drawing made with cadcraft-cli commands | MIT OR Apache-2.0 | Original |
| `docs/images/ui-apartment.png` | CADCraft contributors | screenshot of CADCraft itself (examples/apartment.dxf) | MIT OR Apache-2.0 | Original; no Autodesk UI |
| `docs/images/ui-layout.png` | CADCraft contributors | screenshot of CADCraft itself (a layout of examples/apartment.dxf) | MIT OR Apache-2.0 | Original; no Autodesk UI |
| `docs/images/ui-bracket.png` | CADCraft contributors | screenshot of CADCraft itself (sample drawing generated in code) | MIT OR Apache-2.0 | Original; no Autodesk UI |
| `docs/brand/artcraft-logo-white.png` | ArtCraft Team | ArtCraft brand kit | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |
| `docs/brand/artcraft-logo-white.svg` | ArtCraft Team | ArtCraft brand kit | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |
| `docs/brand/artcraft-logo.png` | ArtCraft Team | ArtCraft brand kit | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |
| `docs/brand/artcraft-logo.svg` | ArtCraft Team | ArtCraft brand kit | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |
| `docs/brand/artcraft-mark-black.png` | ArtCraft Team | ArtCraft brand kit | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |
| `docs/brand/artcraft-mark-black.svg` | ArtCraft Team | ArtCraft brand kit | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |
| `docs/brand/artcraft-mark.png` | ArtCraft Team | ArtCraft brand kit | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |
| `docs/brand/artcraft-mark.svg` | ArtCraft Team | ArtCraft brand kit | ArtCraft brand terms (`docs/brand/LICENSE-brand.txt`) | Trademark, not open source |

## Interface translations

| Asset | Author | Source | Licence | Notes |
|---|---|---|---|---|
| `crates/ui-egui/src/i18n/uk.tsv` | @dmatviichuk | Original Ukrainian translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/pl.tsv` | @neronek22 (Echo Rozumu & Gadajło; AI-assisted) | Original Polish translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/de.tsv` | the CADCraft contributors | Original German translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/es.tsv` | @PedroGuille | Original Spanish translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/pt_br.tsv` | @WladmirJunior | Original Brazilian Portuguese translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/bs.tsv` | Nine (@nine-codes) with Claude Code (AI-assisted) | Original Bosnian translations of CADCraft English strings, shared with storytold/craft-i18n `locales/bs/` | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied; not yet reviewed by a native speaker |
| `crates/ui-egui/src/i18n/cs.tsv` | @Losminek (AI-assisted) | Original Czech translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/ru.tsv` | @pechnikov2026-star | Original Russian translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/fr.tsv` | @pechnikov2026-star; terminology pass by @YannFR81 | Original French translations of CADCraft English strings; AI-assisted consistency pass (imperative prompts, cote/espace objet/réseau/présentation) | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/zh.tsv` | @mayloglog | Original Simplified Chinese translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/sv.tsv` | Erik Holmbom (@holmbom) with Claude Code (AI-assisted) | Original Swedish translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied; terminology and the running app reviewed by a native speaker |
| `crates/ui-egui/src/i18n/it.tsv` | Emanuele Bottelli (AI-assisted); terminology corrections: Andrea Brandi (@starise) with OpenAI Codex (AI-assisted) | Original Italian translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied; targeted Chamfer terminology review by a native speaker |
| `crates/ui-egui/src/i18n/ko.tsv` | @huk082 (AI-assisted) | Original Korean translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied; no font assets added |
| `crates/ui-egui/src/i18n/ar.tsv` | @hasan-sdaif | Original Arabic translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/hu.tsv` | @Skaywalker | Original Hungarian translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied |
| `crates/ui-egui/src/i18n/tr.tsv` | @ogzhnklvz (AI-assisted) | Original Turkish translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied; written from the English strings, no other catalog used as a source; the running app reviewed by a native speaker |
| `crates/ui-egui/src/i18n/el.tsv` | @Camthalion77 with ChatGPT (AI-assisted); 46 later rows: @V1dalTheAbidal with Claude Code (AI-assisted) | Original Greek translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied; the 46 later rows were read by their contributor, a native Greek speaker; independent native proofreading of the whole file pending |
| `crates/ui-egui/src/i18n/vi.tsv` | @2ez4gcx with Claude Code (AI-assisted) | Original Vietnamese translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied; terminology and the craft-i18n#401 phase 1 changes reviewed by a native Vietnamese speaker; older rows not yet proofread in full |
| `crates/ui-egui/src/i18n/ka.tsv` | Davit Matchakhelidze (@devius), native speaker, with Claude Code | Original Georgian translations of CADCraft English strings | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied; written from the English strings, no other catalog used as a source; terminology and doubtful strings decided by a native Georgian speaker; no font assets added (the Georgian face comes from craft-fonts) |
| `crates/ui-egui/src/i18n/ta.tsv` | @newsbaba24x7 (AI-assisted) | Original Tamil translations of CADCraft English strings, shared with storytold/craft-i18n `locales/ta/` | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | No proprietary localization resources copied; written from the English strings; the 1,289 rows from craft-i18n reviewed there for language and terminology by native speakers, and the whole Tamil interface reviewed in the running app by native speakers Radhika and Mohan Raj (2026-11-14); no font assets added (Tamil comes from an installed font or the craft-fonts face) |


## PDF import

The CAD mapping and initial synthetic PDF tests are adapted from CADCraft contributor
@DuukDuul's [PR #2787](https://github.com/storytold/cadcraft/pull/2787), commit
`ec17f63f189d1dccf91357701f9bba243ca88dd5`, under MIT OR Apache-2.0. The mapping has
been changed for the current shared reader, bounded import, fill rules and diagnostics.
PDF test inputs in `crates/io/tests/it/pdf_import.rs`, `crates/io/src/pdf_read.rs`, and
`apps/cadcraft-cli/tests/it/pdf_import.rs` are generated from literal content streams or
CadCraft's own plotter. No customer documents are included.

Enabling `craft-pdf` interpretation uses the following unchanged dependency assets from
`craft-libs` revision `453b0da782ccef39be9a6dac8d27c80b553ef1cc`. Fonts remain in the
shared dependency. Their source and hashes are retained below; notices are in `NOTICE`.

| Asset | Author | Source | Licence | SHA-256 | Notes |
|---|---|---|---|---|---|
| `hayro-interpret 0.7.0 / assets/FoxitDingbats.pfb` | Foxit Software Inc. / PDFium Authors | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/assets/FoxitDingbats.pfb | BSD-3-Clause | `845c752392b6c914fb989c75a08b7792b88f542d2499042ef2889f8c814a16ed` | Renders non-embedded standard-14 fonts (pinned hayro-interpret) |
| `hayro-interpret 0.7.0 / assets/FoxitFixed.pfb` | Foxit Software Inc. / PDFium Authors | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/assets/FoxitFixed.pfb | BSD-3-Clause | `b6c8fe53f134b8b6d4578cd2d544df4cee9624c4efa8d51a560fb40ea296101b` | Renders non-embedded standard-14 fonts (pinned hayro-interpret) |
| `hayro-interpret 0.7.0 / assets/FoxitFixedBold.pfb` | Foxit Software Inc. / PDFium Authors | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/assets/FoxitFixedBold.pfb | BSD-3-Clause | `f1b7159702973f54fc86254ea38bcf3712b2a736eb3a0995751e9f4bb45ad603` | Renders non-embedded standard-14 fonts (pinned hayro-interpret) |
| `hayro-interpret 0.7.0 / assets/FoxitFixedBoldItalic.pfb` | Foxit Software Inc. / PDFium Authors | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/assets/FoxitFixedBoldItalic.pfb | BSD-3-Clause | `8a000945843bd31add06aee63bf9fd41b7578b4f3242a4b7cd46349ad9d24d4d` | Renders non-embedded standard-14 fonts (pinned hayro-interpret) |
| `hayro-interpret 0.7.0 / assets/FoxitFixedItalic.pfb` | Foxit Software Inc. / PDFium Authors | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/assets/FoxitFixedItalic.pfb | BSD-3-Clause | `5007faf8320fa1fcc08b8894b356ad976f60b992ba29ee5272578bbdebaf3876` | Renders non-embedded standard-14 fonts (pinned hayro-interpret) |
| `hayro-interpret 0.7.0 / assets/FoxitSans.pfb` | Foxit Software Inc. / PDFium Authors | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/assets/FoxitSans.pfb | BSD-3-Clause | `a02339e95d20db14cbdfc1c6baf359cd36a091d5434925c3a4040f60e1a1af9e` | Renders non-embedded standard-14 fonts (pinned hayro-interpret) |
| `hayro-interpret 0.7.0 / assets/FoxitSansBold.pfb` | Foxit Software Inc. / PDFium Authors | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/assets/FoxitSansBold.pfb | BSD-3-Clause | `7bdfa36de1d5757dcfaf8b8a4be95c6e41c40fa70dbed9d78bf979a7b9959054` | Renders non-embedded standard-14 fonts (pinned hayro-interpret) |
| `hayro-interpret 0.7.0 / assets/FoxitSansBoldItalic.pfb` | Foxit Software Inc. / PDFium Authors | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/assets/FoxitSansBoldItalic.pfb | BSD-3-Clause | `555ce45924338e2e4b32821910221ce85da691fd59a9723c4c971a7c5627fc8b` | Renders non-embedded standard-14 fonts (pinned hayro-interpret) |
| `hayro-interpret 0.7.0 / assets/FoxitSansItalic.pfb` | Foxit Software Inc. / PDFium Authors | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/assets/FoxitSansItalic.pfb | BSD-3-Clause | `7751cbefeda351708dae8c4a88db043411bc39ab2ae2097685da845d4d16f307` | Renders non-embedded standard-14 fonts (pinned hayro-interpret) |
| `hayro-interpret 0.7.0 / assets/FoxitSerif.pfb` | Foxit Software Inc. / PDFium Authors | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/assets/FoxitSerif.pfb | BSD-3-Clause | `4f57d2b9d884af8f907bf22df6019b52d86cbf6214fdbd02b1ae05472a543f35` | Renders non-embedded standard-14 fonts (pinned hayro-interpret) |
| `hayro-interpret 0.7.0 / assets/FoxitSerifBold.pfb` | Foxit Software Inc. / PDFium Authors | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/assets/FoxitSerifBold.pfb | BSD-3-Clause | `0bdf4b04e964139818d51eda03d566ba999fc3ba2421b1c6c51f9dc969022e80` | Renders non-embedded standard-14 fonts (pinned hayro-interpret) |
| `hayro-interpret 0.7.0 / assets/FoxitSerifBoldItalic.pfb` | Foxit Software Inc. / PDFium Authors | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/assets/FoxitSerifBoldItalic.pfb | BSD-3-Clause | `a406cac82583bf98175cb62c87ed5e95c45fbb34eece63a10b0a13e793cb2e10` | Renders non-embedded standard-14 fonts (pinned hayro-interpret) |
| `hayro-interpret 0.7.0 / assets/FoxitSerifItalic.pfb` | Foxit Software Inc. / PDFium Authors | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/assets/FoxitSerifItalic.pfb | BSD-3-Clause | `610ae0687198045c4db5d1a6650fd5e92536631706382eb8b72e38df578d0ae9` | Renders non-embedded standard-14 fonts (pinned hayro-interpret) |
| `hayro-interpret 0.7.0 / assets/FoxitSymbol.pfb` | Foxit Software Inc. / PDFium Authors | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/assets/FoxitSymbol.pfb | BSD-3-Clause | `47967d055530e7357088a08403115425643ec2cdfd6201ba8af0fbd7116c1539` | Renders non-embedded standard-14 fonts (pinned hayro-interpret) |
| `hayro-interpret 0.7.0 / assets/CGATS001Compat-v2-micro.icc` | Clinton Ingram (saucecontrol/Compact-ICC-Profiles) | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/assets/CGATS001Compat-v2-micro.icc | CC0-1.0 | `73e1ba37d2bad5bab2a964f40a9eed96209666efc067c3322626214bbef234a0` | Default CMYK colour conversion (pinned hayro-interpret) |
| `hayro-cmap 0.1.0 / assets/cmaps.brotli` | Adobe | https://github.com/storytold/pdfcraft/blob/a2ec27b4775f53c40cad3456936d9406611e3a8d/vendor/hayro-cmap/assets/cmaps.brotli | BSD-3-Clause | `ddf37af521d904d5646c2fc37b2b7e912dc1b56f0d210190b61f598154758e46` | Character-code mapping tables required to read CJK PDFs. Non-visual dependency data; existing embedding unchanged |
| `hayro-interpret 0.7.0 / src/font/generated/metrics.rs` | Adobe (Core 14 AFM metrics, PDF specification encodings); tables by The Hayro Authors | https://github.com/storytold/pdfcraft/blob/7437bde378f43856eada40af25b983812e615e8a/vendor/hayro-interpret/src/font/generated/metrics.rs | MIT OR Apache-2.0 | `2ec9e608573eda459de1e1d9bda3bb494315e36bee885acc88912a11fe0caa41` | Glyph widths and encodings for non-embedded standard fonts. Non-visual dependency data; existing embedding unchanged |
