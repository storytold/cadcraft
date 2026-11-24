//! An elliptic HATCH boundary edge that meets the previous edge at its start point must not emit
//! that shared vertex twice, in either traversal direction (CCW or CW). Otherwise the resulting
//! polyline boundary contains consecutive duplicate vertices.

use cadcraft_doc::EntityKind;

fn dxf_hatch_with_elliptic_edge(ccw: bool) -> String {
    // Closed D-shaped boundary:
    // - CCW: line (0,0)->(10,0), elliptic quarter-arc (10,0)->(0,5) (0..90 deg), line (0,5)->(0,0)
    // - CW:  line (0,0)->(0,5), elliptic quarter-arc (0,5)->(10,0) (270..360 deg CW, i.e. 0..90 reversed), line (10,0)->(0,0)
    let (e1, ell, e3) = if ccw {
        (
            "72\n1\n10\n0.0\n20\n0.0\n11\n10.0\n21\n0.0\n",
            "72\n3\n10\n0.0\n20\n0.0\n11\n10.0\n21\n0.0\n40\n0.5\n50\n0.0\n51\n90.0\n73\n1\n",
            "72\n1\n10\n0.0\n20\n5.0\n11\n0.0\n21\n0.0\n",
        )
    } else {
        (
            "72\n1\n10\n0.0\n20\n0.0\n11\n0.0\n21\n5.0\n",
            "72\n3\n10\n0.0\n20\n0.0\n11\n10.0\n21\n0.0\n40\n0.5\n50\n270.0\n51\n360.0\n73\n0\n",
            "72\n1\n10\n10.0\n20\n0.0\n11\n0.0\n21\n0.0\n",
        )
    };
    format!(
        "0\nSECTION\n2\nENTITIES\n\
         0\nHATCH\n5\n100\n100\nAcDbEntity\n8\n0\n100\nAcDbHatch\n\
         10\n0.0\n20\n0.0\n30\n0.0\n210\n0.0\n220\n0.0\n230\n1.0\n\
         2\nSOLID\n70\n1\n71\n0\n\
         91\n1\n92\n1\n93\n3\n{e1}{ell}{e3}97\n0\n\
         75\n0\n76\n1\n98\n0\n\
         0\nENDSEC\n0\nEOF\n"
    )
}

fn assert_no_consecutive_duplicate_vertices(dxf: &str) {
    let doc = cadcraft_io::read(dxf.as_bytes(), "elliptic.dxf").expect("read DXF");
    let EntityKind::Hatch(h) = &doc.model.iter().find(|e| matches!(e.kind, EntityKind::Hatch(_))).expect("hatch").kind else {
        panic!("expected hatch");
    };
    assert_eq!(h.loops.len(), 1);
    let verts = &h.loops[0].vertices;
    assert!(verts.len() >= 4, "expected tessellated loop, got {} vertices", verts.len());
    for i in 0..verts.len() {
        let a = verts[i].p;
        let b = verts[(i + 1) % verts.len()].p;
        assert!(!a.near(b, 1e-9), "duplicate consecutive vertices at index {i}: {a:?} and {b:?}");
    }
}

#[test]
fn ccw_elliptic_edge_does_not_duplicate_shared_start_vertex() {
    let dxf = dxf_hatch_with_elliptic_edge(true);
    assert_no_consecutive_duplicate_vertices(&dxf);

    // Round-trip through DXF write -> read keeps the polyline boundary free of duplicate vertices.
    let doc = cadcraft_io::read(dxf.as_bytes(), "elliptic.dxf").unwrap();
    let saved = cadcraft_io::write_dxf(&doc);
    assert_no_consecutive_duplicate_vertices(&saved);
}

#[test]
fn cw_elliptic_edge_does_not_duplicate_shared_start_vertex() {
    let dxf = dxf_hatch_with_elliptic_edge(false);
    assert_no_consecutive_duplicate_vertices(&dxf);
}
