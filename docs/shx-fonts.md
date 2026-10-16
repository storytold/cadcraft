# SHX text fonts

CADCraft reads user-supplied compiled stroke fonts in the `AutoCAD-86 shapes 1.0`,
`shapes 1.1` and Unicode `unifont 1.0` formats. It draws their pen programs as strokes,
using their cap height and pen-end advances. TEXT, MTEXT (including inline font switches,
wrapping and paragraph formatting), attributes, dimensions and table text use the same resolver.
Font names in DXF styles are preserved on save.

## Supplying fonts

Put fonts you are entitled to use in a separate directory and set `CADCRAFT_SHXPATH` before
starting CADCraft. It is an OS path list: semicolons on Windows, colons on Unix. For example,
in PowerShell:

```powershell
$env:CADCRAFT_SHXPATH = 'C:\Users\you\CADFonts'
cadcraft drawing.dxf
```

The resolver searches those directories once, without recursion, by case-insensitive file name
or stem (`romans.shx` and `romans`). It does not search CAD installation bundles. Font files
are loaded only when used. Restart after changing the directories or installing a font.
Available SHX fonts take precedence over FONTALT. Missing, invalid or unsupported fonts keep
the existing FONTALT / CADCraft Stroke fallback. The original built-in stroke font now covers
the precomposed Latin accents used in Portuguese, instead of mixing thin letters with filled
TrueType accents. Characters absent from an SHX font retain the per-character fallback.

The Rust `cadcraft_fonts::shx::register(name, bytes)` API accepts user-supplied bytes on native
and WASM builds. There is no web font-upload UI yet; the web build cannot search local folders.
No proprietary font is embedded, redistributed or included as a test fixture.

## Supported programs and limits

The interpreter supports pen up/down, the 16 compact vector directions, scaling, location
push/pop, subshapes, displacements, octant arcs, fractional arcs, bulge arcs and skipping
vertical-only commands during horizontal layout. Arcs are tessellated with up to 128 segments
per revolution. Glyphs are decoded lazily and shared across placements.

Big fonts, packed multibyte encodings, vertical layout, SHP source loading and linetype shape
elements remain unsupported. Legacy shape fonts map Latin character numbers and the three
dimension symbols; alternate legacy character encodings are not inferred from the drawing.
Unicode SHX text uses bidi display order; OpenType joining is available through TrueType fonts,
not SHX programs. MTEXT still uses the existing attachment and line-pitch rules: this change
does not claim to fix the separate SHX layout offsets reported in #801.

Hostile-input limits: 8 MiB per file; 8 KiB per glyph program; 32 loaded fonts / 64 MiB of
program bytes; 32 subshape levels and location-stack entries; 65,536 interpreter steps and
8,192 points per decoded glyph; finite coordinates within ±1,000,000 font units. Each font
caches at most 4,096 glyphs / 262,144 points. Directory scans stop after 8,192 entries across
at most 32 directories. Unsupported or over-limit programs fall back without panicking.

## Format references and verification

The pen language and metrics come from Autodesk's public
[Shape Descriptions](https://help.autodesk.com/cloudhelp/2026/ENU/AutoCAD-MAC-Customization/files/GUID-DE941DB5-7044-433C-AA68-2A9AE98A5713.htm),
[Special Codes Reference](https://help.autodesk.com/cloudhelp/2026/ENU/AutoCAD-MAC-Customization/files/GUID-06832147-16BE-4A66-A6D0-3ADF98DC8228.htm)
and [Text Font Descriptions](https://help.autodesk.com/cloudhelp/2026/ENU/AutoCAD-MAC-Customization/files/GUID-9BBE5B28-DF02-4EC5-863A-BA04AB6F5EF1.htm).
The compiled container layout was checked against the public MIT-licensed
[ezdxf format reader](https://github.com/mozman/ezdxf/blob/master/src/ezdxf/fonts/shapefile.py),
as a format reference; no library code or font definitions are incorporated.

Binary headers end in CR/LF and `0x1A`. Shapes files have a little-endian first/last/count
header, an `(id, record length)` index, named records and an `EOF` marker. Unifont has a
little-endian record count, definition id/length and named definition with cap/descent/mode/
encoding/embedding fields, followed by `(id, length, named program)` records. Unicode subshape
operands use high-byte-first ordering (distinct from little-endian record ids), as reported
in [#801](https://github.com/storytold/cadcraft/issues/801).

Tests assemble original programs directly in Rust and cover both containers, native advances,
accent subshapes, arcs, pen/stack operations, inline MTEXT, wrapping, DXF Unicode escapes and
round trips. Truncation at every byte, deterministic mutations, cycles and invalid operands
exercise fallback and bounds. Implementation uses the public format references above; automated
tests use only original synthetic fixtures. At the reporter's explicit request, a manual local
check also loaded their installed `romans.shx` while rendering their DXF. Neither that font nor
the drawing is included in the repository or automated tests.
