//! LEADER is annotative when its dimension style is (gaps F12): the leader and its text keep
//! their paper size at every annotation scale they support, and hide at others with
//! ANNOALLVISIBLE 0, like dimensions and multileaders.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, EntityKind, Handle, Leader, Space};
use serde_json::{Value, json};

fn setvar(s: &mut Session, name: &str, value: Value) {
    s.execute("setvar", &json!({ "name": name, "value": value })).unwrap();
}

fn scales(s: &Session, h: Handle) -> Vec<String> {
    s.doc().unwrap().entity(h).unwrap().common.anno_scales.iter().map(|x| x.name.clone()).collect()
}

/// Bounds of what the renderer draws for `h` in model space.
fn drawn(s: &Session, h: Handle) -> Option<cadcraft_engine::geom::Bounds2> {
    let list = cadcraft_engine::render::build(s.doc().unwrap(), &Space::Model, &Default::default());
    let mut b = cadcraft_engine::geom::Bounds2::EMPTY;
    for p in list.prims.iter().filter(|p| p.handle == h) {
        for q in list.points(p) {
            b.add(*q);
        }
    }
    (!b.is_empty()).then_some(b)
}

/// A horizontal leader with text "NOTE": the leader's drawn height is its arrowhead's width.
fn leader(s: &mut Session) -> (Handle, Handle) {
    let r = s.execute("leader", &json!({ "points": [[0, 0], [50, 0]], "text": "NOTE" })).unwrap();
    let h = |i: usize| Handle::parse_hex(r["handles"][i].as_str().unwrap()).unwrap();
    (h(0), h(1))
}

#[test]
fn a_leader_of_an_annotative_style_keeps_its_paper_size() {
    let mut s = Session::new();
    s.execute("dimstyle", &json!({ "name": "Anno", "annotative": true, "current": true })).unwrap();
    let (l, t) = leader(&mut s);
    assert_eq!(scales(&s, l), ["1:1"]);
    assert_eq!(scales(&s, t), ["1:1"], "its text is annotative too");
    let arrow_1 = drawn(&s, l).unwrap().height();
    let text_1 = drawn(&s, t).unwrap().height();
    setvar(&mut s, "ANNOAUTOSCALE", json!(1));
    setvar(&mut s, "CANNOSCALE", json!("1:4"));
    assert_eq!(scales(&s, l), ["1:1", "1:4"]);
    let arrow_4 = drawn(&s, l).unwrap().height();
    let text_4 = drawn(&s, t).unwrap().height();
    assert!((arrow_4 / arrow_1 - 4.0).abs() < 1e-6, "{arrow_1} → {arrow_4}");
    assert!((text_4 / text_1 - 4.0).abs() < 1e-6, "{text_1} → {text_4}");
    // The leader's line keeps its length: only the arrowhead grows.
    assert!((drawn(&s, l).unwrap().width() - 50.0).abs() < 1e-6);

    // A scale it doesn't support hides it while ANNOALLVISIBLE is 0.
    setvar(&mut s, "ANNOAUTOSCALE", json!(-4));
    setvar(&mut s, "ANNOALLVISIBLE", json!(0));
    setvar(&mut s, "CANNOSCALE", json!("1:10"));
    assert!(drawn(&s, l).is_none() && drawn(&s, t).is_none());
    // Shown again at its first scale with ANNOALLVISIBLE 1.
    setvar(&mut s, "ANNOALLVISIBLE", json!(1));
    assert!((drawn(&s, l).unwrap().height() / arrow_1 - 1.0).abs() < 1e-6);
}

#[test]
fn plain_styles_make_plain_leaders_and_annoupdate_follows_the_style() {
    let mut s = Session::new();
    s.execute("dimstyle", &json!({ "name": "Notes", "current": true })).unwrap();
    let (l, t) = leader(&mut s);
    assert!(scales(&s, l).is_empty() && scales(&s, t).is_empty());
    s.execute("dimstyle", &json!({ "name": "Notes", "annotative": true })).unwrap();
    s.execute("annoupdate", &json!({ "handles": [l.hex()] })).unwrap();
    assert_eq!(scales(&s, l), ["1:1"]);
    s.execute("objectscale", &json!({ "handles": [l.hex()], "add": "1:2" })).unwrap();
    assert_eq!(scales(&s, l), ["1:1", "1:2"]);
    // Properties' Annotative turns it off and on.
    s.execute("properties.set", &json!({ "handles": [l.hex()], "annotative": false })).unwrap();
    assert!(scales(&s, l).is_empty());
    s.execute("properties.set", &json!({ "handles": [l.hex()], "annotative": true })).unwrap();
    assert_eq!(scales(&s, l), ["1:1"]);
}

#[test]
fn hostile_leader_scales_never_panic() {
    let mut s = Session::new();
    let d = s.doc_mut().unwrap();
    for bad in [json!(-5), json!(0), json!("x"), json!(1e308), json!(null), json!([1])] {
        let mut overrides = serde_json::Map::new();
        overrides.insert("scale".into(), bad);
        let k = EntityKind::Leader(Leader {
            vertices: vec![Default::default(), cadcraft_engine::geom::Vec3::new(5.0, 0.0, 0.0)],
            arrow: true,
            spline: false,
            style: "Standard".into(),
            overrides,
        });
        // Plain, the leader's own value is used; annotative, the annotation scale replaces it.
        d.add(&Space::Model, Common::default(), k.clone()).unwrap();
        d.add(&Space::Model, Common { anno_scales: vec![cadcraft_engine::doc::AnnoScale::unit()], ..Default::default() }, k).unwrap();
    }
    let list = cadcraft_engine::render::build(s.doc().unwrap(), &Space::Model, &Default::default());
    assert!(!list.prims.is_empty());
    let b = s.doc().unwrap().extents(&Space::Model);
    assert!(b.is_empty() || (b.min.is_finite() && b.max.is_finite()));
}
