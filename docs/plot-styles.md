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

### User library

Tables can also live in a user-level library shared by every drawing. It is read once at
start-up and holds JSON table files in the `plotstyles` folder of the CADCraft settings
directory:

| System | Folder |
|---|---|
| Linux and FreeBSD | `$XDG_CONFIG_HOME/cadcraft/plotstyles` or `~/.config/cadcraft/plotstyles` |
| macOS | `~/Library/Application Support/CADCraft/plotstyles` |
| Windows | `%APPDATA%\CADCraft\plotstyles` |

`CADCRAFT_CONFIG_DIR` replaces the settings directory. A page setup naming a table resolves it
with this precedence:

1. the drawing's own tables (saved with the drawing),
2. the user library,
3. the built-in tables.

Library tables are not copied into a drawing just because a page setup or the manager shows
them; only an explicit load keeps one in the drawing. Reading the folder is bounded and
hostile-input-safe: files over 4 MB are refused, malformed files are skipped, and a duplicate
table name keeps the first file. Every skipped file is reported as a warning by
`stylesmanager list` and in the log. On the web there is no settings directory, so the
built-in tables are used unless the host installs a store.

## Named plot styles

Layers have a plot style name (default `Normal`). Objects have `ByLayer` (the default), `ByBlock`
or a style name. `ByLayer` takes the layer's style. On layer 0 inside a block, that is the block
reference's layer. `ByBlock` takes the block reference's style. A name the table doesn't have
prints as `Normal`. A colour-dependent table ignores the property.

## Commands

- `pagesetup {layout, plotStyleTable, displayPlotStyles}` sets the layout's table and the
  Display plot styles flag. Without a layout, it reports every table a drawing can use in
  `plotStyleTables` (the drawing's, the library's and the built-in ones).
- `plot` / `exportpdf {plotStyleTable}` replaces the page setup's table for one plot. `"None"`
  plots object colours.
- `plotstyle {name, handles?}` sets the objects' plot style. With nothing selected, it sets the
  current plot style for new objects (`CPLOTSTYLE`). Without `name`, it reports the current
  style and the named styles.
- `layer.set {name, plotStyle}` sets a layer's plot style.
- `stylesmanager {action, …}` manages the tables: `list`, `get`, `new` (`kind`, `from`), `set`
  (`style: {name, …fields}`, `removeStyle`, `description`), `delete`, `load` (`path` or `text`)
  and `save` (`path`, or returns `text`). `list` names the `source` of each table (`drawing`,
  `library` or `builtin`) and reports skipped library files in `warnings`. `load`, `save` and
  `delete` take `library: true` to work on the user library instead of the drawing (the library
  file is `<table name>.json`). Typed or chosen from **File ▸ Plot Styles...**, it opens the
  Plot Style Manager dialog.

## Table files

Tables are saved in our own format: UTF-8 JSON with a format marker. Fields left at "use the
object's" are omitted. The user library keeps the same files, named `<table name>.json`.

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

Binary `.ctb`/`.stb` files are not read. We haven't found a public specification of that format.

## In DXF files

- The page setup's table is PLOTSETTINGS group 7, and Display plot styles is plot layout flag 2
  (group 70).
- Plot style names follow the DXF Reference. The named object dictionary entry
  `ACAD_PLOTSTYLENAME` is an `ACDBDICTIONARYWDFLT` whose entries name `ACDBPLACEHOLDER` objects
  (default `Normal`). Layers and objects point at them with group 390. An object without a 390
  is `ByLayer`. `ByBlock` isn't written, so it reads back as `ByLayer`.
- Tables kept in the drawing travel in the `CADCRAFT_PLOTSTYLES` XRECORD (JSON) of the named
  object dictionary.
