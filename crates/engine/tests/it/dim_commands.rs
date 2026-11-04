//! DIMEDIT (Home/New/Rotate/Oblique), DIMJOGGED, DIMJOGLINE, DIMBREAK and DIMINSPECT: the JSON
//! forms, the prompt sequences and what they draw.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Dimension, EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::render::{DimGeometry, LineRole};
use serde_json::{Value, json};

fn dim_of(s: &Session, h: &str) -> (Dimension, DimGeometry) {
    let d = s.doc().unwrap();
    match &d.entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
        EntityKind::Dimension(dm) => (dm.clone(), cadcraft_engine::render::dimension_in(d, dm)),
        k => panic!("not a dimension: {k:?}"),
    }
}

fn handle(r: &Value) -> String {
    r["handle"].as_str().unwrap().to_string()
}

fn linear(s: &mut Session) -> String {
    handle(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap())
}

/// Whether a line of the role on y = `y` covers `x`.
fn covers(g: &DimGeometry, role: LineRole, y: f64, x: f64) -> bool {
    g.lines_of(role)
        .any(|l| l.windows(2).any(|w| (w[0].y - y).abs() < 1e-9 && (w[1].y - y).abs() < 1e-9 && w[0].x.min(w[1].x) < x && x < w[0].x.max(w[1].x)))
}

#[test]
fn dimedit_modes() {
    let mut s = Session::new();
    let h = linear(&mut s);
    s.execute("dimedit", &json!({ "handles": [h], "mode": "new", "text": "<> TYP" })).unwrap();
    assert_eq!(dim_of(&s, &h).1.value, "10.0000 TYP");
    s.execute("dimedit", &json!({ "handles": [h], "mode": "rotate", "angle": 30 })).unwrap();
    assert!((dim_of(&s, &h).1.text_angle - 30f64.to_radians()).abs() < 1e-9);
    s.execute("dimedit", &json!({ "handles": [h], "mode": "home" })).unwrap();
    let (dm, g) = dim_of(&s, &h);
    assert!(dm.text_rotation == 0.0 && !dm.user_text_pos && g.text_angle == 0.0);
    // Oblique: extension lines at 60°, the measurement unchanged.
    s.execute("dimedit", &json!({ "handles": [h], "mode": "oblique", "angle": 60 })).unwrap();
    let (dm, g) = dim_of(&s, &h);
    assert!((dm.marks.oblique.unwrap() - 60f64.to_radians()).abs() < 1e-9);
    assert_eq!(g.value, "10.0000 TYP");
    for l in g.lines_of(LineRole::Ext) {
        let d = l[1] - l[0];
        assert!((d.y.atan2(d.x).to_degrees() - 60.0).abs() < 1e-6, "{l:?}");
    }
    // The dimension line still runs along y = 3, between the oblique feet.
    let foot = 3.0 / 60f64.to_radians().tan();
    assert!(covers(&g, LineRole::Dim, 3.0, foot + 0.5) && covers(&g, LineRole::Dim, 3.0, 10.0 + foot - 0.5));
    // No angle: back to perpendicular. Other kinds are left alone.
    s.execute("dimedit", &json!({ "handles": [h], "mode": "oblique" })).unwrap();
    assert_eq!(dim_of(&s, &h).0.marks.oblique, None);
    s.execute("circle", &json!({ "center": [30, 0], "radius": 5 })).unwrap();
    let c = s.doc().unwrap().model.last().unwrap().handle.hex();
    let r = handle(&s.execute("dimradius", &json!({ "handle": c })).unwrap());
    let n = s.execute("dimedit", &json!({ "handles": [r], "mode": "oblique", "angle": 60 })).unwrap();
    assert_eq!(n["changed"], json!(0));
    assert!(s.execute("dimedit", &json!({ "handles": [h], "mode": "bogus" })).is_err());
    assert!(s.execute("dimedit", &json!({ "handles": [h], "mode": "rotate" })).is_err());
    assert!(s.execute("dimedit", &json!({ "handles": [h], "mode": "rotate", "angle": "x" })).is_err());
}

