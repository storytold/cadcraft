//! A dimension kept as read (LARGE_RADIAL_DIMENSION, ARC_DIMENSION) draws its `*D` block through
//! a reference in its graphics (#3438); object snaps reach the objects inside that block.

use cadcraft_engine::doc::Space;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::snap::{self, mode};

#[test]
fn a_kept_dimension_snaps_to_its_blocks_objects() {
    // The block's base point isn't the origin; its line is where the dimension draws it.
    let dxf = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n8\n0\n2\n*D1\n70\n1\n10\n100\n20\n50\n3\n*D1\n0\nLINE\n8\n0\n10\n110\n20\n60\n11\n120\n21\n70\n0\nENDBLK\n0\nENDSEC\n\
0\nSECTION\n2\nENTITIES\n0\nLARGE_RADIAL_DIMENSION\n5\n41\n100\nAcDbEntity\n8\n0\n100\nAcDbDimension\n2\n*D1\n0\nENDSEC\n0\nEOF\n";
    let d = cadcraft_io::read_dxf(dxf.as_bytes()).expect("read");
    let at = |p: Vec2, m: u32| snap::osnap(&d, &Space::Model, p + Vec2::new(0.02, -0.01), 0.1, m, None, false).map(|h| (h.point, h.mode));
    let end = Vec2::new(120.0, 70.0);
    assert!(at(end, mode::END).is_some_and(|(p, m)| m == mode::END && p.dist(end) < 1e-9), "{:?}", at(end, mode::END));
    let mid = Vec2::new(115.0, 65.0);
    assert!(at(mid, mode::MID).is_some_and(|(p, m)| m == mode::MID && p.dist(mid) < 1e-9), "{:?}", at(mid, mode::MID));
}

/// A DIMENSION inside a block snaps to the objects of its own `*D` block, as it is drawn; so does
/// one moved by an older AutoCAD (group 12), translated by the move as the renderer draws it.
#[test]
fn dimensions_in_blocks_snap_to_their_blocks_objects() {
    let dim = |handle: &str, offset: (f64, f64)| {
        format!(
            "0\nDIMENSION\n5\n{handle}\n100\nAcDbEntity\n8\n0\n100\nAcDbDimension\n2\n*D2\n10\n5\n20\n7\n30\n0\n11\n5\n21\n8\n31\n0\n12\n{}\n22\n{}\n32\n0\n70\n1\n100\nAcDbAlignedDimension\n13\n0\n23\n7\n33\n0\n14\n10\n24\n7\n34\n0\n",
            offset.0, offset.1
        )
    };
    let dxf = format!(
        "0\nSECTION\n2\nBLOCKS\n\
0\nBLOCK\n8\n0\n2\n*D2\n70\n1\n10\n0\n20\n0\n3\n*D2\n0\nLINE\n8\n0\n10\n0\n20\n7\n11\n10\n21\n7\n0\nENDBLK\n\
0\nBLOCK\n8\n0\n2\nNEST\n70\n0\n10\n0\n20\n0\n3\nNEST\n{}0\nENDBLK\n0\nENDSEC\n\
0\nSECTION\n2\nENTITIES\n0\nINSERT\n5\n70\n8\n0\n2\nNEST\n10\n100\n20\n0\n{}0\nENDSEC\n0\nEOF\n",
        dim("60", (0.0, 0.0)),
        dim("61", (0.0, 50.0)),
    );
    let d = cadcraft_io::read_dxf(dxf.as_bytes()).expect("read");
    let at = |p: Vec2, m: u32| snap::osnap(&d, &Space::Model, p + Vec2::new(0.02, -0.01), 0.1, m, None, false).map(|h| (h.point, h.mode));
    for (p, m) in [(Vec2::new(110.0, 7.0), mode::END), (Vec2::new(105.0, 7.0), mode::MID), (Vec2::new(10.0, 57.0), mode::END)] {
        assert!(at(p, m).is_some_and(|(q, k)| k == m && q.dist(p) < 1e-9), "{p:?}: {:?}", at(p, m));
    }
}
