//! A PDF underlay's frame is snapped to while PDFFRAME shows it, and not when it is hidden (0).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, EntityKind, Insert, PdfUnderlay, Space};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::snap::{self, mode};
use serde_json::json;

/// The endpoint snap near `p`, in a drawing with a 297 × 210 underlay at the origin and a
/// reference at (1000, 0) of block "B", holding the same underlay.
fn end_near(frame: i64, p: Vec2) -> Option<Vec2> {
    let mut s = Session::new();
    let u = || EntityKind::PdfUnderlay(PdfUnderlay { size: Vec2::new(297.0, 210.0), scale: 1.0, ..PdfUnderlay::default() });
    let h = s.doc_mut().unwrap().add(&Space::Model, Common::default(), u()).unwrap().hex();
    s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [h], "keep": "delete" })).unwrap();
    let d = s.doc_mut().unwrap();
    d.add(&Space::Model, Common::default(), u()).unwrap();
    let ins = Insert {
        block: "B".into(),
        insert: Vec3::new(1000.0, 0.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
    d.header.set_i64("PDFFRAME", frame);
    snap::osnap(d, &Space::Model, p + Vec2::new(0.1, 0.1), 0.5, mode::END, None, false).map(|h| h.point)
}

#[test]
fn a_shown_frame_is_snapped_to_and_a_hidden_one_is_not() {
    for corner in [Vec2::new(297.0, 210.0), Vec2::new(1297.0, 210.0)] {
        for frame in [1, 2] {
            assert_eq!(end_near(frame, corner), Some(corner), "PDFFRAME {frame} at {corner:?}");
        }
        assert_eq!(end_near(0, corner), None, "PDFFRAME 0 at {corner:?}");
    }
}

/// The endpoint snap near `p`, in a drawing whose only dimension has block "B" (holding a
/// 297 × 210 underlay at the origin) as the block it draws.
fn end_near_in_dimension(frame: i64, p: Vec2) -> Option<Vec2> {
    let mut s = Session::new();
    let u = EntityKind::PdfUnderlay(PdfUnderlay { size: Vec2::new(297.0, 210.0), scale: 1.0, ..PdfUnderlay::default() });
    let h = s.doc_mut().unwrap().add(&Space::Model, Common::default(), u).unwrap().hex();
    s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [h], "keep": "delete" })).unwrap();
    s.execute("dimlinear", &json!({ "p1": [2000, 0], "p2": [2100, 0], "at": [2050, 50] })).unwrap();
    let d = s.doc_mut().unwrap();
    let dim = d.model.iter().find(|e| matches!(e.kind, EntityKind::Dimension(_))).unwrap().handle;
    d.modify_entity(dim, |e| {
        if let EntityKind::Dimension(dm) = &mut e.kind {
            dm.block = Some("B".into());
        }
    })
    .unwrap();
    d.header.set_i64("PDFFRAME", frame);
    snap::osnap(d, &Space::Model, p + Vec2::new(0.1, 0.1), 0.5, mode::END, None, false).map(|h| h.point)
}

#[test]
fn a_frame_in_a_dimension_block_is_snapped_to_only_when_shown() {
    let corner = Vec2::new(297.0, 210.0);
    assert_eq!(end_near_in_dimension(1, corner), Some(corner), "PDFFRAME 1");
    assert_eq!(end_near_in_dimension(0, corner), None, "PDFFRAME 0");
}
