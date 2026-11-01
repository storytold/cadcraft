//! A negative DIMGAP draws a frame around the dimension text. The `*D` block written for the
//! dimension holds the frame's four sides, the negative DIMGAP reads back from the file, and a
//! PDF plot draws the frame.

use cadcraft_doc::{Common, DimKind, Dimension, Drawing, EntityKind, Space};
use cadcraft_geom::Vec3;

use crate::pdf_read;

fn linear() -> Dimension {
    Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(15.0, 5.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(30.0, 0.0, 0.0),
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

/// Save a drawing with one dimension in a Standard style of DIMGAP `gap`, read it back, and
/// return the style's DIMGAP and the number of LINEs in the dimension's `*D` block.
fn roundtrip(gap: f64) -> (f64, usize) {
    let mut d = Drawing::new_imperial();
    d.dim_styles.iter_mut().for_each(|s| s.text_gap = gap);
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(linear())).unwrap();
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    let dm = back
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Dimension(dm) => Some(dm.clone()),
            _ => None,
        })
        .unwrap();
    let block = back.block(dm.block.as_deref().unwrap()).unwrap();
    let lines = block.entities.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
    (back.dim_style("Standard").unwrap().text_gap, lines)
}

/// The number of path segments (`l`) in the PDF of a drawing with one dimension in a Standard
/// style of DIMGAP `gap`.
fn pdf_segments(gap: f64) -> usize {
    let mut d = Drawing::new_imperial();
    d.dim_styles.iter_mut().for_each(|s| s.text_gap = gap);
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(linear())).unwrap();
    let doc = pdf_read::read(&cadcraft_io::plot(&d, &Space::Model, &serde_json::json!({"compress": false})).unwrap());
    pdf_read::count(&doc.pages[0].content, "l")
}

#[test]
fn pdf_draws_the_text_frame() {
    assert_eq!(pdf_segments(-0.09), pdf_segments(0.09) + 4);
}

#[test]
fn dimension_block_holds_the_text_frame() {
    let (gap, plain) = roundtrip(0.09);
    assert!((gap - 0.09).abs() < 1e-12, "{gap}");
    let (gap, framed) = roundtrip(-0.09);
    assert!((gap + 0.09).abs() < 1e-12, "{gap}");
    assert_eq!(framed, plain + 4);
}
