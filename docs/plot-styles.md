# Plot styles

A plot style table decides how objects print. CADCraft applies it to PDF plots (PLOT,
EXPORTPDF and anything that plots through them). It also applies it to a layout shown with
**Display plot styles** on: on screen and in the layout's PNG and SVG exports.

## Tables

| Kind | Picks the style by | Styles |
|---|---|---|
| Colour-dependent (`.ctb`) | the object's index colour (a true colour uses its nearest index colour) | exactly 255, `Color_1` … `Color_255` |
| Named (`.stb`) | the object's **PlotStyle** property | `Normal` first, then any names |

A style can set these:

| Field | Meaning | "Use object" value |
|---|---|---|
| `color` | output colour, `"#rrggbb"` | absent / `null` |
| `grayscale` | print the colour as the grey of its luminance (Rec. 709 weights) | `false` |
| `screening` | ink intensity 0–100 %, mixing toward the white paper | `100` |
| `linetype` | linetype name (loaded, or from the built-in library; `Continuous` draws solid) | absent / `null` |
| `lineweight` | hundredths of a millimetre, 0–211; used when lineweights are plotted | absent / `null` |
| `end`, `join`, `fill` | line end, line join and area fill style | `object` |

Colour 7 prints black.

### Line ends, joins and fill patterns

- **`end` and `join`** shape lines wider than 0.25 mm on paper (from lineweights). The line
  is drawn as its filled outline: `butt` ends at the end point, `square` reaches half the
  width past it, `round` adds a half circle and `diamond` a point. `miter` joins meet in a
  point (bevelled past 10 half widths), `bevel` cuts the corner, `round` rounds it and
  `diamond` points out one half width. `object` is round, as lines are drawn anyway.
- **`fill`** replaces the solid fill of filled areas: 2D solids, traces, wide polylines,
  solid hatches and filled text. `solid` and `object` keep it solid. The patterns are
  CADCraft's own definitions, measured on paper and anchored to the sheet's origin. They
  repeat every 1 mm:

