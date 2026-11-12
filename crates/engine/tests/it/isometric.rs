//! Isometric drafting: ISODRAFT and ISOPLANE (SNAPSTYL, SNAPISOPAIR), the isometric snap
//! lattice, ortho along the isometric plane's axes and ELLIPSE's Isocircle option.

use cadcraft_doc::EntityKind;
use cadcraft_engine::Session;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::snap::iso::{self, IsoPlane};
use cadcraft_engine::snap::tracking::CursorQuery;
use serde_json::{Value, json};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn deg(a: f64) -> Vec2 {
    Vec2::from_angle(a.to_radians())
}

fn getvar(s: &Session, name: &str) -> Value {
    cadcraft_engine::sysvars::get(s, name).unwrap_or(Value::Null)
}

fn setvar(s: &mut Session, name: &str, value: Value) -> cadcraft_engine::Result<Value> {
    s.execute("setvar", &json!({ "name": name, "value": value }))
}

fn last_ellipse(s: &Session) -> cadcraft_doc::Ellipse {
    match s.doc().unwrap().model.iter().last().map(|e| e.kind.clone()) {
        Some(EntityKind::Ellipse(e)) => e,
        other => panic!("not an ellipse: {other:?}"),
    }
}

/// Whether `p` lies on the full ellipse.
fn on_ellipse(e: &cadcraft_doc::Ellipse, p: Vec2) -> bool {
    let (c, m) = (e.center.xy(), e.major.xy());
    let (a, b) = (m.len(), m.len() * e.ratio);
    let u = m.normalized();
    let q = p - c;
    let (x, y) = (q.dot(u) / a, q.dot(u.perp()) / b);
    (x * x + y * y - 1.0).abs() < 1e-9
}

fn cursor(s: &mut Session, raw: Vec2, base: Option<Vec2>) -> Vec2 {
    s.snap_cursor(raw, &CursorQuery { base, deferred: false, corner: false, aperture: 1e-6, tol: 1e-6, now: 0.0, shift: false }).point
}

#[test]
fn isodraft_and_isoplane_set_snapstyl_and_snapisopair() {
    let mut s = Session::new();
    assert_eq!(getvar(&s, "SNAPSTYL"), json!(0));
    assert_eq!(getvar(&s, "SNAPISOPAIR"), json!(0));

    let r = s.execute("isodraft", &json!({ "plane": "top" })).unwrap();
    assert_eq!(r["message"], "<Isodraft Top>");
    assert_eq!((getvar(&s, "SNAPSTYL"), getvar(&s, "SNAPISOPAIR")), (json!(1), json!(1)));
    s.execute("isodraft", &json!({ "plane": "orthographic" })).unwrap();
    assert_eq!(getvar(&s, "SNAPSTYL"), json!(0));
    assert_eq!(getvar(&s, "SNAPISOPAIR"), json!(1), "the plane is kept");
    // The status-bar button toggles.
    s.execute("isodraft", &json!({})).unwrap();
    assert!(s.settings.isodraft);
    s.execute("isodraft", &json!({ "on": false })).unwrap();
    assert!(!s.settings.isodraft);

    // ISOPLANE (F5) cycles Left, Top, Right.
    let planes: Vec<String> = (0..3).map(|_| s.execute("isoplane", &json!({})).unwrap()["plane"].as_str().unwrap().to_string()).collect();
    assert_eq!(planes, ["Right", "Left", "Top"]);
    s.execute("isoplane", &json!({ "plane": "right" })).unwrap();
    assert_eq!(s.settings.isoplane, IsoPlane::Right);

    // SETVAR reaches both; bad values are refused.
    setvar(&mut s, "SNAPSTYL", json!(1)).unwrap();
    setvar(&mut s, "SNAPISOPAIR", json!(0)).unwrap();
    assert!(s.settings.isodraft && s.settings.isoplane == IsoPlane::Left);
    assert!(setvar(&mut s, "SNAPSTYL", json!(2)).is_err());
    assert!(setvar(&mut s, "SNAPISOPAIR", json!(3)).is_err());
    assert!(setvar(&mut s, "SNAPISOPAIR", json!(-1)).is_err());
    assert!(s.execute("isodraft", &json!({ "plane": "front" })).is_err());
    assert!(s.execute("isoplane", &json!({ "plane": "orthographic" })).is_err());
    // Not a drawing header variable.
    assert!(s.doc().unwrap().header.get("SNAPSTYL").is_none());
}

