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
