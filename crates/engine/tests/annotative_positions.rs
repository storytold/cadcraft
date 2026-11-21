//! An annotative object can sit somewhere else at one of its scales: its grips move it at the
//! current annotation scale only (when it supports several), MOVE and ROTATE move every scale,
//! and ANNORESET puts every scale back where the current one is.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{AnnoDraw, Entity, EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use serde_json::{Value, json};

fn setvar(s: &mut Session, name: &str, value: Value) {
    s.execute("setvar", &json!({ "name": name, "value": value })).unwrap();
}

fn handle(r: &Value) -> Handle {
    r["handle"].as_str().or_else(|| r["handles"][0].as_str()).and_then(Handle::parse_hex).unwrap()
}

fn entity(s: &Session, h: Handle) -> Entity {
    (**s.doc().unwrap().entity(h).unwrap()).clone()
}

/// Annotative text at the origin supporting 1:1 and 1:50, with 1:50 current.
fn two_scales() -> (Session, Handle) {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Notes", "annotative": true, "current": true })).unwrap();
    let h = handle(&s.execute("text", &json!({ "at": [0, 0], "text": "NOTE", "height": 0.25 })).unwrap());
    setvar(&mut s, "CANNOSCALE", json!("1:50"));
    s.execute("objectscale", &json!({ "handles": [h.hex()], "add": "1:50" })).unwrap();
    (s, h)
}

/// Where `h` shows at annotation scale `name`.
fn shown_at(s: &mut Session, h: Handle, name: &str) -> Vec2 {
    setvar(s, "CANNOSCALE", json!(name));
    let d = s.doc().unwrap();
    match d.anno_shown(d.entity(h).unwrap()) {
        EntityKind::Text(t) => t.insert.xy(),
        EntityKind::MText(t) => t.insert.xy(),
        EntityKind::MLeader(m) => m.landing.xy(),
        EntityKind::Dimension(dm) => dm.text_mid.xy(),
        k => panic!("{k:?}"),
    }
}

fn near(a: Vec2, b: Vec2) -> bool {
    a.dist(b) < 1e-9
}

#[test]
fn a_grip_moves_text_at_the_current_scale_only() {
    let (mut s, h) = two_scales();
    s.execute("grip.move", &json!({ "handle": h.hex(), "index": 0, "to": [5, 5] })).unwrap();
    // The grips show where the object is.
    assert!(near(s.grips_of(h).unwrap()[0], Vec2::new(5.0, 5.0)));
    assert!(near(shown_at(&mut s, h, "1:50"), Vec2::new(5.0, 5.0)));
    assert!(near(shown_at(&mut s, h, "1:1"), Vec2::ZERO));
}

#[test]
fn undo_takes_a_move_at_one_scale_back() {
    let (mut s, h) = two_scales();
    s.execute("grip.move", &json!({ "handle": h.hex(), "index": 0, "to": [5, 5] })).unwrap();
    assert_eq!(entity(&s, h).common.anno_moves.len(), 1);
    s.execute("undo", &json!({})).unwrap();
    assert!(entity(&s, h).common.anno_moves.is_empty());
}

#[test]
fn move_and_rotate_carry_every_scale() {
    let (mut s, h) = two_scales();
    s.execute("grip.move", &json!({ "handle": h.hex(), "index": 0, "to": [5, 5] })).unwrap();
    s.execute("move", &json!({ "handles": [h.hex()], "delta": [1, 0] })).unwrap();
    assert!(near(shown_at(&mut s, h, "1:50"), Vec2::new(6.0, 5.0)));
    assert!(near(shown_at(&mut s, h, "1:1"), Vec2::new(1.0, 0.0)));
    s.execute("rotate", &json!({ "handles": [h.hex()], "base": [0, 0], "angle": 90 })).unwrap();
    assert!(near(shown_at(&mut s, h, "1:50"), Vec2::new(-5.0, 6.0)));
    assert!(near(shown_at(&mut s, h, "1:1"), Vec2::new(0.0, 1.0)));
}