| Fill | Pattern |
|---|---|
| `checkerboard` | 0.5 mm squares, every other one filled |
| `crosshatch` | horizontal and vertical bars, 0.2 mm wide |
| `diamonds` | 0.35 mm squares turned 45°, every 0.7 mm |
| `horizontalBars`, `verticalBars` | bars a third of the repeat wide |
| `slantLeft`, `slantRight` | `\` and `/` bars a third of the repeat wide |
| `squareDots` | 0.2 mm squares every 0.5 mm |

Wipeouts are never patterned. A plot that would need more than two million pattern cells keeps
the remaining areas solid. Layouts draw these while the display list is built. A model-space
plot draws them at its plot scale.

### Built-in tables

These are CADCraft's own definitions, not copies of any vendor's files.

| Table | What it does |
|---|---|
| `default.ctb` | keeps every object property |
| `monochrome.ctb` | prints every colour black |
| `grayscale.ctb` | prints every colour as the grey of its luminance |
| `default.stb` | `Normal` (object properties), `Black`, `Screened 50%`, `Thick` (0.50 mm) |
| `monochrome.stb` | `Normal` prints black; `Screened 50%` prints black at 50 % |

A table kept in the drawing shadows a built-in table of the same name.

## Named plot styles

Layers have a plot style name (default `Normal`). Objects have `ByLayer` (the default), `ByBlock`
or a style name. `ByLayer` takes the layer's style. On layer 0 inside a block, that is the block
reference's layer. `ByBlock` takes the block reference's style. A name the table doesn't have
prints as `Normal`. A colour-dependent table ignores the property.

## Commands

- `pagesetup {layout, plotStyleTable, displayPlotStyles}` sets the layout's table and the
  Display plot styles flag. Without a layout, it reports the built-in tables in `plotStyleTables`.
- `plot` / `exportpdf {plotStyleTable}` replaces the page setup's table for one plot. `"None"`
  plots object colours.
- `plotstyle {name, handles?}` sets the objects' plot style. With nothing selected, it sets the
  current plot style for new objects (`CPLOTSTYLE`). Without `name`, it reports the current
  style and the named styles.
- `layer.set {name, plotStyle}` sets a layer's plot style.
- `stylesmanager {action, …}` manages the tables: `list`, `get`, `new` (`kind`, `from`), `set`
  (`style: {name, …fields}`, `removeStyle`, `description`), `delete`, `load` (`path` or `text`)
  and `save` (`path`, or returns `text`). Typed or chosen from **File ▸ Plot Styles...**, it
  opens the Plot Style Manager dialog.

## Table files

Tables are saved in our own format: UTF-8 JSON with a format marker. Fields left at "use the
object's" are omitted.

```json
{
  "cadcraftPlotStyleTable": 1,
  "name": "office.stb",
  "description": "Office pens",
  "kind": "named",
  "styles": [
    { "name": "Normal" },
    { "name": "Pen 5", "color": "#000000", "screening": 40, "lineweight": 35, "linetype": "HIDDEN", "end": "round" }
  ]
}
```

- `kind` is `colorDependent` or `named`.
- `end` is one of `object`, `butt`, `square`, `round` or `diamond`.
- `join` is one of `object`, `miter`, `bevel`, `round` or `diamond`.
- `fill` is one of `object`, `solid`, `checkerboard`, `crosshatch`, `diamonds`, `horizontalBars`,
  `slantLeft`, `slantRight`, `squareDots` or `verticalBars`.
- `color` may also be written `[r, g, b]`.

Reading fixes hostile values:

- Screening is capped at 100 and lineweights at 211.
- A colour-dependent table always gets 255 styles named `Color_n`.
- A named table gets unique style names (at most 1024), with `Normal` first.
- Files over 4 MB are refused.

## Importing AutoCAD `.ctb`/`.stb`/`.pc3` files

**CADCraft does not read AutoCAD's binary plot style tables (`.ctb`, `.stb`) or plotter
configurations (`.pc3`, `.pmp`).** Those formats have no public specification, and CADCraft is
clean-room: it does not reverse-engineer vendor files, use code that did, or ship vendor data.
`STYLESMANAGER load` on one of them says so and names this page; it never guesses at the file and
never changes the drawing.

The supported route is to recreate the table once in CADCraft's documented JSON format and load
that. Nothing is read from the vendor file, so you only need the settings that the source
application's plot style table editor shows.

### Migration checklist

1. **Read the settings in the source application.** Open the CTB or STB in its plot style table
   editor. For each colour (CTB) or named style (STB), note the output colour, screening,
   lineweight, linetype, grayscale and, if used, the line end, join and fill.
2. **Choose the kind.** A colour-dependent table (from a CTB) is `"kind": "colorDependent"`. A
   named table (from an STB) is `"kind": "named"`.
3. **Map the names.** In a colour-dependent table the styles are `Color_1` … `Color_255`, one per
   AutoCAD Color Index (ACI) colour. List only the colours that differ from the object's
   properties: loading fills the rest with "use the object's". In a named table the style names
   must match the `PlotStyle` names the layers and objects use; `Normal` is always there.
4. **Convert the numbers.** In the file, `lineweight` is hundredths of a millimetre (`0.35` mm is
   `35`), `screening` is 0–100, and `color` is `"#rrggbb"`.
5. **Load it.** `stylesmanager {"action": "load", "path": "office.ctb.json"}` (or Load in
   **File ▸ Plot Styles...**).
6. **Assign and check it.** `pagesetup {layout, plotStyleTable}` (or Page Setup), then Plot
   Preview or plot a one-sheet test to PDF.
7. **Keep the file.** It is an ordinary file; keep it with your templates. A user plot style
   library that shares one table across drawings is the follow-up in #1901.

### A CTB template

This is **CADCraft's** format, not a CTB. A binary `.ctb` is never read; the marker
`cadcraftPlotStyleTable` is what tells CADCraft the file is its own.

```json
{
  "cadcraftPlotStyleTable": 1,
  "name": "office-monochrome.ctb",
  "description": "Converted from the office plot style table",
  "kind": "colorDependent",
  "styles": [
    { "name": "Color_1", "color": "#000000", "screening": 100, "lineweight": 35, "linetype": "Continuous", "end": "round", "join": "round", "fill": "solid" },
    { "name": "Color_2", "color": "#000000", "screening": 80, "lineweight": 25 },
    { "name": "Color_3", "grayscale": true, "lineweight": 18 },
    { "name": "Color_7", "color": "#000000" },
    { "name": "Color_250", "color": "#808080", "screening": 60, "lineweight": 13 }
  ]
}
```

- `Color_1` … `Color_255` map to ACI colours 1–255 (colour 0 is ByBlock and 256 is ByLayer, so
  neither has a style): 1 red, 2 yellow, 3 green, 4 cyan, 5 blue, 6 magenta, 7 white (prints
  black), 8 and 9 greys, 10–249 the rest of the palette, 250–255 greys. Colours not listed keep
  the object's properties.
- In a style, every field except `name` may be left out. A missing field means "use the
  object's", which is `screening` 100, `lineweight` and `linetype` from the object, and
  `end`/`join`/`fill` `object`.
- `end` is `object`, `butt`, `square`, `round` or `diamond`; `join` is `object`, `miter`,
  `bevel`, `round` or `diamond`; `fill` is `object`, `solid`, `checkerboard`, `crosshatch`,
  `diamonds`, `horizontalBars`, `slantLeft`, `slantRight`, `squareDots` or `verticalBars`.
- A colour-dependent table always ends up with 255 styles named `Color_n`, including the ones
  you don't list.

A named table (from an STB) uses the same file with `"kind": "named"`; name the styles as the
drawing does:

```json
{
  "cadcraftPlotStyleTable": 1,
  "name": "office.stb",
  "description": "Converted from the office named plot style table",
  "kind": "named",
  "styles": [
    { "name": "Normal" },
    { "name": "Pen 1", "color": "#000000", "lineweight": 35, "end": "round", "join": "round" },
    { "name": "Pen 2", "color": "#000000", "screening": 50, "lineweight": 25 }
  ]
}
```

A `.pc3` is a plotter configuration, not a plot style table. CADCraft doesn't read those either;
recreate the device and its papers in CADCraft's plotter JSON format instead
([plotters.md](plotters.md)).

## In DXF files

- The page setup's table is PLOTSETTINGS group 7, and Display plot styles is plot layout flag 2
  (group 70).
- Plot style names follow the DXF Reference. The named object dictionary entry
  `ACAD_PLOTSTYLENAME` is an `ACDBDICTIONARYWDFLT` whose entries name `ACDBPLACEHOLDER` objects
  (default `Normal`). Layers and objects point at them with group 390. An object without a 390
  is `ByLayer`. `ByBlock` isn't written, so it reads back as `ByLayer`.
- Tables kept in the drawing travel in the `CADCRAFT_PLOTSTYLES` XRECORD (JSON) of the named
  object dictionary.
