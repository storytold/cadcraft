//! SVG export writes TEXT and MTEXT in installed outline fonts as `<text>` (searchable and
//! editable) by default, with `text: "geometry"` keeping the exact glyph outlines (#1876).
//!
//! The font is a tiny synthetic TrueType file built here (no font files in the repository).

use cadcraft_doc::{Common, Drawing, EntityKind, MText, Space, Text, TextStyle};
use cadcraft_geom::Vec3;
use cadcraft_io::svg::{self, TextMode};
use serde_json::json;

// ---------- a minimal TrueType font: .notdef, "A" (triangle), "B" (square) ----------

/// Append `x` to `v` big-endian, as a TrueType table stores 16-bit values.
fn be16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}
/// Append `x` to `v` big-endian, as a TrueType table stores 32-bit values.
fn be32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

/// One simple TrueType glyph from `pts`, all flagged on-curve, padded to a 4-byte boundary.
fn simple_glyph(pts: &[(i16, i16)]) -> Vec<u8> {
    let mut g = Vec::new();
    let xs: Vec<i16> = pts.iter().map(|p| p.0).collect();
    let ys: Vec<i16> = pts.iter().map(|p| p.1).collect();
    be16(&mut g, 1);
    for v in [*xs.iter().min().unwrap(), *ys.iter().min().unwrap(), *xs.iter().max().unwrap(), *ys.iter().max().unwrap()] {
        be16(&mut g, v as u16);
    }
    be16(&mut g, pts.len() as u16 - 1);
    be16(&mut g, 0);
    g.extend(std::iter::repeat_n(1u8, pts.len()));
    let (mut px, mut py) = (0i16, 0i16);
    for (x, _) in pts {
        be16(&mut g, (x - px) as u16);
        px = *x;
    }
    for (_, y) in pts {
        be16(&mut g, (y - py) as u16);
        py = *y;
    }
    while g.len() % 4 != 0 {
        g.push(0);
    }
    g
}

/// A name table naming the family (and full/PostScript names) "SvgTest".
fn name_table() -> Vec<u8> {
    let string: Vec<u8> = "SvgTest".encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut t = Vec::new();
    be16(&mut t, 0);
    be16(&mut t, 3);
    be16(&mut t, 6 + 3 * 12);
    for id in [1u16, 4, 6] {
        for v in [3u16, 1, 0x0409, id, string.len() as u16, 0] {
            be16(&mut t, v);
        }
    }
    t.extend(string);
    t
}

/// A TrueType font with glyphs .notdef (empty), A (triangle) and B (square); its cmap also maps
/// `&`, `<` and `>` so XML escaping can be tested.
fn test_font() -> Vec<u8> {
    let glyphs = [Vec::new(), simple_glyph(&[(50, 0), (550, 0), (300, 700)]), simple_glyph(&[(50, 0), (550, 0), (550, 700), (50, 700)])];
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for g in &glyphs {
        be32(&mut loca, glyf.len() as u32);
        glyf.extend_from_slice(g);
    }
    be32(&mut loca, glyf.len() as u32);
    let mut head = Vec::new();
    be32(&mut head, 0x0001_0000);
    be32(&mut head, 0x0001_0000);
    be32(&mut head, 0);
    be32(&mut head, 0x5F0F_3CF5);
    be16(&mut head, 0);
    be16(&mut head, 1000);
    head.extend_from_slice(&[0; 16]);
    for v in [0i16, 0, 600, 700] {
        be16(&mut head, v as u16);
    }
    for v in [0u16, 8, 2, 1, 0] {
        be16(&mut head, v);
    }
    let mut hhea = Vec::new();
    be32(&mut hhea, 0x0001_0000);
    for v in [800i16, -200, 0, 600, 0, 0, 600, 1, 0, 0, 0, 0, 0, 0, 0] {
        be16(&mut hhea, v as u16);
    }
    be16(&mut hhea, 3);
    let mut maxp = Vec::new();
    be32(&mut maxp, 0x0001_0000);
    for v in [3u16, 4, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0] {
        be16(&mut maxp, v);
    }
    let mut hmtx = Vec::new();
    for _ in 0..3 {
        be16(&mut hmtx, 600);
        be16(&mut hmtx, 50);
    }
    // cmap: format 4, segments for &, <, > and 'A'..'B'.
    let segs: [(u16, u16, u16); 5] = [
        (0x26, 0x26, 1u16.wrapping_sub(0x26)),
        (0x3C, 0x3C, 2u16.wrapping_sub(0x3C)),
        (0x3E, 0x3E, 1u16.wrapping_sub(0x3E)),
        (0x41, 0x42, 1u16.wrapping_sub(0x41)),
        (0xFFFF, 0xFFFF, 1),
    ];
    let mut sub = Vec::new();
    be16(&mut sub, 4);
    be16(&mut sub, (16 + segs.len() * 8) as u16);
    be16(&mut sub, 0);
    be16(&mut sub, (segs.len() * 2) as u16);
    for v in [4u16, 1, 0] {
        be16(&mut sub, v);
    }
    for s in segs {
        be16(&mut sub, s.1);
    }
    be16(&mut sub, 0);
    for s in segs {
        be16(&mut sub, s.0);
    }
    for s in segs {
        be16(&mut sub, s.2);
    }
    for _ in segs {
        be16(&mut sub, 0);
    }
    let mut cmap = Vec::new();
    for v in [0u16, 1, 3, 1] {
        be16(&mut cmap, v);
    }
    be32(&mut cmap, 12);
    cmap.extend_from_slice(&sub);
    let mut os2 = vec![0u8; 78];
    os2[8..10].copy_from_slice(&0u16.to_be_bytes());
    let mut tables: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"OS/2", os2),
        (b"cmap", cmap),
        (b"glyf", glyf),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca),
        (b"maxp", maxp),
        (b"name", name_table()),
    ];
    tables.sort_by_key(|t| *t.0);
    let mut out = Vec::new();
    be32(&mut out, 0x0001_0000);
    for v in [tables.len() as u16, 128, 3, tables.len() as u16 * 16 - 128] {
        be16(&mut out, v);
    }
    let mut off = 12 + 16 * tables.len();
    for (t, d) in &tables {
        out.extend_from_slice(*t);
        be32(&mut out, 0);
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

/// Register the synthetic test font once per process, so every drawing here can use it.
fn fonts() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| cadcraft_fonts::ttf::register("cadcraft-svg-test.ttf", test_font()));
}

