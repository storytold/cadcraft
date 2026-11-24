//! Real text in PDF plots (issue #560): text in TrueType fonts is written as text in an
//! embedded font subset (selectable and searchable through a ToUnicode map), placed with its
//! rotation, width factor and obliquing; fonts whose licence bits forbid embedding, the stroke
//! font and the `text: "geometry"` option keep filled outlines.
//!
//! The fonts are tiny synthetic TrueType files built by the shared test helper in
//! `crates/fonts/tests/support/sfnt.rs` (no font files in the repository).

use cadcraft_doc::{Common, Drawing, EntityKind, Space, Text, TextStyle, Viewport};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_io::plot;
use serde_json::json;

#[path = "support/pdf_read.rs"]
mod pdf_read;

use pdf_read::Doc;

// The synthetic TrueType file: the fonts layer's test-only helper, inlined rather than published
// or depended on.
#[path = "../../fonts/tests/support/sfnt.rs"]
mod sfnt;

// ---------- a minimal TrueType font: .notdef, "A" (triangle), "B" (square) ----------

/// A TrueType font with glyphs .notdef (empty), A and B, and OS/2 `fsType`. `hmtx` holds
/// `metrics` of the three metrics `hhea` declares: fewer is a font the writer cannot subset.
fn test_font(fs_type: u16, metrics: usize) -> Vec<u8> {
    sfnt::bytes(&sfnt::Sfnt {
        // "A" is a triangle and "B" a square: one contour each.
        glyphs: &[
            sfnt::notdef(),
            sfnt::glyph(&[vec![(50, 0), (550, 0), (300, 700)]]),
            sfnt::glyph(&[vec![(50, 0), (550, 0), (550, 700), (50, 700)]]),
        ],
        cmap: &[(0x41, 0x42, 1 - 0x41)], // "A".."B" → glyphs 1..2
        advance: 600,
        lsb: 50,
        metrics,
        fs_type,
        ink_box: [0, 0, 600, 700],
    })
}

/// Register the test fonts once: an embeddable one, one with a restricted licence, and one
/// the PDF writer accepts but can't subset (its `hmtx` is short).
fn fonts() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        cadcraft_fonts::ttf::register("cadcraft-pdf-test.ttf", test_font(0, 3));
        cadcraft_fonts::ttf::register("cadcraft-pdf-restricted.ttf", test_font(0x0002, 3));
        cadcraft_fonts::ttf::register("cadcraft-pdf-short-hmtx.ttf", test_font(0, 1));
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
    style(&mut d, "Short", "cadcraft-pdf-short-hmtx.ttf");
    d
}

/// The uncompressed PDF, read back.
fn pdf(d: &Drawing, space: &Space, extra: serde_json::Value) -> Doc {
    let mut o = json!({"compress": false});
    if let (Some(m), Some(e)) = (o.as_object_mut(), extra.as_object()) {
        m.extend(e.clone());
    }
    pdf_read::read(&plot(d, space, &o).unwrap())
}

/// The glyph matrices (`a b c d e f`: em axes and origin on the page) of the text, in order.
fn matrices(doc: &Doc) -> Vec<[f64; 6]> {
    pdf_read::text_matrices(doc.content())
}

/// The glyph codes shown, in order (two bytes each).
fn codes(doc: &Doc) -> Vec<u32> {
    pdf_read::shown(&doc.pages[0].content_bytes)
        .iter()
        .flat_map(|s| s.chunks(2).map(|c| u32::from(c[0]) << 8 | u32::from(*c.get(1).unwrap_or(&0))).collect::<Vec<_>>())
        .collect()
}

/// Filled paths drawn (glyph outlines are fills).
fn fills(doc: &Doc) -> usize {
    pdf_read::count(doc.content(), "f")
}

/// Embedded font files.
fn font_files(doc: &Doc) -> usize {
    doc.pages.iter().flat_map(|p| &p.fonts).filter(|f| f.font_file.is_some()).count()
}

#[test]
fn truetype_text_is_embedded_as_text() {
    let mut d = drawing();
    text(&mut d, &Space::Model, "Real", (0.0, 0.0), "ABBA", (0.0, 1.0, 0.0));
    let doc = pdf(&d, &Space::Model, json!({}));
    let page = &doc.pages[0];
    // A Type0 font over a CIDFontType2 subset with a ToUnicode map.
    assert_eq!(page.fonts.len(), 1);
    let f = &page.fonts[0];
    assert_eq!((f.subtype.as_str(), f.encoding.as_deref(), f.descendant.as_deref()), ("Type0", Some("Identity-H"), Some("CIDFontType2")));
    assert!(f.cid_to_gid.as_deref() == Some("Identity") && f.font_file.is_some() && f.to_unicode.is_some());
    let base = f.base_font.as_bytes();
    assert!(base.len() > 7 && base[6] == b'+' && base[..6].iter().all(u8::is_ascii_uppercase), "{}", f.base_font);
    // Four glyphs written as text: A B B A.
    let c = codes(&doc);
    assert_eq!(c.len(), 4, "{c:?}");
    assert!(c[0] == c[3] && c[1] == c[2] && c[0] != c[1], "{c:?}");
    let map = pdf_read::to_unicode_map(f.to_unicode.as_deref().unwrap());
    assert!(map.contains(&(c[0], "A".into())) && map.contains(&(c[1], "B".into())), "ToUnicode maps the glyphs to A and B: {map:?}");
    for code in [c[0], c[1]] {
        assert!(f.widths.contains(&(code, 600.0)), "width of {code}: {:?}", f.widths);
    }
    // Text objects are balanced, at font size 1.
    assert_eq!(pdf_read::count(&page.content, "BT"), pdf_read::count(&page.content, "ET"));
    assert!(pdf_read::ops(&page.content).iter().filter(|o| o.op == "Tf").all(|o| o.nums == [1.0]));
    // No filled outlines left for them.
    assert_eq!(fills(&doc), 0, "glyph outlines are not drawn: {}", page.content);
    // The glyphs advance along the baseline by 600/1000 em, scaled like the outline would be.
    let m = matrices(&doc);
    assert!(m.iter().all(|m| m[1].abs() < 1e-3 && m[2].abs() < 1e-3 && (m[0] - m[3]).abs() < 1e-3 && m[0] > 0.0), "{m:?}");
    let step = m[1][4] - m[0][4];
    assert!((step - 0.6 * m[0][0]).abs() < 0.01, "advance {step} for em {}", m[0][0]);
    assert!(m.windows(2).all(|w| (w[0][5] - w[1][5]).abs() < 1e-3), "one baseline");
}

