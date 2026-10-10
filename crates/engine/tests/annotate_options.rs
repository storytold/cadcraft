//! Options listed at the dimension and multileader prompts take effect (#243).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{DimKind, Dimension, EntityKind, MLeader};
use cadcraft_engine::geom::Vec2;

fn last(s: &Session) -> EntityKind {
    s.doc().unwrap().model.last().unwrap().kind.clone()
}

fn last_dim(s: &Session) -> Dimension {
    match last(s) {
        EntityKind::Dimension(d) => d,
        other => panic!("not a dimension: {other:?}"),
    }
}

fn last_mleader(s: &Session) -> MLeader {
    match last(s) {
        EntityKind::MLeader(m) => m,
        other => panic!("not a multileader: {other:?}"),
    }
}

fn value(s: &Session, d: &Dimension) -> String {
    cadcraft_engine::render::dimension_in(s.doc().unwrap(), d).value
}

fn near(a: Vec2, b: Vec2) -> bool {
    a.dist(b) < 1e-6
}

#[test]
fn dimlinear_angle_rotates_the_text_and_text_replaces_it() {
    let mut s = Session::new();
    s.script("dimlinear 0,0 10,0 a 30 t\nX=<>\n5,-3\n").unwrap();
    let d = last_dim(&s);
    assert_eq!(d.kind, DimKind::Linear { rotation: 0.0 }, "Angle must not rotate the dimension line");
    assert!((d.text_rotation - 30f64.to_radians()).abs() < 1e-9, "{}", d.text_rotation);
    assert_eq!(d.text, "X=<>");
    assert!(s.running.is_none());
}

#[test]
fn dimlinear_vertical_forces_the_orientation() {
    let mut s = Session::new();
    s.script("dimlinear 0,0 10,5 v").unwrap();
    let p = s.prompt_text();
    assert!(p.contains("[Mtext/Text/Angle]"), "{p}");
    s.script("5,-3\n").unwrap();
    assert_eq!(last_dim(&s).kind, DimKind::Linear { rotation: std::f64::consts::FRAC_PI_2 });
}

#[test]
fn dimordinate_datum_and_text_options() {
    let mut s = Session::new();
    // The leader endpoint alone would make a Y datum; Xdatum forces X, and Text is a text option.
    s.script("dimordinate 0,0 x t\nA <>\n10,2\n").unwrap();
    let d = last_dim(&s);
    assert_eq!(d.kind, DimKind::Ordinate { x_type: true });
    assert_eq!(d.text, "A <>");
    s.script("dimordinate 0,0 y a 90 2,10\n").unwrap();
    let d = last_dim(&s);
    assert_eq!(d.kind, DimKind::Ordinate { x_type: false });
    assert!((d.text_rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
}

#[test]
fn dimradius_text_options() {
    let mut s = Session::new();
    s.script("circle 0,0 5\ndimradius 5,0 t\nR=<>\na 45 8,8\n").unwrap();
    let d = last_dim(&s);
    assert_eq!(d.kind, DimKind::Radius);
    assert_eq!(d.text, "R=<>");
    assert!((d.text_rotation - 45f64.to_radians()).abs() < 1e-9);
}

#[test]
fn dimangular_quadrant_locks_the_measured_angle() {
    let mut s = Session::new();
    // Placed at (-3,-4) the arc would measure the 270° side; the quadrant keeps the 90° one.
    s.script("dimangular\n\n0,0 10,0 0,10 q 5,5 -3,-4\n").unwrap();
    let d = last_dim(&s);
    assert_eq!(value(&s, &d), "90°");
    assert!(near(d.defpt.xy(), Vec2::new(5.0, 5.0).normalized() * 5.0), "{:?}", d.defpt);
}

#[test]
fn dimarc_partial_measures_part_of_the_arc() {
    let mut s = Session::new();
    s.script("arc c 0,0 -10,0 10,0\n").unwrap();
    assert!(matches!(last(&s), EntityKind::Arc(_)), "{:?}", last(&s));
    s.script("dimarc 0,-10 p 10,0 0,-10 5,-12\n").unwrap();
    let d = last_dim(&s);
    assert_eq!(d.kind, DimKind::ArcLength);
    assert!(near(d.p13.xy(), Vec2::new(0.0, -10.0)) && near(d.p14.xy(), Vec2::new(10.0, 0.0)), "{d:?}");
    assert_eq!(value(&s, &d), "⌒15.7080");
}

#[test]
fn mleader_landing_first_and_content_first() {
    let mut s = Session::new();
    s.script("mleader l 10,5 0,0 note\n").unwrap();
    let m = last_mleader(&s);
    assert_eq!(m.leaders.len(), 1);
    assert!(near(m.leaders[0][0].xy(), Vec2::ZERO) && near(m.landing.xy(), Vec2::new(10.0, 5.0)), "{m:?}");
    assert_eq!(m.text.unwrap().contents, "note");

    s.script("mleader c 10,5 hello\n0,0\n").unwrap();
    let m = last_mleader(&s);
    assert!(near(m.leaders[0][0].xy(), Vec2::ZERO) && near(m.landing.xy(), Vec2::new(10.0, 5.0)), "{m:?}");
    assert_eq!(m.text.unwrap().contents, "hello");
}

#[test]
fn mleader_options_take_effect() {
    let mut s = Session::new();
    // Enter opens Options; Maxpoints 3 and no landing line.
    s.script("mleader\n\nm 3 a n x 0,0 5,5 10,5 txt\n").unwrap();
    let m = last_mleader(&s);
    assert_eq!(m.leaders[0].len(), 2, "{m:?}");
    assert!(near(m.leaders[0][1].xy(), Vec2::new(5.0, 5.0)) && near(m.landing.xy(), Vec2::new(10.0, 5.0)));
    assert_eq!(m.dogleg, 0.0);
    assert_eq!(m.text.unwrap().contents, "txt");

    // First angle constraint snaps the leader to 45° steps.
    s.script("mleader o f 45 x 0,0 10,8 note\n").unwrap();
    let m = last_mleader(&s);
    let r = Vec2::new(10.0, 8.0).len();
    assert!(near(m.landing.xy(), Vec2::new(r, r) / 2f64.sqrt()), "{:?}", m.landing);

    // Unmodelled choices say so and keep the following input.
    s.script("mleader o l p x 0,0 4,4 hi\n").unwrap();
    assert!(s.log.iter().any(|l| l.contains("not available yet")), "{:?}", s.log);
    assert_eq!(last_mleader(&s).text.unwrap().contents, "hi");
    assert!(s.running.is_none());
}
