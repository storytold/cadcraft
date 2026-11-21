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

/// 41 of the 50,001-period lines, stacked in Y: 2,050,041 dashes in all — more than the dash
/// budget a plot's lines share, so the plot stops expanding partway and draws the rest
/// continuous.
fn many_lines_dxf() -> String {
    let mut entities = String::new();
    for i in 0..41 {
        let y = i as f64 * 1000.0;
        entities.push_str(&format!("0\nLINE\n8\nL1\n10\n0.0\n20\n{y}\n30\n0.0\n11\n37500.75\n21\n{y}\n31\n0.0\n"));
    }
    "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n9\n$LTSCALE\n40\n1.0\n0\nENDSEC\n\
     0\nSECTION\n2\nTABLES\n\
     0\nTABLE\n2\nLTYPE\n70\n2\n\
     0\nLTYPE\n2\nCONTINUOUS\n70\n0\n3\nSolid line\n72\n65\n73\n0\n40\n0.0\n\
     0\nLTYPE\n2\nDASHED\n70\n0\n3\nDashed 0.5/0.25\n72\n65\n73\n2\n40\n0.75\n49\n0.5\n74\n0\n49\n-0.25\n74\n0\n\
     0\nENDTAB\n\
     0\nTABLE\n2\nLAYER\n70\n1\n\
     0\nLAYER\n2\nL1\n70\n0\n62\n7\n6\nDASHED\n0\nENDTAB\n0\nENDSEC\n\
     0\nSECTION\n2\nENTITIES\n"
        .to_string()
        + &entities
        + "0\nENDSEC\n0\nEOF\n"
}

#[test]
fn a_plot_keeps_dashing_until_its_dash_budget_is_spent() {
    let d = cadcraft_io::read_dxf(many_lines_dxf().as_bytes()).unwrap();
    let list = build_plot(&d, &Space::Model, &Options::default());
    // 41 lines × 50,001 dashes would be 2,050,041 primitives; past the shared budget the
    // remaining lines are drawn continuous. Most of the plot stays dashed.
    let all_dashed = 41 * 50_001;
    assert!(list.prims.len() < all_dashed, "the budget bounded the dashes: {}", list.prims.len());
    assert!(list.prims.len() > 39 * 50_001, "most lines stay dashed: {}", list.prims.len());
}
