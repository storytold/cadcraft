//! A dashed line longer than 50,000 pattern periods is drawn as a single solid line in plots and
//! exports: the guard against huge dash counts is for a display zoomed out a lot, but it fired
//! with the level of detail off too, where there is no zoom (#1069).
#![cfg(not(target_arch = "wasm32"))]

use cadcraft_doc::Space;
use cadcraft_render::{Options, build_plot};

/// One LINE of `length` units on the DASHED linetype (0.5 dash, 0.25 gap).
fn dxf(length: f64) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n9\n$LTSCALE\n40\n1.0\n0\nENDSEC\n\
         0\nSECTION\n2\nTABLES\n\
         0\nTABLE\n2\nLTYPE\n70\n2\n\
         0\nLTYPE\n2\nCONTINUOUS\n70\n0\n3\nSolid line\n72\n65\n73\n0\n40\n0.0\n\
         0\nLTYPE\n2\nDASHED\n70\n0\n3\nDashed 0.5/0.25\n72\n65\n73\n2\n40\n0.75\n49\n0.5\n74\n0\n49\n-0.25\n74\n0\n\
         0\nENDTAB\n\
         0\nTABLE\n2\nLAYER\n70\n1\n\
         0\nLAYER\n2\nL1\n70\n0\n62\n7\n6\nDASHED\n0\nENDTAB\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nL1\n10\n0.0\n20\n0.0\n30\n0.0\n11\n{length}\n21\n0.0\n31\n0.0\n0\nENDSEC\n0\nEOF\n"
    )
}

#[test]
fn a_very_long_dashed_line_is_dashed_in_a_plot() {
    // Either side of the 50,000 periods the display guard used to stop at: both are dashed.
    for (length, periods) in [(37_499.25, 49_999), (37_500.75, 50_001)] {
        let d = cadcraft_io::read_dxf(dxf(length).as_bytes()).unwrap();
        let list = build_plot(&d, &Space::Model, &Options::default());
        assert_eq!(list.prims.len(), periods, "the plot drew the {length}-unit line as one run");
    }
}
