//! #2896: feet and inches with a hyphenated fraction (`6-1/2`, `1'6-1/2"`), typed at the command
//! line, where a space would end the input, in coordinates, polar points and distances.

use cadcraft_doc::EntityKind;
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;

/// An architectural drawing, the line typed into it, and the line's ends.
fn line(typed: &[&str]) -> Option<(Vec2, Vec2)> {
    let mut s = Session::new();
    s.cmdline("setvar lunits 4").unwrap();
    for t in typed {
        s.cmdline(t).unwrap();
    }
    s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Line(l) = &e.kind { Some((l.a.xy(), l.b.xy())) } else { None })
}

#[test]
fn a_hyphenated_fraction_is_a_coordinate() {
    let got = line(&["line 0,0 6-1/2,1'0-1/16\"", ""]);
    assert!(got.is_some_and(|(a, b)| a.near(Vec2::ZERO, 1e-12) && b.near(Vec2::new(6.5, 12.0625), 1e-12)), "{got:?}");
}

#[test]
fn a_hyphenated_fraction_is_a_polar_distance() {
    let got = line(&["line 0,0 @1'6-1/2\"<0", ""]);
    assert!(got.is_some_and(|(_, b)| b.near(Vec2::new(18.5, 0.0), 1e-12)), "{got:?}");
}

#[test]
fn dist_reports_a_hyphenated_fraction() {
    let mut s = Session::new();
    s.cmdline("setvar lunits 4").unwrap();
    s.cmdline("dist 0,0 1'6-1/2\",0").unwrap();
    assert!(s.log.iter().any(|l| l.starts_with("Distance = 1'-6 1/2\"")), "{:?}", s.log);
}
