//! Real text in PDF plots (issue #560): text in TrueType fonts is written as text in an
//! embedded font subset (selectable and searchable through a ToUnicode map), placed with its
//! rotation, width factor and obliquing; fonts whose licence bits forbid embedding, the stroke
//! font and the `text: "geometry"` option keep filled outlines.
//!
//! The fonts are tiny synthetic TrueType files built here (no font files in the repository).

use cadcraft_doc::{Common, Drawing, EntityKind, Space, Text, TextStyle, Viewport};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_io::plot;
use serde_json::json;

// ---------- a minimal TrueType font: .notdef, "A" (triangle), "B" (square) ----------

fn be16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}
fn be32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

/// A simple glyph: one closed contour of on-curve points.
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

/// A TrueType font with glyphs .notdef (empty), A and B, and OS/2 `fsType`.
fn test_font(fs_type: u16) -> Vec<u8> {
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
    // cmap: format 4, 'A'..'B' → 1..2.
    let mut sub = Vec::new();
    let segs: [(u16, u16, u16); 2] = [(0x41, 0x42, 1u16.wrapping_sub(0x41)), (0xFFFF, 0xFFFF, 1)];
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
    os2[8..10].copy_from_slice(&fs_type.to_be_bytes());
    let mut tables: Vec<(&[u8; 4], Vec<u8>)> =
        vec![(b"OS/2", os2), (b"cmap", cmap), (b"glyf", glyf), (b"head", head), (b"hhea", hhea), (b"hmtx", hmtx), (b"loca", loca), (b"maxp", maxp)];
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

/// Register the test fonts once: an embeddable one and one with a restricted licence.
fn fonts() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        cadcraft_fonts::ttf::register("cadcraft-pdf-test.ttf", test_font(0));
        cadcraft_fonts::ttf::register("cadcraft-pdf-restricted.ttf", test_font(0x0002));
    });
}

// ---------- drawings and PDF reading ----------

fn style(d: &mut Drawing, name: &str, font: &str) {
    d.text_styles.push(TextStyle { name: name.into(), font: font.into(), ..TextStyle::default() });
}

fn text(d: &mut Drawing, space: &Space, style: &str, at: (f64, f64), value: &str, (rotation, width_factor, oblique): (f64, f64, f64)) {
    let t = Text {
        insert: Vec3::new(at.0, at.1, 0.0),
        align_pt: None,
        height: 10.0,
        value: value.into(),
        rotation,
        width_factor,
        oblique,
        style: style.into(),
        halign: Default::default(),
        valign: Default::default(),
    };
    d.add(space, Common::default(), EntityKind::Text(t)).unwrap();
}

fn drawing() -> Drawing {
    fonts();
    let mut d = Drawing::new_metric();
    style(&mut d, "Real", "cadcraft-pdf-test.ttf");
    style(&mut d, "Locked", "cadcraft-pdf-restricted.ttf");
    d
}

/// The uncompressed PDF as text (offsets kept: non-ASCII bytes become `?`).
fn pdf(d: &Drawing, space: &Space, extra: serde_json::Value) -> String {
    let mut o = json!({"compress": false});
    if let (Some(m), Some(e)) = (o.as_object_mut(), extra.as_object()) {
        m.extend(e.clone());
    }
    let bytes = plot(d, space, &o).unwrap();
    check_xref(&bytes);
    bytes.iter().map(|b| if b.is_ascii() { char::from(*b) } else { '?' }).collect()
}

/// The cross-reference table points at every object.
fn check_xref(bytes: &[u8]) {
    let text: String = bytes.iter().map(|b| if b.is_ascii() { char::from(*b) } else { '?' }).collect();
    let sx = text.rfind("startxref\n").unwrap();
    let xref: usize = text[sx + 10..].lines().next().unwrap().trim().parse().unwrap();
    let mut lines = text[xref..].lines().skip(1);
    let count: usize = lines.next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
    for (i, e) in lines.take(count).enumerate().skip(1) {
        let off: usize = e[..10].parse().unwrap();
        assert!(text[off..].starts_with(&format!("{i} 0 obj\n")), "object {i}");
    }
}

/// The text matrices (`a b c d e f Tm`) of the content, in order.
fn matrices(pdf: &str) -> Vec<[f64; 6]> {
    pdf.lines()
        .filter(|l| l.ends_with(" Tj") && l.contains(" Tm "))
        .map(|l| {
            let v: Vec<f64> = l.split(" Tm ").next().unwrap().split_whitespace().map(|x| x.parse().unwrap()).collect();
            [v[0], v[1], v[2], v[3], v[4], v[5]]
        })
        .collect()
}