#[test]
fn dimedit_prompts() {
    let mut s = Session::new();
    let h = linear(&mut s);
    s.cmdline("dimedit").unwrap();
    assert!(s.prompt_text().contains("[Home/New/Rotate/Oblique] <Home>"), "{}", s.prompt_text());
    // Oblique: the dimensions first, then the angle.
    s.script("o\n4,3\n\n45\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!((dim_of(&s, &h).0.marks.oblique.unwrap() - 45f64.to_radians()).abs() < 1e-9);
    // Rotate: the angle first.
    s.script("dimedit r 15\n4,3\n\n").unwrap();
    assert!(s.running.is_none());
    assert!((dim_of(&s, &h).0.text_rotation - 15f64.to_radians()).abs() < 1e-9);
    // Home by default.
    s.script("dimedit\n\n4,3\n\n").unwrap();
    assert_eq!(dim_of(&s, &h).0.text_rotation, 0.0);
    // The Dimension ▸ Oblique menu item asks for the dimensions straight away.
    s.cmdline("dimedit.oblique").unwrap();
    assert!(s.prompt_text().contains("Select objects"), "{}", s.prompt_text());
    s.script("4,3\n\n\n").unwrap();
    assert_eq!(dim_of(&s, &h).0.marks.oblique, None);
}

#[test]
fn jogged_radius() {
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 50 })).unwrap();
    let c = s.doc().unwrap().model.last().unwrap().handle.hex();
    let r = s.execute("dimjogged", &json!({ "handle": c, "centerOverride": [20, -10], "at": [40, 40], "jog": [30, 15] })).unwrap();
    let h = handle(&r);
    let (dm, g) = dim_of(&s, &h);
    let j = dm.marks.jogged.clone().unwrap();
    assert_eq!(g.value, "R50.0000", "the true radius");
    assert!((dm.measurement - 50.0).abs() < 1e-9, "the stored measurement is the radius, got {} (#2421)", dm.measurement);
    assert!(j.center.xy().dist(Vec2::new(20.0, -10.0)) < 1e-9);
    // The line runs from the arrowhead on the circle to the override, with a 45° jog (DIMJOGANG).
    let tip = Vec2::new(1.0, 1.0).normalized() * 50.0;
    let path = g.lines_of(LineRole::Dim).find(|l| l.len() == 4).unwrap().clone();
    assert!(path[0].dist(tip) < 1e-9 && path[3].dist(Vec2::new(20.0, -10.0)) < 1e-9, "{path:?}");
    let u = tip.normalized();
    let jog = path[2] - path[1];
    let angle = jog.normalized().dot(u).abs().acos().to_degrees();
    assert!((angle - 45.0).abs() < 1e-6, "{angle}");
    assert!((path[3] - path[2]).normalized().cross(u).abs() < 1e-9, "parallel to the first part");
    // DIMJOGANG changes the jog.
    s.execute("setvar", &json!({ "name": "DIMJOGANG", "value": 60 })).unwrap();
    let h2 = handle(&s.execute("dimjogged", &json!({ "handle": c, "centerOverride": [20, -10], "at": [40, 40] })).unwrap());
    let g = dim_of(&s, &h2).1;
    let path = g.lines_of(LineRole::Dim).find(|l| l.len() == 4).unwrap().clone();
    assert!(((path[2] - path[1]).normalized().dot(u).abs().acos().to_degrees() - 60.0).abs() < 1e-6);
    // At the command line.
    s.script("dimjogged 0,50\n10,-20\n0,70\n5,20\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let last = s.doc().unwrap().model.last().unwrap().handle.hex();
    let (dm, _) = dim_of(&s, &last);
    let j = dm.marks.jogged.unwrap();
    assert!(j.center.xy().dist(Vec2::new(10.0, -20.0)) < 1e-9 && j.jog.xy().dist(Vec2::new(5.0, 20.0)) < 1e-9);
    assert!(dm.p15.xy().dist(Vec2::new(0.0, 50.0)) < 1e-9);
    // Not a circle: refused.
    s.execute("line", &json!({ "points": [[100, 0], [110, 0]] })).unwrap();
    let l = s.doc().unwrap().model.last().unwrap().handle.hex();
    assert!(s.execute("dimjogged", &json!({ "handle": l, "centerOverride": [0, 0], "at": [1, 1] })).is_err());
}

