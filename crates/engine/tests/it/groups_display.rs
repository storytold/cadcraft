//! Groups (gap F16): GROUPDISPLAYMODE (one grip at the group's centre, with its bounding box),
//! copies of whole groups becoming new unnamed groups, and GROUPEDIT/CLASSICGROUP for the Object
//! Grouping dialog.

use cadcraft_engine::doc::Handle;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::grips::GripMode;
use cadcraft_engine::{GROUP_GRIP, Session};
use serde_json::{Value, json};

fn h(v: &Value) -> String {
    v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap().to_string()
}

fn hx(s: &str) -> Handle {
    Handle::parse_hex(s).unwrap()
}

/// Two lines grouped as "G", and a loose circle.
fn setup() -> (Session, String, String, String) {
    let mut s = Session::new();
    let a = h(&s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap());
    let b = h(&s.execute("line", &json!({ "points": [[0, 0], [0, 4]] })).unwrap());
    let c = h(&s.execute("circle", &json!({ "center": [50, 50], "radius": 1 })).unwrap());
    s.execute("group", &json!({ "name": "G", "handles": [a, b], "description": "frame" })).unwrap();
    (s, a, b, c)
}

#[test]
fn a_selected_group_shows_one_grip_at_its_centre() {
    let (mut s, a, b, c) = setup();
    s.set_selection(vec![hx(&a), hx(&b)]);
    let gs = s.group_grips();
    assert_eq!(gs.len(), 1);
    assert_eq!(gs[0].name, "G");
    assert!(gs[0].frame, "GROUPDISPLAYMODE 2 (default): the bounding box too");
    assert!(gs[0].center.near(Vec2::new(5.0, 2.0), 1e-9), "{:?}", gs[0].center);
    // Part of a group selected: no group grip.
    s.set_selection(vec![hx(&a), hx(&c)]);
    assert!(s.group_grips().is_empty());
    // Mode 1: the grip without the box; mode 0: every member's grips.
    s.execute("setvar", &json!({ "name": "GROUPDISPLAYMODE", "value": 1 })).unwrap();
    s.set_selection(vec![hx(&a), hx(&b)]);
    assert!(!s.group_grips()[0].frame);
    s.execute("setvar", &json!({ "name": "GROUPDISPLAYMODE", "value": 0 })).unwrap();
    assert!(s.group_grips().is_empty());
    // Group selection off: members are plain objects.
    s.execute("setvar", &json!({ "name": "GROUPDISPLAYMODE", "value": 2 })).unwrap();
    s.execute("pickstyle", &json!({ "value": 0 })).unwrap();
    assert!(s.group_grips().is_empty());
}

#[test]
fn dragging_the_group_grip_moves_the_group() {
    let (mut s, a, b, c) = setup();
    s.set_selection(vec![hx(&a), hx(&b)]);
    s.start_grip_edit(&[(hx(&a), GROUP_GRIP)]).unwrap();
    s.input(cadcraft_engine::Input::Point(Vec2::new(15.0, 2.0))).unwrap();
    let ext = |k: &str| match &s.doc().unwrap().entity(hx(k)).unwrap().kind {
        cadcraft_engine::doc::EntityKind::Line(l) => (l.a.x, l.b.x),
        _ => (f64::NAN, f64::NAN),
    };
    assert_eq!(ext(&a), (10.0, 20.0));
    assert_eq!(ext(&b), (10.0, 10.0));
    let _ = c;
    // The grip_edit API with the group grip index does the same.
    s.set_selection(vec![hx(&a), hx(&b)]);
    s.grip_edit(hx(&a), GROUP_GRIP, Vec2::new(5.0, 2.0), GripMode::Move).ok();
    // A grip index that is no group's is refused.
    s.set_selection(vec![hx(&c)]);
    assert!(s.start_grip_edit(&[(hx(&c), GROUP_GRIP)]).is_err());
}

#[test]
fn copying_a_whole_group_makes_a_new_unnamed_group() {
    let (mut s, a, b, c) = setup();
    s.execute("copy", &json!({ "handles": [a, b], "delta": [100, 0] })).unwrap();
    let groups = s.execute("groups.list", &json!({})).unwrap();
    let g = groups["groups"].as_array().unwrap();
    assert_eq!(g.len(), 2);
    assert_eq!(g[1]["name"], "*A1");
    assert_eq!(g[1]["description"], "frame");
    assert_eq!(g[1]["members"].as_array().unwrap().len(), 2);
    assert!(g[1]["members"].as_array().unwrap().iter().all(|m| m != &json!(a) && m != &json!(b)));
    // Mirroring with a copy too; copying part of a group or loose objects makes none.
    s.execute("mirror", &json!({ "handles": [a, b], "p1": [0, -10], "p2": [10, -10] })).unwrap();
    assert_eq!(s.execute("groups.list", &json!({})).unwrap()["groups"].as_array().unwrap().len(), 3);
    s.execute("copy", &json!({ "handles": [a, c], "delta": [0, 100] })).unwrap();
    assert_eq!(s.execute("groups.list", &json!({})).unwrap()["groups"].as_array().unwrap().len(), 3);
    // Undo takes the copy and its group back together.
    s.execute("undo", &json!({})).unwrap();
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.execute("groups.list", &json!({})).unwrap()["groups"].as_array().unwrap().len(), 2);
}

#[test]
fn object_grouping_edits() {
    let (mut s, a, _, c) = setup();
    let l = s.execute("classicgroup", &json!({})).unwrap();
    assert_eq!(l["groups"][0]["name"], "G");
    s.execute("groupedit", &json!({ "name": "G", "description": "walls", "selectable": false, "add": [c], "remove": [a] })).unwrap();
    let g = &s.execute("groups.list", &json!({})).unwrap()["groups"][0];
    assert_eq!((g["description"].as_str(), g["selectable"].as_bool()), (Some("walls"), Some(false)));
    assert_eq!(g["members"].as_array().unwrap().len(), 2);
    // Not selectable: no group grip.
    let members: Vec<Handle> = g["members"].as_array().unwrap().iter().map(|m| hx(m.as_str().unwrap())).collect();
    s.set_selection(members);
    assert!(s.group_grips().is_empty());
}