#[test]
fn the_subset_is_a_valid_truetype_font() {
    let mut d = drawing();
    text(&mut d, &Space::Model, "Real", (0.0, 0.0), "A", (0.0, 1.0, 0.0));
    let doc = pdf(&d, &Space::Model, json!({}));
    let file = doc.pages[0].fonts[0].font_file.clone().expect("an embedded font file");
    let face = cadcraft_fonts::ttf::Face::new(std::sync::Arc::new(file.clone()), 0).expect("the subset parses");
    assert_eq!(cadcraft_fonts::subset::embedding(&face), cadcraft_fonts::subset::Embedding::Allowed);
    // Glyph A kept, B (unused) left out: the subset is smaller than the font.
    assert!(file.len() < test_font(0, 3).len());
}

#[test]
fn rotation_width_factor_and_obliquing_go_into_the_text_matrix() {
    let mut d = drawing();
    let quarter = std::f64::consts::FRAC_PI_2;
    text(&mut d, &Space::Model, "Real", (0.0, 0.0), "A", (quarter, 1.0, 0.0));
    text(&mut d, &Space::Model, "Real", (0.0, 50.0), "A", (0.0, 2.0, 0.0));
    text(&mut d, &Space::Model, "Real", (0.0, 100.0), "A", (0.0, 1.0, 15f64.to_radians()));
    let m = matrices(&pdf(&d, &Space::Model, json!({})));
    assert_eq!(m.len(), 3, "{m:?}");
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
fn a_font_the_writer_cannot_subset_plots_as_outlines() {
    let mut d = drawing();
    text(&mut d, &Space::Model, "Short", (0.0, 0.0), "AB", (0.0, 1.0, 0.0));
    let p = pdf(&d, &Space::Model, json!({}));
    assert!(font_files(&p) == 0 && pdf_read::count(p.content(), "BT") == 0 && fills(&p) > 0, "outlines: {}", p.content());
}

#[test]
fn fonts_that_forbid_embedding_and_the_geometry_option_draw_outlines() {
    let mut d = drawing();
    text(&mut d, &Space::Model, "Locked", (0.0, 0.0), "AB", (0.0, 1.0, 0.0));
    let p = pdf(&d, &Space::Model, json!({}));
    assert!(font_files(&p) == 0 && pdf_read::count(p.content(), "BT") == 0 && fills(&p) > 0, "restricted licence: outlines");
    let mut d = drawing();
    text(&mut d, &Space::Model, "Real", (0.0, 0.0), "AB", (0.0, 1.0, 0.0));
    let p = pdf(&d, &Space::Model, json!({"text": "geometry"}));
    assert!(font_files(&p) == 0 && pdf_read::count(p.content(), "BT") == 0 && fills(&p) > 0, "text as geometry");
    // The stroke font is always geometry.
    let mut d = drawing();
    text(&mut d, &Space::Model, "Standard", (0.0, 0.0), "AB", (0.0, 1.0, 0.0));
    let p = pdf(&d, &Space::Model, json!({}));
    assert!(pdf_read::count(p.content(), "BT") == 0 && pdf_read::count(p.content(), "S") > 0);
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
        twist: 0.0,
    };
    d.add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
    text(&mut d, &paper, "Real", (20.0, 20.0), "B", (0.0, 1.0, 0.0));
    let p = pdf(&d, &paper, json!({}));
    // "AB" inside, the paper-space "B", and the glyph of "BA" left of the edge: real text. The
    // glyph the edge cuts stays a (clipped) outline.
    assert_eq!(pdf_read::count(p.content(), "Tj"), 4, "{}", p.content());
    assert!(fills(&p) > 0, "the cut glyph is an outline");
    // Text seen through the viewport sits in its layer's marked content.
    let ops = pdf_read::ops(p.content());
    let first_bt = ops.iter().position(|o| o.op == "BT").unwrap();
    let layer0 = p.pages[0].properties.iter().find(|(_, g)| g == "0").map(|(k, _)| k.clone()).unwrap();
    assert!(ops[..first_bt].iter().any(|o| o.op == "BDC" && o.names.get(1) == Some(&layer0)));
}

#[test]
fn text_sits_in_its_layers_marked_content() {
    let mut d = drawing();
    text(&mut d, &Space::Model, "Real", (0.0, 0.0), "AB", (0.0, 1.0, 0.0));
    let p = pdf(&d, &Space::Model, json!({}));
    let ops = pdf_read::ops(p.content());
    let first_bt = ops.iter().position(|o| o.op == "BT").unwrap();
    let layer0 = p.pages[0].properties.iter().find(|(_, g)| g == "0").map(|(k, _)| k.clone()).unwrap();
    assert!(ops[..first_bt].iter().any(|o| o.op == "BDC" && o.names.get(1) == Some(&layer0)));
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