#[test]
fn jogged_linear() {
    let mut s = Session::new();
    let h = linear(&mut s);
    s.execute("dimjogline", &json!({ "handle": h, "at": [2, 3] })).unwrap();
    let (dm, g) = dim_of(&s, &h);
    assert!(dm.marks.jog_line.is_some());
    // The line is cut around the jog and a zigzag drawn there (1.5 × the text height).
    assert!(!covers(&g, LineRole::Dim, 3.0, 2.0));
    let zig = g.lines_of(LineRole::Dim).find(|l| l.len() == 4).unwrap();
    let h_jog = zig.iter().map(|p| p.y).fold(f64::MIN, f64::max) - zig.iter().map(|p| p.y).fold(f64::MAX, f64::min);
    assert!((h_jog - 1.5 * 0.18).abs() < 1e-9, "{h_jog}");
    // Default place: halfway between the first extension line and the text.
    s.execute("dimjogline", &json!({ "handle": h })).unwrap();
    assert!(dim_of(&s, &h).0.marks.jog_line.unwrap().xy().dist(Vec2::new(2.5, 3.0)) < 1e-9);
    s.execute("dimjogline", &json!({ "handle": h, "remove": true })).unwrap();
    assert_eq!(dim_of(&s, &h).0.marks.jog_line, None);
    // Prompts: pick the dimension, then the jog location; Remove picks the jog to remove.
    s.script("dimjogline 1,3\n8,3\n").unwrap();
    assert!(s.running.is_none());
    assert!(dim_of(&s, &h).0.marks.jog_line.unwrap().xy().dist(Vec2::new(8.0, 3.0)) < 1e-9);
    s.script("dimjogline r\n1,3\n").unwrap();
    assert!(s.running.is_none());
    assert_eq!(dim_of(&s, &h).0.marks.jog_line, None);
    // Radial dimensions take no jog.
    s.execute("circle", &json!({ "center": [30, 0], "radius": 5 })).unwrap();
    let c = s.doc().unwrap().model.last().unwrap().handle.hex();
    let r = handle(&s.execute("dimradius", &json!({ "handle": c })).unwrap());
    assert!(s.execute("dimjogline", &json!({ "handle": r })).is_err());
}

