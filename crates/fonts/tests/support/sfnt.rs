//! A synthetic TrueType (sfnt) font file, built in code for tests that need a real font without a
//! font file in the repository or one installed on the machine: `head`, `hhea`, `maxp`, `hmtx`,
//! `glyf`, `loca`, a format-4 `cmap` and `OS/2`, laid out in a sorted table directory.
//!
//! Test-only, and shared: `crates/render/src/tests.rs` (the level-of-detail fixture) and
//! `crates/io/tests/pdf_text.rs` (the PDF writer's embedding fixtures) build their fonts with it,
//! each including this file with `#[path]`, so neither needs a published API, a dependency or a
//! copy of the other's tables. `crates/fonts` is the layer that reads those tables
//! (`crates/fonts/src/ttf.rs`), which is why the skeleton lives beside its tests; the glyph shapes,
//! the character map ranges and the metrics a font records stay at the caller that needs them.

fn be16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}

fn be32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

/// A simple glyph, with the point and contour counts `maxp` records for the font.
pub struct Glyph {
    bytes: Vec<u8>,
    points: usize,
    contours: usize,
}

/// `.notdef`: no contours, no bytes.
pub fn notdef() -> Glyph {
    Glyph { bytes: Vec::new(), points: 0, contours: 0 }
}

/// One simple glyph: closed contours of on-curve points, with the box the file records for it.
pub fn glyph(contours: &[Vec<(i16, i16)>]) -> Glyph {
    let points: Vec<(i16, i16)> = contours.iter().flatten().copied().collect();
    let mut g = Vec::new();
    be16(&mut g, contours.len() as u16);
    let box_ = points.iter().fold((i16::MAX, i16::MAX, i16::MIN, i16::MIN), |b, p| (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1)));
    for v in [box_.0, box_.1, box_.2, box_.3] {
        be16(&mut g, v as u16);
    }
    let mut end = 0u16;
    for c in contours {
        end = end.saturating_add(c.len() as u16);
        be16(&mut g, end.saturating_sub(1));
    }
    be16(&mut g, 0);
    g.extend(std::iter::repeat_n(1u8, points.len()));
    let (mut px, mut py) = (0i16, 0i16);
    for (x, _) in &points {
        be16(&mut g, x.wrapping_sub(px) as u16);
        px = *x;
    }
    for (_, y) in &points {
        be16(&mut g, y.wrapping_sub(py) as u16);
        py = *y;
    }
    while g.len() % 4 != 0 {
        g.push(0);
    }
    Glyph { bytes: g, points: points.len(), contours: contours.len() }
}

/// What the callers differ in; every other table field is the same in each font this builds.
pub struct Sfnt<'a> {
    /// `glyf` entries, `.notdef` first.
    pub glyphs: &'a [Glyph],
    /// `cmap` format-4 segments: first character code, last character code, and glyph id minus
    /// code. The `0xFFFF` segment that ends the map is added here.
    pub cmap: &'a [(u16, u16, i16)],
    /// The advance width every glyph shares, in em units.
    pub advance: u16,
    /// The left side bearing every glyph shares.
    pub lsb: i16,
    /// `hmtx` entries written. Fewer than the glyphs `hhea` declares is a deliberately short
    /// table: that font is malformed on purpose, and a reader must cope with it, not repair it.
    pub metrics: usize,
    /// `OS/2` `fsType`: the embedding permissions, `0` for any use.
    pub fs_type: u16,
    /// `head`'s ink box: xMin, yMin, xMax, yMax.
    pub ink_box: [i16; 4],
}

/// Em units per em: with a 700-unit cap height, one em is 1000 units, so a text height of one unit
/// is a cap height of one unit.
const UNITS_PER_EM: u16 = 1000;

