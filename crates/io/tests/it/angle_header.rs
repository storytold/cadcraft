//! The angle settings round-trip through DXF: `$ANGBASE` (group 50) in degrees, as AutoCAD writes
//! it (a drawing measured from north has 90.0), held in radians; `$ANGDIR` and `$AUNITS`
//! (group 70) as integers.

use cadcraft_doc::{Drawing, HVal};

#[test]
fn angbase_angdir_aunits_round_trip_through_dxf() {
    let mut d = Drawing::new_metric();
    d.header.set_f64("ANGBASE", std::f64::consts::FRAC_PI_2);
    d.header.set_i64("ANGDIR", 1);
    d.header.set_i64("AUNITS", 3);
    let text = cadcraft_io::write_dxf(&d);
    // Written in degrees.
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let at = lines.iter().position(|l| *l == "$ANGBASE").expect("$ANGBASE is written");
    let written: f64 = lines.get(at + 2).and_then(|v| v.parse().ok()).expect("a number");
    assert!((written - 90.0).abs() < 1e-9, "$ANGBASE written as {written}");
    let r = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let base = r.header.f64("ANGBASE", 0.0);
    assert!((base - std::f64::consts::FRAC_PI_2).abs() < 1e-12, "$ANGBASE read back as {base}");
    assert_eq!(r.header.get("ANGDIR"), Some(&HVal::Int(1)));
    assert_eq!(r.header.get("AUNITS"), Some(&HVal::Int(3)));
}

#[test]
fn angbase_in_degrees_is_read_as_radians() {
    // What AutoCAD writes for a drawing whose angles run clockwise from north.
    let text = "0\nSECTION\n2\nHEADER\n9\n$ANGBASE\n50\n90.0\n9\n$ANGDIR\n70\n1\n0\nENDSEC\n0\nEOF\n";
    let d = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let base = d.header.f64("ANGBASE", 0.0);
    assert!((base - std::f64::consts::FRAC_PI_2).abs() < 1e-12, "$ANGBASE 90.0 read as {base} radians");
}
