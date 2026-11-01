//! A layout keeps its limits (LAYOUT groups 10/11) and its page's paper image origin
//! (groups 148/149) through a DXF and a DWG save, as the file had them, so another program puts
//! the sheet back where it was (#2109). A layout CADCraft made writes the defaults as before.

use cadcraft_doc::geom::Vec2;
use cadcraft_doc::{Drawing, Layout};
use cadcraft_io::{SaveOptions, Version, read, read_dxf, write_dxf, write_with};

/// A drawing whose layout has the limits and image origin of the ZWCAD file in #2109.
fn drawing() -> Drawing {
    let mut d = Drawing::new_imperial();
    let mut layout = Layout::new("Sheet", 1);
    layout.limits = Some((Vec2::new(1.038, 0.694), Vec2::new(9.506, 6.681)));
    layout.page.image_origin = Some(Vec2::new(-1.054, -0.71));
    d.layouts = vec![layout];
    d
}

fn sheet(d: &Drawing) -> &Layout {
    d.layouts.iter().find(|l| l.name == "Sheet").expect("the layout")
}

fn near(a: Vec2, b: Vec2) -> bool {
    (a - b).len() < 1e-9
}

fn assert_kept(d: &Drawing, how: &str) {
    let l = sheet(d);
    let (lo, hi) = l.limits.unwrap_or_else(|| panic!("{how}: the limits are kept"));
    assert!(near(lo, Vec2::new(1.038, 0.694)) && near(hi, Vec2::new(9.506, 6.681)), "{how}: {lo:?} {hi:?}");
    let origin = l.page.image_origin.unwrap_or_else(|| panic!("{how}: the image origin is kept"));
    assert!(near(origin, Vec2::new(-1.054, -0.71)), "{how}: {origin:?}");
}

#[test]
fn a_layout_keeps_its_limits_and_image_origin_through_dxf() {
    let back = read_dxf(write_dxf(&drawing()).as_bytes()).expect("reopen");
    assert_kept(&back, "DXF");
    // And again: a second save writes what the first read.
    let again = read_dxf(write_dxf(&back).as_bytes()).expect("reopen");
    assert_kept(&again, "DXF twice");
}

#[test]
fn a_layout_keeps_its_limits_and_image_origin_through_dwg() {
    let dwg = write_with(&drawing(), "t.dwg", &SaveOptions { version: Some(Version::R2018), binary: false }).expect("DWG");
    let back = read(&dwg, "t.dwg").expect("reopen");
    assert_kept(&back, "DWG");
}

#[test]
fn a_new_layout_writes_the_defaults() {
    let text = write_dxf(&Drawing::new_imperial());
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    // The AcDbLayout limits after the layout's name, and the image origin before it.
    let after = |code: &str| lines.windows(2).filter(|w| w[0] == code).map(|w| w[1]).collect::<Vec<_>>();
    assert!(after("148").iter().all(|v| v.parse::<f64>() == Ok(0.0)), "{:?}", after("148"));
    let back = read_dxf(text.as_bytes()).expect("reopen");
    let l = back.layouts.first().expect("a layout");
    // The defaults read back as none, so a drawing CADCraft made round-trips unchanged.
    assert_eq!((l.limits, l.page.image_origin), (None, None));
    let limits = (after("10"), after("11"));
    assert!(limits.0.iter().any(|v| v.parse::<f64>() == Ok(0.0)) && limits.1.iter().any(|v| v.parse::<f64>() == Ok(12.0)), "{limits:?}");
}
