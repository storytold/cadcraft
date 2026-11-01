//! Angular extension lines stay with their legs when the arc is on the other side, and the
//! DIMCEN centre mark is drawn only when the dimension line is outside the circle (issue #374).

use cadcraft_doc::{DimKind, DimStyle, Dimension};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_render::{LineRole, dimension_geometry};

fn dim(kind: DimKind) -> Dimension {
    Dimension {
        kind,
        defpt: Vec3::ZERO,
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::ZERO,
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    }
}

fn ext_lines(d: &Dimension, st: &DimStyle) -> Vec<Vec<Vec2>> {
    dimension_geometry(d, st, 1.0).lines_of(LineRole::Ext).cloned().collect()
}

fn near(a: Vec2, b: Vec2) -> bool {
    a.dist(b) < 1e-9
}

#[test]
fn angular_extension_lines_follow_their_legs_when_the_arc_is_swapped() {
    // Vertex 0,0; leg 1 to 10,0 (reaches past the arc), leg 2 to 0,2 (short of it); the arc
    // location -3,-3 is outside the 0°→90° sweep, so the 270° arc runs from leg 2 to leg 1.
    let mut a = dim(DimKind::Angular3P);
    a.p13 = Vec3::new(10.0, 0.0, 0.0);
    a.p14 = Vec3::new(0.0, 2.0, 0.0);
    a.defpt = Vec3::new(-3.0, -3.0, 0.0);
    let st = DimStyle::default();
    assert_eq!(dimension_geometry(&a, &st, 1.0).value, "270°");
    let r = 18f64.sqrt();
    // Only leg 2 needs an extension line: from DIMEXO past 0,2 to DIMEXE past the arc.
    let lines = ext_lines(&a, &st);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(near(lines[0][0], Vec2::new(0.0, 2.0 + st.ext_offset)) && near(lines[0][1], Vec2::new(0.0, r + st.ext_extend)), "{lines:?}");
    // DIMSE1 belongs to leg 1 (which has none), DIMSE2 to leg 2.
    assert_eq!(ext_lines(&a, &DimStyle { suppress_ext1: true, ..DimStyle::default() }), lines);
    assert!(ext_lines(&a, &DimStyle { suppress_ext2: true, ..DimStyle::default() }).is_empty());
    // DIMBLK1 sits on leg 1's end of the arc.
    let dot1 = DimStyle { arrow_block1: "_DOT".into(), arrow_block2: "_NONE".into(), ..DimStyle::default() };
    let g = dimension_geometry(&a, &dot1, 1.0);
    assert!(!g.fills.is_empty() && g.fills.iter().flatten().all(|p| p.dist(Vec2::new(r, 0.0)) < st.arrow_size), "{:?}", g.fills.first());
}

#[test]
fn centre_mark_only_when_the_dimension_line_is_outside_the_circle() {
    let st = DimStyle::default();
    // Radius led out to its text: the mark is drawn.
    let mut rad = dim(DimKind::Radius);
    rad.p15 = Vec3::new(5.0, 0.0, 0.0);
    assert_eq!(ext_lines(&rad, &st).len(), 2, "centre mark of an outside radius dimension");
    // Text moved inside the circle: the dimension line is inside, no mark.
    rad.user_text_pos = true;
    rad.text_mid = Vec3::new(2.0, 0.0, 0.0);
    assert!(ext_lines(&rad, &st).is_empty());
    // A diameter with DIMTOFL runs through the centre: no mark, nor centre lines (negative
    // DIMCEN). Without DIMTOFL it is only led out to its text, like a radius, and has the mark.
    let mut dia = dim(DimKind::Diameter);
    dia.defpt = Vec3::new(-5.0, 0.0, 0.0);
    dia.p15 = Vec3::new(5.0, 0.0, 0.0);
    let tofl = DimStyle { force_line_inside: true, ..DimStyle::default() };
    assert!(ext_lines(&dia, &tofl).is_empty());
    assert!(ext_lines(&dia, &DimStyle { center_mark: -0.5, ..tofl.clone() }).is_empty());
    assert_eq!(ext_lines(&dia, &st).len(), 2);
    // A radius with DIMTOFL runs in from the centre: no mark.
    rad.user_text_pos = false;
    assert!(ext_lines(&rad, &tofl).is_empty());
}