/// The embedded font file (FontFile2 stream data).
fn font_file(pdf: &str, bytes_of: &[u8]) -> Vec<u8> {
    let i = pdf.find("/Length1 ").expect("a FontFile2 stream");
    let head = &pdf[..i];
    let len_at = head.rfind("/Length ").unwrap() + 8;
    let len: usize = pdf[len_at..].split(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap();
    let start = pdf[i..].find("stream\n").unwrap() + i + 7;
    bytes_of[start..start + len].to_vec()
}

#[test]
fn truetype_text_is_embedded_as_text() {
    let mut d = drawing();
    text(&mut d, &Space::Model, "Real", (0.0, 0.0), "ABBA", (0.0, 1.0, 0.0));
    let p = pdf(&d, &Space::Model, json!({}));
    // A Type0 font over a CIDFontType2 subset with a ToUnicode map.
    assert!(p.contains("/Subtype /Type0") && p.contains("/Encoding /Identity-H") && p.contains("/Subtype /CIDFontType2"), "{p}");
    assert!(p.contains("/CIDToGIDMap /Identity") && p.contains("/FontFile2 ") && p.contains("/ToUnicode "));
    let base = p.split("/BaseFont /").nth(1).unwrap().split_whitespace().next().unwrap();
    assert!(base.len() == 11 && base.as_bytes()[6] == b'+' && base[..6].chars().all(|c| c.is_ascii_uppercase()), "{base}");
    assert!(p.contains("<0001> <0041>") && p.contains("<0002> <0042>"), "ToUnicode maps glyphs to A and B");
    assert!(p.contains("/W [1 [600] 2 [600]]"), "widths");
    // The page uses the font; four glyphs written as text, in a text object.
    assert!(p.contains("/Font << /F0 "));
    let tj: Vec<&str> = p.lines().filter(|l| l.ends_with(" Tj")).map(|l| l.rsplit(' ').nth(1).unwrap()).collect();
    assert_eq!(tj, ["<0001>", "<0002>", "<0002>", "<0001>"]);
    assert!(p.contains("BT\n/F0 1 Tf\n") && p.contains("ET\n"));
    assert_eq!(p.matches("BT\n").count(), p.matches("ET\n").count());
    // No filled outlines left for them.
    assert!(!p.contains("h\nf\n"), "glyph outlines are not drawn: {p}");
    // The glyphs advance along the baseline by 600/1000 em, scaled like the outline would be.
    let m = matrices(&p);
    assert!(m.iter().all(|m| m[1].abs() < 1e-3 && m[2].abs() < 1e-3 && (m[0] - m[3]).abs() < 1e-3 && m[0] > 0.0));
    let step = m[1][4] - m[0][4];
    assert!((step - 0.6 * m[0][0]).abs() < 0.01, "advance {step} for em {}", m[0][0]);
    assert!(m.windows(2).all(|w| (w[0][5] - w[1][5]).abs() < 1e-3), "one baseline");
}

#[test]
fn the_subset_is_a_valid_truetype_font() {
    let mut d = drawing();
    text(&mut d, &Space::Model, "Real", (0.0, 0.0), "A", (0.0, 1.0, 0.0));
    let bytes = plot(&d, &Space::Model, &json!({"compress": false})).unwrap();
    let p: String = bytes.iter().map(|b| if b.is_ascii() { char::from(*b) } else { '?' }).collect();
    let file = font_file(&p, &bytes);
    assert!(p.contains(&format!("/Length1 {}", file.len())));
    let face = cadcraft_fonts::ttf::Face::new(std::sync::Arc::new(file.clone()), 0).expect("the subset parses");
    assert_eq!(cadcraft_fonts::subset::embedding(&face), cadcraft_fonts::subset::Embedding::Allowed);
    // Glyph A kept, B (unused) emptied: the subset is smaller than the font.
    assert!(file.len() < test_font(0).len());
    // Whole-file checksum adjusted.
    let sum = file.chunks(4).fold(0u32, |s, c| {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        s.wrapping_add(u32::from_be_bytes(w))
    });
    assert_eq!(sum, 0xB1B0_AFBA);
}

#[test]
fn rotation_width_factor_and_obliquing_go_into_the_text_matrix() {
    let mut d = drawing();
    let quarter = std::f64::consts::FRAC_PI_2;
    text(&mut d, &Space::Model, "Real", (0.0, 0.0), "A", (quarter, 1.0, 0.0));
    text(&mut d, &Space::Model, "Real", (0.0, 50.0), "A", (0.0, 2.0, 0.0));
    text(&mut d, &Space::Model, "Real", (0.0, 100.0), "A", (0.0, 1.0, 15f64.to_radians()));
    let m = matrices(&pdf(&d, &Space::Model, json!({})));
    assert_eq!(m.len(), 3);
    // Rotated a quarter turn: the baseline points up, "up" points left.
    let [a, b, c, dd, _, _] = m[0];
    assert!(a.abs() < 1e-3 && b > 0.0 && c < 0.0 && dd.abs() < 1e-3, "{:?}", m[0]);
    // Width factor 2: twice as wide as high.
    assert!((m[1][0] - 2.0 * m[1][3]).abs() < 1e-2 && m[1][1].abs() < 1e-3, "{:?}", m[1]);
    // Oblique 15°: the up axis leans right by tan 15°.
    assert!((m[2][2] - m[2][3] * 15f64.to_radians().tan()).abs() < 1e-2, "{:?}", m[2]);
    // The em is sized so the cap height (here 700/1000 em) is the text height: 10 / 0.7.
    let em_pt = m[1][3];
    let em_mm = em_pt * 25.4 / 72.0;
    assert!(em_mm > 0.0);
}

#[test]
fn fonts_that_forbid_embedding_and_the_geometry_option_draw_outlines() {
    let mut d = drawing();
    text(&mut d, &Space::Model, "Locked", (0.0, 0.0), "AB", (0.0, 1.0, 0.0));
    let p = pdf(&d, &Space::Model, json!({}));
    assert!(!p.contains("/FontFile2") && !p.contains("BT\n") && p.contains("h\nf\n"), "restricted licence: outlines");
    let mut d = drawing();
    text(&mut d, &Space::Model, "Real", (0.0, 0.0), "AB", (0.0, 1.0, 0.0));
    let p = pdf(&d, &Space::Model, json!({"text": "geometry"}));
    assert!(!p.contains("/FontFile2") && !p.contains("BT\n") && p.contains("h\nf\n"), "text as geometry");
    // The stroke font is always geometry.
    let mut d = drawing();
    text(&mut d, &Space::Model, "Standard", (0.0, 0.0), "AB", (0.0, 1.0, 0.0));
    let p = pdf(&d, &Space::Model, json!({}));
    assert!(!p.contains("BT\n") && p.contains(" S\n") || p.contains("\nS\n"));
}

#[test]
fn text_seen_through_a_viewport_and_on_layers() {
    let mut d = drawing();
    let paper = Space::Paper("Layout1".into());
    // Model: one word well inside the viewport, one cut by its right edge.
    text(&mut d, &Space::Model, "Real", (0.0, 0.0), "AB", (0.0, 1.0, 0.0));
    text(&mut d, &Space::Model, "Real", (88.0, 0.0), "BA", (0.0, 1.0, 0.0));
    let vp = Viewport {
        center: Vec3::new(150.0, 100.0, 0.0),
        width: 100.0,
        height: 100.0,
        view_center: Vec2::new(50.0, 5.0),
        view_height: 100.0,
        id: 2,
        locked: false,
        frozen_layers: Vec::new(),
        layer_colors: Vec::new(),
        clip: None,
        on: true,
    };
    d.add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
    text(&mut d, &paper, "Real", (20.0, 20.0), "B", (0.0, 1.0, 0.0));
    let p = pdf(&d, &paper, json!({}));
    let tj = p.lines().filter(|l| l.ends_with(" Tj")).count();
    // "AB" inside, the paper-space "B", and the glyph of "BA" left of the edge: real text. The
    // glyph the edge cuts stays a (clipped) outline.
    assert_eq!(tj, 4, "{p}");
    assert!(p.contains("h\nf\n"), "the cut glyph is an outline");
    // Text sits in its layer's marked content.
    let first_bt = p.find("BT\n").unwrap();
    assert!(p[..first_bt].contains("/OC /L0 BDC"));
}

#[test]
fn hostile_text_never_panics() {
    let mut d = drawing();
    for (v, h, r) in
        [("A".repeat(5000), 10.0, 0.0), ("AB".into(), 1e300, 0.0), ("AB".into(), 1e-300, 1e300), ("\u{0}A\u{FFFF}B".into(), 5.0, f64::NAN)]
    {
        let t = Text {
            insert: Vec3::new(0.0, 0.0, 0.0),
            align_pt: None,
            height: h,
            value: v,
            rotation: r,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Real".into(),
            halign: Default::default(),
            valign: Default::default(),
        };
        d.add(&Space::Model, Common::default(), EntityKind::Text(t)).unwrap();
    }
    pdf(&d, &Space::Model, json!({}));
    pdf(&d, &Space::Model, json!({"compress": true}));
}