#[test]
fn isodraft_and_isoplane_at_the_command_line() {
    let mut s = Session::new();
    s.cmdline("isodraft r").unwrap();
    assert!(s.settings.isodraft && s.settings.isoplane == IsoPlane::Right);
    assert!(s.log.iter().any(|l| l.contains("<Isodraft Right>")));
    s.cmdline("isoplane").unwrap();
    assert_eq!(s.current_prompt().unwrap().default.as_deref(), Some("Left"), "Enter takes the next plane");
    s.cmdline("").unwrap();
    assert_eq!(s.settings.isoplane, IsoPlane::Left);
    s.cmdline("isoplane t").unwrap();
    assert_eq!(s.settings.isoplane, IsoPlane::Top);
    // Enter at ISODRAFT toggles: back to orthographic.
    s.cmdline("isodraft").unwrap();
    s.cmdline("").unwrap();
    assert!(!s.settings.isodraft);
    s.cmdline("isodraft o").unwrap();
    assert!(!s.settings.isodraft);
    // Switching the plane mid-command leaves the command running.
    s.cmdline("line 0,0").unwrap();
    s.execute("isoplane", &json!({})).unwrap();
    assert!(s.running.is_some());
}

#[test]
fn isometric_snap_follows_the_lattice() {
    let mut s = Session::new();
    s.execute("snap", &json!({ "on": true })).unwrap();
    setvar(&mut s, "SNAPUNIT", json!([1.0, 2.0])).unwrap();
    // Rectangular: the X and Y spacing.
    assert!(cursor(&mut s, v(0.9, 2.2), None).near(v(1.0, 2.0), 1e-12));
    s.execute("isodraft", &json!({ "plane": "left" })).unwrap();
    // Isometric: the Y spacing along the 30°, 90° and 150° axes.
    for (a, k) in [(30.0, 1.0), (90.0, 2.0), (150.0, 3.0), (210.0, 1.0), (330.0, 2.0)] {
        let target = deg(a) * (2.0 * k);
        let got = cursor(&mut s, target + v(0.13, -0.11), None);
        assert!(got.near(target, 1e-9), "{a}° × {k}: {got:?}");
    }
    // (1, 2) is a rectangular grid point but not on the isometric lattice.
    let got = cursor(&mut s, v(1.0, 2.0), None);
    assert!(!got.near(v(1.0, 2.0), 1e-6));
    assert!(got.near(v(0.0, 2.0), 1e-9), "{got:?}");
    // Snap off: the cursor is free.
    s.execute("snap", &json!({ "on": false })).unwrap();
    assert!(cursor(&mut s, v(1.0, 2.0), None).near(v(1.0, 2.0), 1e-12));
}

#[test]
fn ortho_follows_the_isometric_plane_axes() {
    let mut s = Session::new();
    s.execute("ortho", &json!({ "on": true })).unwrap();
    s.execute("polar", &json!({ "on": false })).unwrap();
    let base = v(1.0, 1.0);
    // Off: horizontal and vertical.
    assert!(cursor(&mut s, base + deg(25.0) * 3.0, Some(base)).near(base + v(3.0 * 25f64.to_radians().cos(), 0.0), 1e-9));
    for (plane, raw, axis) in [
        ("top", 25.0, 30.0),
        ("top", 160.0, 150.0),
        ("top", 100.0, 150.0),
        ("left", 25.0, 330.0),
        ("left", 80.0, 90.0),
        ("right", 25.0, 30.0),
        ("right", 200.0, 210.0),
    ] {
        s.execute("isodraft", &json!({ "plane": plane })).unwrap();
        let got = cursor(&mut s, base + deg(raw) * 3.0, Some(base));
        let dir = (got - base).normalized();
        assert!(dir.near(deg(axis), 1e-9), "{plane} {raw}°: {dir:?}");
    }
    // With isometric snap, whole snap steps along the axis.
    s.execute("isodraft", &json!({ "plane": "top" })).unwrap();
    s.execute("snap", &json!({ "on": true })).unwrap();
    setvar(&mut s, "SNAPUNIT", json!([0.5, 0.5])).unwrap();
    let got = cursor(&mut s, base + deg(27.0) * 1.37, Some(base));
    assert!(got.near(base + deg(30.0) * 1.5, 1e-9), "{got:?}");

    // Direct distance entry runs along the plane's axis too.
    s.execute("snap", &json!({ "on": false })).unwrap();
    s.cmdline("line 0,0").unwrap();
    s.cursor = deg(140.0) * 5.0;
    s.cmdline("2").unwrap();
    s.cmdline("").unwrap();
    match s.doc().unwrap().model.iter().last().map(|e| e.kind.clone()) {
        Some(EntityKind::Line(l)) => assert!(l.b.xy().near(deg(150.0) * 2.0, 1e-9), "{:?}", l.b),
        other => panic!("not a line: {other:?}"),
    }
}