#[test]
fn annoreset_puts_every_scale_at_the_current_one() {
    let (mut s, h) = two_scales();
    s.execute("grip.move", &json!({ "handle": h.hex(), "index": 0, "to": [5, 5] })).unwrap();
    setvar(&mut s, "CANNOSCALE", json!("1:50"));
    let r = s.execute("annoreset", &json!({ "handles": [h.hex()] })).unwrap();
    assert_eq!(r["reset"], 1, "{r}");
    assert!(entity(&s, h).common.anno_moves.is_empty());
    assert!(near(shown_at(&mut s, h, "1:1"), Vec2::new(5.0, 5.0)));
    assert!(near(shown_at(&mut s, h, "1:50"), Vec2::new(5.0, 5.0)));
}

#[test]
fn removing_a_scale_drops_its_position() {
    let (mut s, h) = two_scales();
    s.execute("grip.move", &json!({ "handle": h.hex(), "index": 0, "to": [5, 5] })).unwrap();
    s.execute("objectscale", &json!({ "handles": [h.hex()], "delete": "1:50" })).unwrap();
    assert!(entity(&s, h).common.anno_moves.is_empty());
}

#[test]
fn an_object_with_one_scale_moves_everywhere() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Notes", "annotative": true, "current": true })).unwrap();
    setvar(&mut s, "CANNOSCALE", json!("1:50"));
    let h = handle(&s.execute("text", &json!({ "at": [0, 0], "text": "NOTE", "height": 0.25 })).unwrap());
    s.execute("grip.move", &json!({ "handle": h.hex(), "index": 0, "to": [5, 5] })).unwrap();
    let e = entity(&s, h);
    assert!(e.common.anno_moves.is_empty());
    let EntityKind::Text(t) = &e.kind else { panic!() };
    assert!(near(t.insert.xy(), Vec2::new(5.0, 5.0)));
}

#[test]
fn a_multileader_moves_its_content_and_keeps_its_arrowhead() {
    let mut s = Session::new();
    s.execute("mleaderstyle", &json!({ "name": "Anno", "annotative": true })).unwrap();
    let h = handle(&s.execute("mleader", &json!({ "points": [[0, 0], [5, 5]], "text": "A" })).unwrap());
    setvar(&mut s, "CANNOSCALE", json!("1:50"));
    s.execute("objectscale", &json!({ "handles": [h.hex()], "add": "1:50" })).unwrap();
    let EntityKind::MLeader(m) = entity(&s, h).kind else { panic!() };
    let landing = m.leaders.iter().map(Vec::len).sum::<usize>();
    let to = m.landing.xy() + Vec2::new(2.0, 1.0);
    s.execute("grip.move", &json!({ "handle": h.hex(), "index": landing, "to": [to.x, to.y] })).unwrap();
    assert!(near(shown_at(&mut s, h, "1:50"), to));
    assert!(near(shown_at(&mut s, h, "1:1"), m.landing.xy()));
    setvar(&mut s, "CANNOSCALE", json!("1:50"));
    let d = s.doc().unwrap();
    let EntityKind::MLeader(shown) = d.anno_shown(d.entity(h).unwrap()) else { panic!() };
    assert!(near(shown.leaders[0][0].xy(), Vec2::ZERO), "the arrowhead stays");
}

#[test]
fn a_dimension_moves_its_text_at_one_scale() {
    let mut s = Session::new();
    s.execute("dimstyle", &json!({ "name": "Anno", "annotative": true, "current": true })).unwrap();
    let h = handle(&s.execute("dimaligned", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 2] })).unwrap());
    setvar(&mut s, "CANNOSCALE", json!("1:2"));
    s.execute("objectscale", &json!({ "handles": [h.hex()], "add": "1:2" })).unwrap();
    s.execute("grip.move", &json!({ "handle": h.hex(), "index": 3, "to": [20, 20] })).unwrap();
    assert!(near(shown_at(&mut s, h, "1:2"), Vec2::new(20.0, 20.0)));
    let e = entity(&s, h);
    let EntityKind::Dimension(dm) = &e.kind else { panic!() };
    assert!(!near(dm.text_mid.xy(), Vec2::new(20.0, 20.0)), "the shared text position stays");
    assert_eq!(e.common.anno_moves.len(), 1);
}

