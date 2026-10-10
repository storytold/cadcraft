# File-format parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** minor (WIPEOUT and hatch gradients now written, #147, #162) · **Target:** Autodesk AutoCAD 2027

Every format AutoCAD 2027 reads or writes, with CADCraft's support, fidelity and tests. Work
items are in [gaps.md](gaps.md#file-formats); the weighted score feeds
[target-app-parity.md](target-app-parity.md).

**Headline (estimated):** file formats ≈ **30%** of AutoCAD's. Opening the commonest 2D DXF/DWG
drawings works, but saving is lossy: everything CADCraft doesn't model is dropped, output is
always R2000, and no test uses a real third-party file. Remaining: **≈ 170–280 Opus 5.5 hours**:
≈ 40–60 h for DXF/DWG fidelity (the part that decides beta), ≈ 110–180 h for the other formats
below, and ≈ 20–40 h for 3D exchange formats that wait for 3D modelling.

## How this was measured

- CADCraft: read from `crates/dxf`, `crates/io` (`dxf_read.rs`, `dxf_write.rs`, `pdf.rs`,
  `svg.rs`, `tests.rs`) and `crates/dwg` on 2026-10-10. Nothing was run (the measurement pass
  did no builds).
- AutoCAD: the formats in its Open/Save As/Import/Export/Attach/Plot dialogs, from Autodesk's
  published documentation and the menu tree in `xtask/data/menu-catalog.txt`. The installed
  AutoCAD 2027 for Mac bundle was inspected by listing names only (clean-room rule: nothing
  inside it is read): `Resources/Support` lists `acad.lin`, `acadiso.lin`, `acad.pat`,
  `acadiso.pat`, `acad.mln`, `.dct` spell dictionaries, `.pss` and `.shx` files, which confirms the
  supporting formats below exist in the target. None of them may be copied.
- No AutoCAD-produced file is committed or used as a fixture. A real-file corpus needs owner
  action (see [gaps.md](gaps.md)): files made by people who license them to us, kept in a
  separate `storytold/cadcraft-corpus` repo.

## DWG, version by version

AutoCAD 2027 opens every DWG version from R2.0 onward and saves 2018 (AC1032, still the current
DWG format), 2013, 2010, 2007, 2004, 2000 and R14. CADCraft reads DWG by converting it to DXF
with the `acadrust` 0.6.1 crate and reading that DXF; it saves by writing its own R2000 DXF and
converting it to DWG.

| DWG version | Code | AutoCAD 2027 | CADCraft read | CADCraft write | Tested how |
|---|---|---|---|---|---|
| R1.0–R2.6, R9, R10, R11/R12 | MC0.0–AC1009 | read | **no** (`is_dwg` accepts only `AC10xx` magic; acadrust doesn't target pre-R13) | no | — |
| R13 | AC1012 | read | acadrust claims support; untested | no | none |
| R14 | AC1014 | read, write | acadrust claims support; untested | **no** | none |
| 2000–2002 | AC1015 | read, write | yes via bridge | **yes, always this version** | synthetic round trips (`dwg_roundtrip_keeps_non_ascii_text`, `dwg_roundtrip_keeps_extension_data`) |
| 2004–2006 | AC1018 | read, write | via bridge; untested | no (version list in `crates/dwg` is unused) | none |
| 2007–2009 | AC1021 | read, write | via bridge; untested | no | none |
| 2010–2012 | AC1024 | read, write | via bridge | no | one synthetic file (`ac1024_dwg_with_many_entities_opens`, made by rewriting `$ACADVER`) |
| 2013–2017 | AC1027 | read, write | via bridge; untested | no | none |
| 2018–2027 | AC1032 | read, write (default) | via bridge; untested on real files | no | none |
| Web build | — | — | **no** (error: "save as DXF") | no | — |

Everything lost in the DXF read/write paths below is also lost through DWG, plus whatever
`acadrust`'s own DXF output omits (proxy objects, ACIS data, custom objects: not measured). User
report #57: a 10 MB DWG failed after about two minutes; limits now stop runaway conversions
early (#175), but the throughput problem remains. **DWG ≈ 30% read, ≈ 15% write.**

## DXF, version by version

| DXF version | AutoCAD 2027 | CADCraft read | CADCraft write |
|---|---|---|---|
| R12 (AC1009), ASCII | read, write | yes: no version gating; old-style POLYLINE/VERTEX read (polyface meshes flattened to 3D polylines) | **no** |
| R12 binary | read, write | **no** (binary parser expects R13+ 2-byte group codes) | no |
| R13–R2018, ASCII | read, write each of 2000/2004/2007/2010/2013/2018 | yes, one reader for all versions | **R2000 (AC1015) only** |
| R13–R2018, binary | read, write | yes | **no** (ASCII only) |
| Code pages (`$DWGCODEPAGE`, ANSI_932/936/949/950/125x) | yes | **no**: UTF-8, else Latin-1, so pre-2007 CJK and Cyrillic files are garbled | writes `\U+` escapes (#169), which is correct |
| `\U+`/`\M+` escapes | yes | `\U+` yes; `\M+` not checked | `\U+` yes |

### DXF content: read and write by object type

| Content | Read | Write | Notes |
|---|---|---|---|
| LINE, POINT, CIRCLE, ARC, ELLIPSE, LWPOLYLINE (widths, bulges, elevation), SPLINE, RAY, XLINE | yes | yes | round-trip tests |
| POLYLINE 2D/3D | yes | yes (as LWPOLYLINE / 3D POLYLINE) | polyface and polygon meshes flattened: lossy |
| TEXT, MTEXT, ATTDEF, INSERT + ATTRIB | yes | yes | ATTDEF prompt kept (#137) |
| DIMENSION (all 7 kinds) | yes | yes, with anonymous `*D` blocks | `*D` blocks duplicate on re-save (known); DIMASSOC objects written |
| LEADER | yes | yes | |
| MULTILEADER | **no** (becomes Unknown) | **no**: our MLeaders are written as LEADER + MTEXT | MULTILEADER is AutoCAD's default leader since 2008 |
| HATCH pattern / solid | yes | yes | |
| HATCH gradient | yes | yes (#162, fixes #80) | |
| SOLID, TRACE, 3DFACE | yes | yes | |
| ACAD_TABLE | yes | yes (entity + block) | |
| VIEWPORT | yes | yes; frozen-layer lists kept (#150) | VP layer colour overrides lost (#99, PR #197) |
| IMAGE | partial (no IMAGEDEF, so the file path is empty) | **dropped** | |
| WIPEOUT | yes | yes (#147, fixes #79) | |
| MLINE, REGION, 3DSOLID, BODY, SURFACE, MESH, TOLERANCE, SHAPE, HELIX, UNDERLAY, OLE2FRAME, LIGHT, CAMERA, SECTION, POINTCLOUD, proxy entities | kept as Unknown (raw tags) | **dropped on save** | the most serious file gap: opening and saving someone's drawing deletes what we don't model |
| Header variables | all read | 36 written | AutoCAD writes about 280 |
| Tables: LAYER (+ transparency, description), LTYPE, STYLE, DIMSTYLE, BLOCK_RECORD | yes | yes | |
| Tables: VPORT, VIEW, UCS, APPID | **not read** | VPORT one record; VIEW and UCS empty; APPID yes | named views and UCSs lost |
| Objects: LAYOUT, DICTIONARY, XRECORD, TABLESTYLE, DIMASSOC | yes | yes | |
| Objects: GROUP, MLINESTYLE, MLEADERSTYLE, IMAGEDEF, SCALE, VISUALSTYLE, PLOTSETTINGS, materials, field objects | ignored | not written (ACAD_GROUP empty) | groups lost |
| Foreign xdata, extension dictionaries, reactors | **not kept** | — | third-party application data lost |
| Handles and owners | regenerated | consistent | handles change on every save |

**DXF ≈ 50% read, ≈ 30% write** (estimated; weights: common 2D entities 50%, annotation objects
20%, tables/objects 15%, foreign data preservation 15%).

## Other formats

| Format | AutoCAD 2027 | CADCraft | Hours to parity (est.) |
|---|---|---|---|
| DWT templates | open, save | no (no template support) | 3–5 |
| DWS standards | open, save, CHECKSTANDARDS | no | 6–10 |
| PDF output (PLOT, EXPORTPDF) | multi-sheet, layers, searchable text, hyperlinks | vector PDF 1.4, one page, lineweights; **no font objects** (text is strokes), no layers (OCG), no multi-sheet | 10–15 |
| PDF import (PDFIMPORT) and PDF underlays (PDFATTACH) | yes | no | 15–25 |
| Raster images attach (PNG, JPEG, TIFF, BMP …) | yes | IMAGE entity exists, no attach/loading/display path | 6–10 |
| PNG / JPEG / BMP / TIFF export | PNGOUT, JPGOUT, BMPOUT, TIFOUT | PNG; SVG too (AutoCAD doesn't export SVG) | 1–2 |
| DWF/DWFx export, DWF underlay | yes | no | 10–15 |
| DGN import/export/underlay | yes | no (needs a public spec; owner decision) | 25–40 |
| WMF, EPS, ACIS SAT, STL, FBX, IGES/STEP (Windows), 3DS | various | no; most are 3D and wait for M11 | 20–40 |
| SHX fonts (`.shx`, big fonts) | read | **not parsed**: our stroke font or FONTALT stands in, so text metrics differ from AutoCAD | 8–12 |
| Linetype (`.lin`) and hatch pattern (`.pat`) files | load user files | own built-in library only; user files can't be loaded | 3–5 |
| Plot styles (`.ctb`, `.stb`), plotter configs (`.pc3`), `.pmp` | yes | no | 8–12 |
| Scripts (`.scr`) | SCRIPT | script text through the CLI, MCP and control channel; no SCRIPT command reading a `.scr` file | 1–2 |
| Aliases (`.pgp`), CUI(x), workspaces | yes | no | 6–10 |
| AutoLISP (`.lsp`, `.fas`, `.vlx`), `.dll`/`.arx` plug-ins | yes | no | see [gaps.md](gaps.md) (M12) |
| Data extraction (`.csv`, `.xls`) and data links | yes | no | 6–10 |
| eTransmit (`.zip` packages) | yes | no | 4–6 |

## Tests

`crates/io/src/tests.rs` has 36 tests (round trips of entities, styles, associativity, tables,
constraints, non-ASCII text, DWG round trips, hostile input). **All inputs are synthetic**:
inline strings or our own writer's output. No test opens a file made by another program, and no
external validator runs in CI (ezdxf's audit was run by hand once, 2026-10-06). The next step is
a licensed real-file corpus with an open → save → re-open → compare test per file, and checking
our output in AutoCAD black-box.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | minor | WIPEOUT and gradients written since #147/#162; other save losses remain (#201, #204, #208, #209, #213, #223) |
| 2026-10-10 | major | First file-format parity doc: DWG and DXF version by version, per-object read/write, other formats |