#[test]
fn breaks() {
    let mut s = Session::new();
    let h = linear(&mut s);
    // A line crossing the dimension line and the second extension line.
    s.execute("line", &json!({ "points": [[4, -1], [4, 6]] })).unwrap();
    s.execute("line", &json!({ "points": [[8, 1], [12, 1]] })).unwrap();
    s.execute("dimbreak", &json!({ "handles": [h] })).unwrap();
    let (dm, g) = dim_of(&s, &h);
    assert_eq!(dm.marks.breaks.len(), 2, "{:?}", dm.marks.breaks);
    assert!(dm.marks.breaks.iter().all(|b| b.auto && (b.size - 0.125).abs() < 1e-12));
    assert!(!covers(&g, LineRole::Dim, 3.0, 4.0) && covers(&g, LineRole::Dim, 3.0, 3.8));
    let ext_at = |g: &DimGeometry, y: f64| {
        g.lines_of(LineRole::Ext).any(|l| l.windows(2).any(|w| (w[0].x - 10.0).abs() < 1e-9 && w[0].y.min(w[1].y) < y && y < w[0].y.max(w[1].y)))
    };
    assert!(!ext_at(&g, 1.0) && ext_at(&g, 2.0), "extension line 2 broken at y = 1");
    // Auto again doesn't pile up; only the chosen objects; manual; remove.
    let first = s.doc().unwrap().model.iter().nth(1).unwrap().handle.hex();
    s.execute("dimbreak", &json!({ "handles": [h], "objects": [first], "size": 0.5 })).unwrap();
    assert_eq!(dim_of(&s, &h).0.marks.breaks.len(), 1);
    s.execute("dimbreak", &json!({ "handles": [h], "mode": "manual", "first": [6, 3], "second": [7, 3] })).unwrap();
    let (dm, g) = dim_of(&s, &h);
    assert_eq!(dm.marks.breaks.len(), 2);
    assert!(!covers(&g, LineRole::Dim, 3.0, 6.5) && covers(&g, LineRole::Dim, 3.0, 7.2));
    s.execute("dimbreak", &json!({ "handles": [h], "mode": "remove" })).unwrap();
    assert!(dim_of(&s, &h).0.marks.breaks.is_empty());
    // Prompts: the dimension, then objects (Enter ends) or Auto/Manual/Remove.
    s.script("dimbreak 1,3\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(dim_of(&s, &h).0.marks.breaks.len(), 2);
    s.script("dimbreak 1,3 r\n").unwrap();
    assert!(dim_of(&s, &h).0.marks.breaks.is_empty());
    s.script("dimbreak 1,3 m 1,3 2,3\n").unwrap();
    assert_eq!(dim_of(&s, &h).0.marks.breaks.len(), 1);
    // Bad input.
    assert!(s.execute("dimbreak", &json!({ "handles": [h], "size": -1 })).is_err());
    assert!(s.execute("dimbreak", &json!({ "handles": [h], "mode": "manual", "first": [1, 1], "second": [1, 1] })).is_err());
    assert!(s.execute("dimbreak", &json!({ "handles": [h], "mode": "nope" })).is_err());
}

#[test]
fn inspection_dimensions() {
    let mut s = Session::new();
    let h = linear(&mut s);
    s.execute("diminspect", &json!({ "handles": [h], "label": "A", "rate": "50%" })).unwrap();
    let (dm, g) = dim_of(&s, &h);
    assert_eq!(dm.marks.inspect.as_ref().unwrap().shape, 0);
    assert_eq!(g.value, "A  10.0000  50%");
    // The frame: two long sides, two separators, two round ends.
    let plain = {
        let mut p = dm.clone();
        p.marks.inspect = None;
        cadcraft_engine::render::dimension_in(s.doc().unwrap(), &p)
    };
    assert_eq!(g.lines_of(LineRole::Dim).count(), plain.lines_of(LineRole::Dim).count() + 6);
    s.execute("diminspect", &json!({ "handles": [h], "shape": "none", "label": "", "rate": "" })).unwrap();
    let (_, g) = dim_of(&s, &h);
    assert_eq!(g.value, "10.0000");
    assert_eq!(g.lines_of(LineRole::Dim).count(), plain.lines_of(LineRole::Dim).count());
    s.execute("diminspect", &json!({ "handles": [h], "remove": true })).unwrap();
    assert_eq!(dim_of(&s, &h).0.marks.inspect, None);
    assert!(s.execute("diminspect", &json!({ "handles": [h], "shape": "oval" })).is_err());
    // Prompts: dimensions, shape, label, rate.
    s.script("diminspect 1,3\n\na\nQC\n25%\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let ins = dim_of(&s, &h).0.marks.inspect.unwrap();
    assert_eq!((ins.shape, ins.label.as_str(), ins.rate.as_str()), (1, "QC", "25%"));
    s.script("diminspect 1,3\n\nremove\n").unwrap();
    assert_eq!(dim_of(&s, &h).0.marks.inspect, None);
}

#[test]
fn marks_follow_move_and_rotate_and_hostile_values_do_not_panic() {
    let mut s = Session::new();
    let h = linear(&mut s);
    s.execute("dimjogline", &json!({ "handle": h, "at": [2, 3] })).unwrap();
    s.execute("dimedit", &json!({ "handles": [h], "mode": "oblique", "angle": 60 })).unwrap();
    s.execute("move", &json!({ "handles": [h], "from": [0, 0], "to": [10, 5] })).unwrap();
    let (dm, _) = dim_of(&s, &h);
    assert!(dm.marks.jog_line.unwrap().xy().dist(Vec2::new(12.0, 8.0)) < 1e-9);
    s.execute("rotate", &json!({ "handles": [h], "base": [10, 5], "angle": 90 })).unwrap();
    let (dm, _) = dim_of(&s, &h);
    assert!((dm.marks.oblique.unwrap() - 150f64.to_radians()).abs() < 1e-9);
    // Damaged marks still draw.
    let d = s.doc_mut().unwrap();
    let hh = Handle::parse_hex(&h).unwrap();
    d.modify_entity(hh, |e| {
        if let EntityKind::Dimension(dm) = &mut e.kind {
            dm.marks.oblique = Some(f64::NAN);
            dm.marks.jog_line = Some(cadcraft_engine::geom::Vec3::new(f64::INFINITY, 0.0, 0.0));
            dm.marks.breaks.push(cadcraft_engine::doc::DimBreak { at: Default::default(), size: f64::NAN, auto: false });
            dm.marks.inspect = Some(cadcraft_engine::doc::Inspection { shape: 9, label: "x".into(), rate: String::new() });
        }
    })
    .unwrap();
    let _ = dim_of(&s, &h);
}

/// A dimension made in the session reports its measurement at once, not 0.0 until it is saved
/// and reopened (#2421).
#[test]
fn a_new_dimension_reports_its_measurement() {
    let mut s = Session::new();
    let measurement = |s: &Session, h: &str| dim_of(s, h).0.measurement;
    let horizontal = handle(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [100, 0], "at": [50, 20] })).unwrap());
    assert!((measurement(&s, &horizontal) - 100.0).abs() < 1e-9, "{}", measurement(&s, &horizontal));
    let vertical = handle(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [0, 37.5], "at": [-10, 20] })).unwrap());
    assert!((measurement(&s, &vertical) - 37.5).abs() < 1e-9, "{}", measurement(&s, &vertical));
}

/// A new dimension's text position is known at once, as it is after save and reopen (#2421).
#[test]
fn a_new_dimension_reports_its_text_position() {
    let mut s = Session::new();
    let horizontal = handle(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [100, 0], "at": [50, 20] })).unwrap());
    let vertical = handle(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [0, 37.5], "at": [-20, 18] })).unwrap());
    for h in [&horizontal, &vertical] {
        let (dm, g) = dim_of(&s, h);
        assert!(dm.text_mid.xy().dist(g.text_pos) < 1e-9, "stored {:?}, drawn at {:?}", dm.text_mid, g.text_pos);
        assert!(dm.text_mid.xy().dist(Vec2::ZERO) > 1.0, "still at the origin: {:?}", dm.text_mid);
    }
    assert!((dim_of(&s, &horizontal).0.text_mid.x - 50.0).abs() < 1e-9);
}