/// The font file `f` describes, as a TrueType file.
pub fn bytes(f: &Sfnt<'_>) -> Vec<u8> {
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for g in f.glyphs {
        be32(&mut loca, glyf.len() as u32);
        glyf.extend_from_slice(&g.bytes);
    }
    be32(&mut loca, glyf.len() as u32);
    let mut head = Vec::new();
    be32(&mut head, 0x0001_0000); // version
    be32(&mut head, 0x0001_0000); // fontRevision
    be32(&mut head, 0); // checkSumAdjustment
    be32(&mut head, 0x5F0F_3CF5); // magic
    be16(&mut head, 0); // flags
    be16(&mut head, UNITS_PER_EM);
    head.extend_from_slice(&[0; 16]); // created, modified
    for v in f.ink_box {
        be16(&mut head, v as u16);
    }
    // macStyle, lowestRecPPEM, fontDirectionHint, indexToLocFormat 1 = long offsets (`loca` holds
    // `u32`), glyphDataFormat.
    for v in [0u16, 8, 2, 1, 0] {
        be16(&mut head, v);
    }
    let mut hhea = Vec::new();
    be32(&mut hhea, 0x0001_0000);
    // ascender, descender, lineGap, advanceWidthMax, minLeftSideBearing, minRightSideBearing,
    // xMaxExtent, caretSlopeRise, caretSlopeRun, caretOffset, four reserved, metricDataFormat.
    for v in [800i16, -200, 0, f.advance as i16, 0, 0, f.advance as i16, 1, 0, 0, 0, 0, 0, 0, 0] {
        be16(&mut hhea, v as u16);
    }
    be16(&mut hhea, f.glyphs.len() as u16); // numberOfHMetrics
    let max_points = f.glyphs.iter().map(|g| g.points).max().unwrap_or(0);
    let max_contours = f.glyphs.iter().map(|g| g.contours).max().unwrap_or(0);
    let mut maxp = Vec::new();
    be32(&mut maxp, 0x0001_0000);
    // numGlyphs, maxPoints, maxContours, maxCompositePoints, maxCompositeContours, maxZones, and
    // the composite and instruction maxima after them.
    for v in [f.glyphs.len() as u16, max_points as u16, max_contours as u16, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0] {
        be16(&mut maxp, v);
    }
    let mut hmtx = Vec::new();
    for _ in 0..f.metrics {
        be16(&mut hmtx, f.advance);
        be16(&mut hmtx, f.lsb as u16);
    }
    // cmap: version 0 with one encoding record (Windows, Unicode BMP), a format-4 subtable over
    // the caller's segments and the `0xFFFF` one that ends it.
    let segs: Vec<(u16, u16, i16)> = f.cmap.iter().copied().chain([(0xFFFF, 0xFFFF, 1)]).collect();
    let seg_count = segs.len() as u32;
    let seg_selector = seg_count.ilog2();
    let seg_search_range = 2 * (1 << seg_selector);
    let mut sub = Vec::new();
    be16(&mut sub, 4); // format
    be16(&mut sub, (16 + segs.len() * 8) as u16); // length
    be16(&mut sub, 0); // language
    be16(&mut sub, (segs.len() * 2) as u16); // segCountX2
    for v in [seg_search_range as u16, seg_selector as u16, (2 * seg_count - seg_search_range) as u16] {
        be16(&mut sub, v);
    }
    for s in &segs {
        be16(&mut sub, s.1);
    }
    be16(&mut sub, 0); // reservedPad
    for s in &segs {
        be16(&mut sub, s.0);
    }
    for s in &segs {
        be16(&mut sub, s.2 as u16);
    }
    for _ in &segs {
        be16(&mut sub, 0); // idRangeOffset
    }
    let mut cmap = Vec::new();
    for v in [0u16, 1, 3, 1] {
        be16(&mut cmap, v);
    }
    be32(&mut cmap, 12); // the subtable, straight after the records
    cmap.extend_from_slice(&sub);
    let mut os2 = vec![0u8; 78];
    os2[8..10].copy_from_slice(&f.fs_type.to_be_bytes());
    let mut tables: Vec<(&[u8; 4], Vec<u8>)> =
        vec![(b"OS/2", os2), (b"cmap", cmap), (b"glyf", glyf), (b"head", head), (b"hhea", hhea), (b"hmtx", hmtx), (b"loca", loca), (b"maxp", maxp)];
    tables.sort_by_key(|t| *t.0);
    let mut out = Vec::new();
    be32(&mut out, 0x0001_0000); // sfnt version 1.0
    let num_tables = tables.len() as u32;
    let table_selector = num_tables.ilog2();
    let dir_search_range = 16 * (1 << table_selector);
    // numTables, searchRange, entrySelector, rangeShift: 16-byte records, largest power of two first.
    for v in [num_tables as u16, dir_search_range as u16, table_selector as u16, (16 * num_tables - dir_search_range) as u16] {
        be16(&mut out, v);
    }
    let mut off = 12 + 16 * tables.len();
    for (t, d) in &tables {
        out.extend_from_slice(*t);
        be32(&mut out, 0); // checkSum: the readers here do not verify it
        be32(&mut out, off as u32);
        be32(&mut out, d.len() as u32);
        off += d.len().div_ceil(4) * 4;
    }
    for (_, d) in &tables {
        out.extend_from_slice(d);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    out
}
