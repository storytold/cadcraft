//! Left-justified TEXT with a vertical alignment (TL, ML, BL) is placed by its alignment point
//! (DXF group 11), like every justification but baseline-left; group 10 is then a computed point
//! on the baseline (#342).

use cadcraft_doc::{EntityKind, Space, entity_bounds};

/// One TEXT, height 1, "AB", group 72 = 0 and group 73 = `v`, with group 10 at `p10`, group 11 at
/// `p11`, and `extra` group codes.
fn dxf(v: i32, p10: (f64, f64), p11: (f64, f64), extra: &str) -> String {
    format!(
        "0\nSECTION\n2\nENTITIES\n0\nTEXT\n8\n0\n10\n{}\n20\n{}\n40\n1\n1\nAB\n72\n0\n11\n{}\n21\n{}\n{extra}73\n{v}\n0\nENDSEC\n0\nEOF\n",
        p10.0, p10.1, p11.0, p11.1
    )
}

/// (rendered box, extents box) of the drawing's only object, as (min x, min y, max x, max y).
fn boxes(text: &str) -> ([f64; 4], [f64; 4]) {
    let d = cadcraft_io::read(text.as_bytes(), "t.dxf").unwrap();
    let r = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default()).bounds;
    let e = d.model.iter().find(|e| matches!(e.kind, EntityKind::Text(_))).map(|e| entity_bounds(&d, e, 0)).unwrap();
    ([r.min.x, r.min.y, r.max.x, r.max.y], [e.min.x, e.min.y, e.max.x, e.max.y])
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn left_justified_text_is_placed_by_its_alignment_point() {
    // TL: the top-left of the text at the alignment point 0,0; the baseline one height lower.
    let (r, e) = boxes(&dxf(3, (0.0, -1.0), (0.0, 0.0), ""));
    assert!(near(r[0], 0.0) && near(r[3], 0.0), "TL drawn at {r:?}");
    assert!(near(e[0], 0.0) && near(e[3], 0.0) && near(e[1], -1.0), "TL extents {e:?}");
    // ML: the middle of the text height at the alignment point.
    let (r, e) = boxes(&dxf(2, (0.0, -0.5), (0.0, 0.0), ""));
    assert!(near(r[3], 0.5) && near(e[3], 0.5) && near(e[1], -0.5), "ML drawn at {r:?}, extents {e:?}");
    // BL: the descender line at the alignment point, so the baseline ("AB" has no descenders) a
    // third of the height above it.
    let (r, e) = boxes(&dxf(1, (0.0, 1.0 / 3.0), (0.0, 0.0), ""));
    assert!(near(r[1], 1.0 / 3.0) && near(e[1], 1.0 / 3.0) && near(e[3], 4.0 / 3.0), "BL drawn at {r:?}, extents {e:?}");

    // Mirrored (extrusion 0,0,-1): the OCS points (-10,4) and (-10,5) are WCS (10,4) and (10,5);
    // drawn readable, the text runs left from its alignment point with its top there.
    let (r, _) = boxes(&dxf(3, (-10.0, 4.0), (-10.0, 5.0), "210\n0\n220\n0\n230\n-1\n"));
    assert!(near(r[2], 10.0) && near(r[3], 5.0), "mirrored TL drawn at {r:?}");
}

#[test]
fn dxf_export_baseline_respects_fontalt() {
    // Skip if Arial is not installed on the host system (e.g. headless Linux container / CI)
    if cadcraft_fonts::ttf::find("Arial").is_none() {
        eprintln!("Skipping dxf_export_baseline_respects_fontalt: Arial not found on host.");
        return;
    }

    use cadcraft_doc::{Drawing, Entity, HAlign, Handle, Text, TextStyle, VAlign};
    use cadcraft_geom::Vec3;

    struct AltGuard(String);
    impl Drop for AltGuard {
        fn drop(&mut self) {
            cadcraft_fonts::ttf::set_font_alt(&self.0);
        }
    }
    let _guard = AltGuard(cadcraft_fonts::ttf::font_alt());

    let mut d = Drawing::new_metric();
    d.text_styles.push(TextStyle { name: "MissingFont".into(), font: "non_existent_font_xyz.ttf".into(), ..TextStyle::default() });

    let make_text = || {
        let t = Text {
            insert: Vec3::new(-999.0, -999.0, 0.0),
            align_pt: Some(Vec3::new(100.0, 20.0, 0.0)),
            height: 2.5,
            value: "Sample Text".into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "MissingFont".into(),
            halign: HAlign::Right,
            valign: VAlign::Baseline,
        };
        Entity { handle: Handle(10), common: Default::default(), kind: EntityKind::Text(t) }
    };

    // Case 1: Without FONTALT, missing font falls back to stroke font
    cadcraft_fonts::ttf::set_font_alt("");
    let (_, tags_stroke) = cadcraft_io::entity_groups(&d, &make_text());
    let x_stroke = tags_stroke.iter().find(|t| t.code == 10).map(|t| t.value.as_f64()).unwrap();

    // Case 2: With FONTALT set to an installed TrueType font (Arial)
    cadcraft_fonts::ttf::set_font_alt("Arial");
    let (_, tags_fontalt) = cadcraft_io::entity_groups(&d, &make_text());
    let x_fontalt = tags_fontalt.iter().find(|t| t.code == 10).map(|t| t.value.as_f64()).unwrap();

    assert!(
        (x_stroke - x_fontalt).abs() > 0.01,
        "Stroke font and Arial FONTALT should produce distinct baseline x for right alignment (stroke={x_stroke}, fontalt={x_fontalt})"
    );
}