#[test]
fn ellipse_isocircle_in_each_plane() {
    let mut s = Session::new();
    // Only offered with the isometric snap style.
    s.cmdline("ellipse").unwrap();
    assert!(!s.current_prompt().unwrap().keywords.iter().any(|k| k == "Isocircle"));
    s.running = None;

    s.execute("isodraft", &json!({ "plane": "top" })).unwrap();
    s.cmdline("ellipse").unwrap();
    assert!(s.current_prompt().unwrap().keywords.iter().any(|k| k == "Isocircle"));
    s.cmdline("i 3,4 2").unwrap();
    assert!(s.running.is_none());
    let e = last_ellipse(&s);
    assert!(e.center.xy().near(v(3.0, 4.0), 1e-12));
    assert!((e.ratio - 1.0 / 3f64.sqrt()).abs() < 1e-12);
    assert!((e.major.xy().len() - 2.0 * 1.5f64.sqrt()).abs() < 1e-12);
    assert!(e.major.xy().normalized().cross(Vec2::X).abs() < 1e-12, "top isocircles lie flat");
    // It meets the plane's axes at the radius.
    for a in [30.0, 150.0, 210.0, 330.0] {
        assert!(on_ellipse(&e, v(3.0, 4.0) + deg(a) * 2.0), "{a}°");
    }

    // Left plane by diameter; the Right plane through a picked point.
    s.execute("isoplane", &json!({ "plane": "left" })).unwrap();
    s.cmdline("ellipse i 0,0 d 4").unwrap();
    let e = last_ellipse(&s);
    for a in [90.0, 150.0, 270.0, 330.0] {
        assert!(on_ellipse(&e, deg(a) * 2.0), "left {a}°");
    }
    s.execute("isoplane", &json!({ "plane": "right" })).unwrap();
    let p = deg(30.0) * 1.0 + deg(90.0) * 2.0;
    s.cmdline(&format!("ellipse i 0,0 {},{}", p.x, p.y)).unwrap();
    let e = last_ellipse(&s);
    assert!(on_ellipse(&e, p));
    assert!(on_ellipse(&e, deg(90.0) * 5f64.sqrt()));

    // An isometric arc: the Arc option then Isocircle, then start and end angles.
    s.cmdline("ellipse a i 0,0 1 0 90").unwrap();
    assert!(s.running.is_none());
    let e = last_ellipse(&s);
    assert!((e.end - e.start).abs() > 1e-6 && (e.end - e.start).abs() < cadcraft_geom::TAU - 1e-6);

    // JSON form.
    let r = s.execute("ellipse", &json!({ "isocircle": { "center": [1, 1], "radius": 1, "plane": "top" } })).unwrap();
    assert_eq!(r["plane"], "Top");
    assert!(on_ellipse(&last_ellipse(&s), v(1.0, 1.0) + deg(30.0)));
    for bad in
        [json!({ "center": [0, 0], "radius": -1 }), json!({ "center": [0, 0], "radius": 0 }), json!({ "center": [0, 0] }), json!({ "radius": 1 })]
    {
        assert!(s.execute("ellipse", &json!({ "isocircle": bad })).is_err(), "{bad}");
    }
    assert!(s.execute("ellipse", &json!({ "isocircle": { "center": [0, 0], "radius": 1, "plane": "orthographic" } })).is_err());
}

#[test]
fn hostile_isometric_inputs_never_panic() {
    for d in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e-300, 1e300] {
        let p = iso::lattice_snap(v(3.0, 4.0), d, Vec2::ZERO, 0.0);
        assert!(p.is_finite());
        let p = iso::lattice_snap(v(f64::MAX, -f64::MAX), d, Vec2::ZERO, f64::NAN);
        let _ = p;
    }
    let view = cadcraft_geom::Bounds2 { min: v(-1e12, -1e12), max: v(1e12, 1e12) };
    assert!(iso::grid_lines(view, 1.0, Vec2::ZERO, 0.0, 5).is_empty(), "too many lines");
    let view = cadcraft_geom::Bounds2 { min: v(-10.0, -10.0), max: v(10.0, 10.0) };
    let lines = iso::grid_lines(view, 1.0, Vec2::ZERO, 0.3, 0);
    assert!(!lines.is_empty() && lines.len() <= 3 * (iso::MAX_GRID_LINES + 1));
    for plane in IsoPlane::ALL {
        assert!(iso::isocircle(plane, Vec2::ZERO, f64::NAN).is_none());
        assert!(iso::isocircle(plane, v(f64::NAN, 0.0), 1.0).is_none());
        assert!(iso::iso_radius(plane, Vec2::ZERO, Vec2::ZERO).is_none());
    }
    let mut s = Session::new();
    s.execute("isodraft", &json!({ "plane": "top" })).unwrap();
    for t in ["ellipse i 0,0 abc", "ellipse i 0,0 -5", "ellipse i 0,0 1e400"] {
        let _ = s.cmdline(t);
        s.running = None;
    }
}
