//! Prompt keywords that change how the next value is read: ELLIPSE Rotation and Parameter, BREAK
//! First point.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;

fn ellipses(s: &Session) -> Vec<cadcraft_engine::doc::Ellipse> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Ellipse(el) = &e.kind { Some(el.clone()) } else { None }).collect()
}

fn lines(s: &Session) -> Vec<((f64, f64), (f64, f64))> {
    let mut v: Vec<_> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some(((l.a.x, l.a.y), (l.b.x, l.b.y))) } else { None })
        .collect();
    v.sort_by(|a, b| a.0.0.total_cmp(&b.0.0));
    v
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn ellipse_rotation_sets_the_axis_ratio() {
    let mut s = Session::new();
    s.cmdline("ELLIPSE 0,0 10,0 R 60").unwrap();
    assert!(s.running.is_none());
    let e = ellipses(&s);
    assert_eq!(e.len(), 1);
    let e = &e[0];
    assert!(near(e.center.x, 5.0) && near(e.center.y, 0.0), "center {:?}", e.center);
    assert!(near(e.major.x.abs(), 5.0) && near(e.major.y, 0.0), "major {:?}", e.major);
    assert!(near(e.ratio, 0.5), "ratio {}", e.ratio);
    // Center mode, via a script.
    let mut s = Session::new();
    s.script("ELLIPSE C 0,0 0,8 R 0\n").unwrap();
    let e = ellipses(&s);
    assert_eq!(e.len(), 1);
    assert!(near(e[0].ratio, 1.0) && near(e[0].major.y, 8.0), "{:?}", e[0]);
    // Out of range: re-prompts instead of drawing a degenerate ellipse.
    let mut s = Session::new();
    s.cmdline("ELLIPSE 0,0 10,0 R 90").unwrap();
    assert!(s.running.is_some());
    assert!(ellipses(&s).is_empty());
}

#[test]
fn elliptical_arc_parameter_values() {
    let mut s = Session::new();
    // A 10 x 5 ellipse: the angle 45° is the parameter atan2(sin45/5, cos45/10) ≠ 45°; with
    // Parameter the values are taken as given.
    s.cmdline("ELLIPSE A 0,0 20,0 5 P 45 A 180").unwrap();
    assert!(s.running.is_none());
    let e = ellipses(&s);
    assert_eq!(e.len(), 1);
    assert!(near(e[0].start, 45f64.to_radians()), "start {}", e[0].start);
    // `A` switches the end value back to an angle; 180° is parameter π either way.
    assert!(near(e[0].end, std::f64::consts::PI), "end {}", e[0].end);
}

#[test]
fn break_first_point_option() {
    let mut s = Session::new();
    s.cmdline("LINE 0,0 10,0").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("BREAK 5,0 F 2,0 6,0").unwrap();
    assert!(s.running.is_none());
    assert_eq!(lines(&s), vec![((0.0, 0.0), (2.0, 0.0)), ((6.0, 0.0), (10.0, 0.0))]);
}
