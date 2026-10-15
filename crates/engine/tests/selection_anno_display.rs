//! A selected annotative object also shows at its other scales, faded (SELECTIONANNODISPLAY):
//! each at that scale's size and position. Grips stay on the current scale's representation.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{AnnoScale, Entity, EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use serde_json::{Value, json};

fn setvar(s: &mut Session, name: &str, value: Value) {
    s.execute("setvar", &json!({ "name": name, "value": value })).unwrap();
}

fn getvar(s: &mut Session, name: &str) -> Value {
    s.execute("getvar", &json!({ "name": name })).unwrap()["value"].clone()
}

fn handle(r: &Value) -> Handle {
    r["handle"].as_str().or_else(|| r["handles"][0].as_str()).and_then(Handle::parse_hex).unwrap()
}

fn entity(s: &Session, h: Handle) -> Entity {
    (**s.doc().unwrap().entity(h).unwrap()).clone()
}

fn near(a: Vec2, b: Vec2) -> bool {
    a.dist(b) < 1e-9
}

/// An annotative multileader supporting 1:1 and 1:10, its landing moved (2, 1) at 1:10, with
/// 1:1 current again. Returns it as it shows at 1:10.
fn moved_at_one_to_ten() -> (Session, Handle, Entity) {
    let mut s = Session::new();
    s.execute("mleaderstyle", &json!({ "name": "Anno", "annotative": true })).unwrap();
    let h = handle(&s.execute("mleader", &json!({ "points": [[0, 0], [5, 5]], "text": "A" })).unwrap());
    setvar(&mut s, "CANNOSCALE", json!("1:10"));
    s.execute("objectscale", &json!({ "handles": [h.hex()], "add": "1:10" })).unwrap();
    let EntityKind::MLeader(m) = entity(&s, h).kind else { panic!() };
    let landing = m.leaders.iter().map(Vec::len).sum::<usize>();
    let to = m.landing.xy() + Vec2::new(2.0, 1.0);
    s.execute("grip.move", &json!({ "handle": h.hex(), "index": landing, "to": [to.x, to.y] })).unwrap();
    let d = s.doc().unwrap();
    let e = d.entity(h).unwrap();
    let at_ten = Entity { kind: d.anno_shown(e), ..(**e).clone() };
    setvar(&mut s, "CANNOSCALE", json!("1:1"));
    (s, h, at_ten)
}

#[test]
fn a_selected_multileader_shows_its_other_scale_where_it_was_moved() {
    let (mut s, h, at_ten) = moved_at_one_to_ten();
    assert_eq!(getvar(&mut s, "SELECTIONANNODISPLAY"), json!(1), "on by default");
    assert!(s.selection_anno_others().is_empty(), "nothing selected");
    s.set_selection(vec![h]);
    let others = s.selection_anno_others();
    assert_eq!(others.len(), 1, "one other scale");
    let (EntityKind::MLeader(other), EntityKind::MLeader(ten)) = (&others[0].kind, &at_ten.kind) else { panic!() };
    assert!(near(other.landing.xy(), ten.landing.xy()), "at 1:10's position");
    assert_eq!(other, ten, "at 1:10's size too");
    assert!(others[0].common.anno_scales.is_empty(), "drawn as it is");
    // The grips stay on the 1:1 representation.
    let EntityKind::MLeader(m) = entity(&s, h).kind else { panic!() };
    let grips = s.grips_of(h).unwrap();
    assert!(grips.iter().any(|g| near(*g, m.landing.xy())));
    assert!(!grips.iter().any(|g| near(*g, ten.landing.xy())));
    // At 1:10 the other one is 1:1's, where it was.
    setvar(&mut s, "CANNOSCALE", json!("1:10"));
    let others = s.selection_anno_others();
    let [EntityKind::MLeader(one)] = others.iter().map(|e| &e.kind).collect::<Vec<_>>()[..] else { panic!() };
    assert!(near(one.landing.xy(), m.landing.xy()));
}

#[test]
fn selectionannodisplay_0_shows_no_other_scale() {
    let (mut s, h, _) = moved_at_one_to_ten();
    s.set_selection(vec![h]);
    s.execute("setvar", &json!({ "name": "SELECTIONANNODISPLAY", "value": 2 })).unwrap_err();
    s.execute("setvar", &json!({ "name": "SELECTIONANNODISPLAY", "value": "x" })).unwrap_err();
    assert_eq!(getvar(&mut s, "SELECTIONANNODISPLAY"), json!(1), "a refused value changes nothing");
    setvar(&mut s, "SELECTIONANNODISPLAY", json!(0));
    assert!(s.selection_anno_others().is_empty());
    s.cmdline("SELECTIONANNODISPLAY 1").unwrap();
    assert_eq!(s.selection_anno_others().len(), 1, "typed at the command line");
}

#[test]
fn a_single_scale_or_plain_object_shows_nothing_more() {
    let mut s = Session::new();
    s.execute("mleaderstyle", &json!({ "name": "Anno", "annotative": true })).unwrap();
    let one = handle(&s.execute("mleader", &json!({ "points": [[0, 0], [5, 5]], "text": "A" })).unwrap());
    let line = handle(&s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap());
    s.set_selection(vec![one, line]);
    assert_eq!(entity(&s, one).common.anno_scales.len(), 1);
    assert!(s.selection_anno_others().is_empty());
}

#[test]
fn hostile_scales_give_bounded_representations() {
    let (s, h, _) = moved_at_one_to_ten();
    let mut e = entity(&s, h);
    // Invalid ratios are skipped; the same ratio under many names comes once; the list is capped.
    e.common.anno_scales.push(AnnoScale::new("nan", f64::NAN, 1.0));
    e.common.anno_scales.push(AnnoScale::new("zero", 0.0, 1.0));
    e.common.anno_scales.push(AnnoScale::new("inf", 1.0, f64::INFINITY));
    e.common.anno_scales.push(AnnoScale::new("huge", 1e-308, 1e308));
    for i in 0..5000 {
        e.common.anno_scales.push(AnnoScale::new(&format!("dup{i}"), 1.0, 10.0));
    }
    for view in [1.0, 10.0, f64::NAN, -1.0, 0.0, f64::INFINITY] {
        let others = cadcraft_engine::doc::anno_others(&e, view, true);
        assert!(others.len() <= 2, "{view}: {}", others.len());
    }
    assert!(cadcraft_engine::doc::anno_others(&e, 3.0, false).is_empty(), "not shown in that view");
}