#[test]
fn shown_draws_through_the_offset() {
    let (mut s, h) = two_scales();
    s.execute("grip.move", &json!({ "handle": h.hex(), "index": 0, "to": [5, 5] })).unwrap();
    setvar(&mut s, "CANNOSCALE", json!("1:50"));
    let d = s.doc().unwrap();
    assert!(matches!(d.anno_draw(d.entity(h).unwrap()), AnnoDraw::Scaled { by, .. } if near(by, Vec2::new(5.0, 5.0))));
    let b = cadcraft_engine::doc::entity_bounds(d, d.entity(h).unwrap(), 0);
    assert!(b.min.x > 4.0 && b.min.y > 4.0, "{b:?}");
}

#[test]
fn hostile_offsets_never_panic() {
    let (mut s, h) = two_scales();
    for to in [json!([1e308, -1e308]), json!([f64::MAX, 0]), json!(["x", 1])] {
        let _ = s.execute("grip.move", &json!({ "handle": h.hex(), "index": 0, "to": to }));
    }
    s.doc_mut().unwrap().modify_entity(h, |e| e.common.move_at("1:50", Vec2::new(f64::NAN, 1.0))).unwrap();
    let _ = s.execute("annoreset", &json!({ "handles": [h.hex()] }));
    let _ = s.execute("rotate", &json!({ "handles": [h.hex()], "base": [0, 0], "angle": 1e300 }));
    let d = s.doc().unwrap();
    let _ = cadcraft_engine::render::build(d, &cadcraft_engine::doc::Space::Model, &Default::default());
    let _ = bounds(d, h);
}

fn bounds(d: &cadcraft_engine::doc::Drawing, h: Handle) -> cadcraft_engine::geom::Bounds2 {
    cadcraft_engine::doc::entity_bounds(d, d.entity(h).unwrap(), 0)
}

#[test]
fn in_a_viewport_grips_and_annoreset_use_the_viewport_s_scale() {
    // The text moved at 1:50; CANNOSCALE is 1:1 while a 1:50 viewport is active (MSPACE): the
    // grips show where the viewport draws it, a grip moves it at 1:50, ANNORESET takes 1:50's place.
    let (mut s, h) = two_scales();
    s.execute("grip.move", &json!({ "handle": h.hex(), "index": 0, "to": [5, 5] })).unwrap();
    setvar(&mut s, "CANNOSCALE", json!("1:1"));
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    let vp = s.execute("mview", &json!({ "p1": [0, 0], "p2": [200, 100] })).unwrap();
    let vh = vp["viewports"][0].as_str().and_then(Handle::parse_hex).unwrap();
    s.execute("viewport.set", &json!({ "handle": vh.hex(), "scale": "1:50", "center": [0, 0] })).unwrap();
    s.execute("mspace", &json!({ "handle": vh.hex() })).unwrap();
    assert!((s.anno_view().unwrap() - 50.0).abs() < 1e-9);
    assert!(near(s.grips_of(h).unwrap()[0], Vec2::new(5.0, 5.0)), "{:?}", s.grips_of(h));
    s.execute("grip.move", &json!({ "handle": h.hex(), "index": 0, "to": [7, 7] })).unwrap();
    assert!(near(s.grips_of(h).unwrap()[0], Vec2::new(7.0, 7.0)));
    s.execute("annoreset", &json!({ "handles": [h.hex()] })).unwrap();
    s.execute("pspace", &json!({})).unwrap();
    s.execute("layout.set", &json!({ "name": "Model" })).unwrap();
    // 1:1 was never moved by the grips, and now sits where 1:50 is.
    assert!(near(shown_at(&mut s, h, "1:1"), Vec2::new(7.0, 7.0)));
    assert!(near(shown_at(&mut s, h, "1:50"), Vec2::new(7.0, 7.0)));
}
