//! Running object snaps in a fresh drawing and in drawings opened from DXF and DWG files, with
//! AutoCAD's default modes (OSMODE 4133: Endpoint, Center, Intersection, Extension).
//!
//! Moving onto a circle, an arc or a polyline arc finds its Center. OSMODE is a profile setting,
//! as in AutoCAD (saved in the registry there, not the drawing): a file's `$OSMODE` header
//! variable, 0 included, never changes the running snaps.

use cadcraft_engine::Session;
use cadcraft_engine::doc::Drawing;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::snap::{mode, tracking::CursorQuery};
use serde_json::json;

/// AutoCAD's default running snaps.
const DEFAULT: u32 = 4133;

fn polar(c: Vec2, r: f64, deg: f64) -> Vec2 {
    let a = deg.to_radians();
    c + Vec2::new(r * a.cos(), r * a.sin())
}

/// A line crossed by another, a circle, an arc and a polyline with a straight and an arc segment.
fn draw(s: &mut Session) {
    s.execute("line", &json!({ "points": [[0, 0], [20, 0]] })).unwrap();
    s.execute("line", &json!({ "points": [[5, -5], [5, 5]] })).unwrap();
    s.execute("circle", &json!({ "center": [50, 0], "radius": 5 })).unwrap();
    s.execute("arc", &json!({ "center": [80, 0], "radius": 5, "start": 0, "end": 180 })).unwrap();
    // 100,0 to 110,0 straight, then a half circle (bulge 1) to 130,0 round 120,0, below the axis.
    s.execute("pline", &json!({ "vertices": [[100, 0], {"p": [110, 0], "bulge": 1.0}, [130, 0]] })).unwrap();
}

fn drawing() -> Drawing {
    let mut s = Session::new();
    draw(&mut s);
    s.doc().unwrap().clone()
}

/// Where a click at `cursor` lands while LINE asks for its first point (the drawing area's path).
fn snap(s: &mut Session, cursor: Vec2) -> Option<(Vec2, u32)> {
    if s.running.is_none() {
        s.start("line").unwrap();
    }
    let q = CursorQuery { base: None, deferred: true, corner: false, aperture: 1.0, tol: 0.5, now: 0.0, shift: false };
    let r = s.snap_cursor(cursor, &q);
    r.snap.map(|h| (r.point, h.mode))
}

fn assert_snaps(s: &mut Session, what: &str) {
    assert_eq!(s.settings.osmode, DEFAULT, "{what}: the running snaps are the profile's");
    let cases = [
        ("the line's end", Vec2::new(20.2, 0.1), Vec2::new(20.0, 0.0), &[mode::END][..]),
        ("the lines' crossing", Vec2::new(5.2, 0.15), Vec2::new(5.0, 0.0), &[mode::INT][..]),
        ("the circle", polar(Vec2::new(50.0, 0.0), 5.3, 45.0), Vec2::new(50.0, 0.0), &[mode::CEN][..]),
        ("the arc", polar(Vec2::new(80.0, 0.0), 5.2, 90.0), Vec2::new(80.0, 0.0), &[mode::CEN][..]),
        ("the arc's end", Vec2::new(85.2, 0.1), Vec2::new(85.0, 0.0), &[mode::END][..]),
        ("the polyline's vertex", Vec2::new(110.2, 0.1), Vec2::new(110.0, 0.0), &[mode::END][..]),
        ("the polyline's arc", polar(Vec2::new(120.0, 0.0), 10.2, 270.0), Vec2::new(120.0, 0.0), &[mode::CEN][..]),
    ];
    for (name, cursor, want, modes) in cases {
        let (p, m) = snap(s, cursor).unwrap_or_else(|| panic!("{what}: no snap on {name}"));
        assert!(modes.contains(&m), "{what}, {name}: mode {m}");
        assert!(p.near(want, 1e-6), "{what}, {name}: {p:?}");
    }
    // Away from everything: nothing.
    assert_eq!(snap(s, Vec2::new(35.0, 20.0)), None, "{what}");
    // F3 off: nothing; on again: back.
    s.execute("osnap", &json!({})).unwrap();
    assert_eq!(snap(s, Vec2::new(20.2, 0.1)), None, "{what}: F3 off");
    s.execute("osnap", &json!({})).unwrap();
    assert_eq!(snap(s, Vec2::new(20.2, 0.1)).map(|h| h.1), Some(mode::END), "{what}: F3 on");
}

#[test]
fn default_snaps_work_in_a_fresh_drawing() {
    let mut s = Session::new();
    draw(&mut s);
    assert_snaps(&mut s, "fresh drawing");
}

/// `text` with `$OSMODE` set to `value` at the top of its header.
fn with_osmode(text: &str, value: u32) -> String {
    let at = text.find("HEADER").expect("a header section") + "HEADER".len();
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let (head, tail) = text.split_at(at);
    format!("{head}{eol}  9{eol}$OSMODE{eol} 70{eol}{value}{tail}")
}

#[test]
fn a_dxf_with_osmode_0_keeps_the_running_snaps() {
    let text = String::from_utf8(cadcraft_io::write(&drawing(), "snaps.dxf").unwrap()).unwrap();
    let text = with_osmode(&text, 0);
    let d = cadcraft_io::read(text.as_bytes(), "snaps.dxf").unwrap();
    let mut s = Session::new();
    // OSMODE is the profile's: the user's own choice survives opening the file.
    s.execute("setvar", &json!({ "name": "OSMODE", "value": DEFAULT })).unwrap();
    s.open_drawing(d, "snaps.dxf", None);
    assert_snaps(&mut s, "DXF with $OSMODE 0");
    // Nor does a file change a profile without Center.
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "OSMODE", "value": mode::END })).unwrap();
    s.open_drawing(cadcraft_io::read(with_osmode(&text, DEFAULT).as_bytes(), "x.dxf").unwrap(), "x.dxf", None);
    assert_eq!(s.settings.osmode, mode::END);
}

#[test]
fn default_snaps_work_in_a_drawing_opened_from_dwg() {
    let bytes = cadcraft_io::write(&drawing(), "snaps.dwg").unwrap();
    let d = cadcraft_io::read(&bytes, "snaps.dwg").unwrap();
    let mut s = Session::new();
    s.open_drawing(d, "snaps.dwg", None);
    assert_snaps(&mut s, "DWG");
}
