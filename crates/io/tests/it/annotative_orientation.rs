//! Annotative TEXT read from a file with "Match orientation to layout" on (`AcadAnnoPO` xdata = 1)
//! is drawn and measured upright, ignoring its stored rotation, as AutoCAD shows it (#2498).

use cadcraft_doc::{EntityKind, Space, entity_bounds};

/// The xdata AutoCAD writes on such text: the `AcadAnnotative` data, then `AcadAnnoPO`.
const ANNO_PO: &str = "1001\nAcadAnnotative\n1000\nAnnotativeData\n1002\n{\n1070\n1\n1070\n1\n1002\n}\n1001\nAcadAnnoPO\n1070\n1\n";

/// One TEXT "TITLE" at 10,5, height 2.5, rotated `deg` degrees, followed by `xdata`.
fn dxf(deg: f64, xdata: &str) -> String {
    format!("0\nSECTION\n2\nENTITIES\n0\nTEXT\n5\n2A\n8\n0\n10\n10\n20\n5\n40\n2.5\n1\nTITLE\n50\n{deg}\n{xdata}0\nENDSEC\n0\nEOF\n")
}

/// (rendered box, extents box) of the drawing's only object, as (min x, min y, max x, max y).
fn boxes(text: &str) -> ([f64; 4], [f64; 4]) {
    let d = cadcraft_io::read(text.as_bytes(), "t.dxf").unwrap();
    let r = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default()).bounds;
    let e = d.model.iter().find(|e| matches!(e.kind, EntityKind::Text(_))).map(|e| entity_bounds(&d, e, 0)).unwrap();
    ([r.min.x, r.min.y, r.max.x, r.max.y], [e.min.x, e.min.y, e.max.x, e.max.y])
}

fn same(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

#[test]
fn match_orientation_to_layout_draws_the_text_upright() {
    let d = cadcraft_io::read(dxf(180.0, ANNO_PO).as_bytes(), "t.dxf").unwrap();
    assert!(d.model.iter().any(|e| e.common.matches_layout_orientation()), "the AcadAnnoPO flag is read");
    let (upright_r, upright_e) = boxes(&dxf(0.0, ""));
    let (r, e) = boxes(&dxf(180.0, ANNO_PO));
    assert!(same(r, upright_r), "drawn at {r:?}, upright at {upright_r:?}");
    assert!(same(e, upright_e), "extents {e:?}, upright {upright_e:?}");
}

#[test]
fn without_the_flag_the_stored_rotation_applies() {
    let (upright_r, upright_e) = boxes(&dxf(0.0, ""));
    let off = ANNO_PO.replace("AcadAnnoPO\n1070\n1\n", "AcadAnnoPO\n1070\n0\n");
    for xdata in ["", off.as_str()] {
        let (r, e) = boxes(&dxf(180.0, xdata));
        assert!(!same(r, upright_r), "drawn at {r:?}");
        assert!(!same(e, upright_e), "extents {e:?}");
    }
}