/// A metric drawing whose "Real" text style uses the synthetic outline font.
fn drawing() -> Drawing {
    fonts();
    let mut d = Drawing::new_metric();
    d.text_styles.push(TextStyle { name: "Real".into(), font: "cadcraft-svg-test.ttf".into(), ..TextStyle::default() });
    d
}

/// Add a single-line TEXT of `value` at `at`, `height` tall, in the synthetic font.
fn text(d: &mut Drawing, at: (f64, f64), value: &str, height: f64) {
    let t = Text {
        insert: Vec3::new(at.0, at.1, 0.0),
        align_pt: None,
        height,
        value: value.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Real".into(),
        halign: Default::default(),
        valign: Default::default(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::Text(t)).unwrap();
}

/// Add an MTEXT at `at`, 10 units tall and unbounded wide, whose `contents` use `\P` for line
/// breaks.
fn mtext(d: &mut Drawing, at: (f64, f64), contents: &str) {
    let t = MText {
        insert: Vec3::new(at.0, at.1, 0.0),
        height: 10.0,
        width: 0.0,
        attach: 1,
        rotation: 0.0,
        style: "Real".into(),
        contents: contents.into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    d.add(&Space::Model, Common::default(), EntityKind::MText(t)).unwrap();
}

/// The numbers of attribute `name="…"` of the first element carrying it.
fn attr(s: &str, name: &str) -> Vec<f64> {
    let key = format!("{name}=\"");
    let rest = s.split(&key).nth(1).unwrap_or_default();
    let val = rest.split('"').next().unwrap_or_default();
    val.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).filter_map(|t| t.parse().ok()).collect()
}

/// Guards #1876's main promise: TEXT and every MTEXT line become `<text>` elements with the
/// font family, a positive font size and the glyphs' matrix, and the outlines they replace are
/// not drawn as well.
#[test]
fn text_and_mtext_are_written_as_text() {
    let mut d = drawing();
    text(&mut d, (0.0, 0.0), "ABBA", 10.0);
    mtext(&mut d, (0.0, 20.0), "AB\\PBA");
    let s = svg::export_window_with_text(&d, &Space::Model, None, true, TextMode::Text);

    assert!(s.matches("<text").count() >= 3, "one TEXT and two MTEXT lines: {s}");
    assert!(s.contains(">ABBA</text>"), "{s}");
    assert!(s.contains(">AB</text>") && s.contains(">BA</text>"), "MTEXT lines: {s}");
    assert!(s.contains(r#"font-family="SvgTest""#), "{s}");
    assert!(s.matches("font-size=\"").count() >= 3, "{s}");
    let size = attr(&s, "font-size");
    assert!(size.len() == 1 && size[0].is_finite() && size[0] > 0.0, "{s}");
    // Rotation, size and shear are in the text's own matrix; nothing is positioned by guesswork.
    assert!(s.contains("transform=\"matrix("), "{s}");
    assert!(s.contains(r#"text-anchor="start""#), "{s}");
    // The glyph outlines that the text replaces are not drawn as well.
    assert!(!s.contains("<path"), "no glyph outlines alongside the text: {s}");
}

/// Guards that geometry mode (`text: "geometry"`) writes no `<text>` elements and keeps the glyphs
/// as outline polylines (here with TEXTFILL off), exactly like a plot.
#[test]
fn geometry_mode_keeps_glyph_outlines() {
    let mut d = drawing();
    d.header.set_i64("TEXTFILL", 0);
    text(&mut d, (0.0, 0.0), "ABBA", 10.0);
    mtext(&mut d, (0.0, 20.0), "AB\\PBA");
    let s = svg::export_window_with_text(&d, &Space::Model, None, true, TextMode::Geometry);

    assert!(!s.contains("<text"), "geometry mode writes no text: {s}");
    assert!(s.contains("<polyline"), "glyph outlines stay polylines: {s}");
}

/// Guards that a rotated TEXT keeps the space between its words in the run's characters and puts
/// the rotation (with SVG's flipped y axis) in the text's matrix rather than guessed positions.
#[test]
fn rotation_and_spaces_are_kept() {
    let mut d = drawing();
    let t = Text {
        insert: Vec3::new(0.0, 0.0, 0.0),
        align_pt: None,
        height: 10.0,
        value: "A B".into(),
        rotation: 0.5,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Real".into(),
        halign: Default::default(),
        valign: Default::default(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::Text(t)).unwrap();
    let s = svg::export_window_with_text(&d, &Space::Model, None, true, TextMode::Text);
    assert!(s.contains(">A B</text>"), "the space between the words is kept: {s}");
    let m: Vec<f64> = s
        .split("matrix(")
        .nth(1)
        .unwrap_or_default()
        .split(')')
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|v| v.parse().ok())
        .collect();
    assert_eq!(m.len(), 6, "{s}");
    // The baseline rotation is in the matrix (SVG's y axis is down).
    assert!((m[0] - 0.5f64.cos()).abs() < 1e-5 && (m[1] + 0.5f64.sin()).abs() < 1e-5, "{m:?}");
    assert!((m[2] + 0.5f64.sin()).abs() < 1e-5 && (m[3] + 0.5f64.cos()).abs() < 1e-5, "{m:?}");
}

/// Guards that `&`, `<` and `>` in drawing text are XML-escaped inside `<text>`, so a drawing
/// with those characters still yields a well-formed document.
#[test]
fn special_characters_are_escaped() {
    let mut d = drawing();
    text(&mut d, (0.0, 0.0), "A&B<A>", 10.0);
    let s = svg::export_window_with_text(&d, &Space::Model, None, true, TextMode::Text);
    assert!(s.contains(">A&amp;B&lt;A&gt;</text>"), "{s}");
    assert!(!s.contains(">A&B<A></text>"), "unescaped text: {s}");
}

/// Guards that hostile text heights (zero, negative, NaN, infinite, huge, tiny) and control
/// characters still produce a complete SVG instead of panicking.
#[test]
fn hostile_text_never_panics() {
    let mut d = drawing();
    for (i, h) in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e300, 1e-300].into_iter().enumerate() {
        text(&mut d, (i as f64 * 20.0, 0.0), "AB\u{0}\u{1}", h);
    }
    let s = svg::export_window_with_text(&d, &Space::Model, None, true, TextMode::Text);
    assert!(s.starts_with("<svg") && s.ends_with("</svg>"), "{s}");
}

/// Guards that `export_with` reads the `text` option for SVG (text by default, glyph outlines for
/// `"geometry"`) and that formats such as DXF ignore it.
#[test]
fn the_export_json_option_selects_the_mode() {
    let mut d = drawing();
    text(&mut d, (0.0, 0.0), "ABBA", 10.0);
    let plain = cadcraft_io::export_with(&d, "a.svg", &json!({})).unwrap();
    assert!(String::from_utf8_lossy(&plain).contains("<text"));
    let outlines = cadcraft_io::export_with(&d, "a.svg", &json!({ "text": "geometry" })).unwrap();
    assert!(!String::from_utf8_lossy(&outlines).contains("<text"));
    // Other formats ignore the option.
    assert!(cadcraft_io::export_with(&d, "a.dxf", &json!({ "text": "geometry" })).is_ok());
}
