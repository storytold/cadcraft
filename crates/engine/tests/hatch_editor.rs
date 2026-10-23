//! The Hatch Editor contextual tab's commands: hatch type (solid, gradient, pattern,
//! user-defined), double, background colour, draw order, Match Properties, boundaries removed by
//! a click, region boundaries, Display Boundary Objects, the boundary set and the default origin.

use cadcraft_engine::doc::{EntityKind, Handle, Hatch};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn hatch(s: &Session, h: &str) -> Hatch {
    match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
        EntityKind::Hatch(x) => x.clone(),
        k => panic!("not a hatch: {k:?}"),
    }
}

fn handle(v: &Value) -> String {
    v["handle"].as_str().unwrap().to_string()
}

fn rect(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    handle(&s.execute("rectang", &json!({ "p1": a, "p2": b })).unwrap())
}

/// A 20 x 20 square with a 5 x 5 island, hatched (ANSI31, associative), and the three handles.
fn square_with_island() -> (Session, String, String, String) {
    let mut s = Session::new();
    let outer = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    let island = rect(&mut s, [5.0, 5.0], [10.0, 10.0]);
    let h = handle(&s.execute("hatch", &json!({ "points": [[2, 2]], "pattern": "ANSI31" })).unwrap());
    (s, outer, island, h)
}

#[test]
fn the_type_list_turns_a_hatch_solid_user_defined_pattern_or_gradient() {
    let (mut s, _, _, h) = square_with_island();
    s.execute("hatchedit", &json!({ "handles": [h], "type": "solid" })).unwrap();
    assert!(hatch(&s, &h).solid);

    s.execute("hatchedit", &json!({ "handles": [h], "type": "user", "spacing": 2.5 })).unwrap();
    let x = hatch(&s, &h);
    assert!(x.is_user() && !x.solid && !x.is_double());
    assert_eq!((x.pattern.as_str(), x.scale, x.pattern_lines.len()), ("_USER", 2.5, 1));

    s.execute("hatchedit", &json!({ "handles": [h], "double": true })).unwrap();
    assert!(hatch(&s, &h).is_double(), "a second family of lines at 90 degrees");

    s.execute("setvar", &json!({ "name": "HPNAME", "value": "ANSI37" })).unwrap();
    s.execute("hatchedit", &json!({ "handles": [h], "type": "pattern" })).unwrap();
    let x = hatch(&s, &h);
    assert_eq!(x.pattern, "ANSI37", "a hatch turned back to a pattern takes HPNAME");
    assert!(x.pattern_lines.is_empty() && !x.is_user());

    s.execute("hatchedit", &json!({ "handles": [h], "type": "gradient" })).unwrap();
    assert!(hatch(&s, &h).gradient.is_some());
    assert!(s.execute("hatchedit", &json!({ "handles": [h], "type": "dotted" })).is_err());
}

/// Turned to a pattern, a hatch takes ANSI31 when HPNAME holds a gradient tile, and a pattern a
/// .pat file loaded with its lines.
#[test]
fn a_hatch_turned_to_a_pattern_never_takes_a_gradient_tile_and_keeps_pat_lines() {
    let (mut s, _, _, h) = square_with_island();
    s.execute("hatchedit", &json!({ "handles": [h], "type": "solid" })).unwrap();
    s.execute("setvar", &json!({ "name": "HPNAME", "value": "GR_LINEAR" })).unwrap();
    s.execute("hatchedit", &json!({ "handles": [h], "type": "pattern" })).unwrap();
    let x = hatch(&s, &h);
    assert_eq!(x.pattern, "ANSI31", "a gradient tile is no pattern");
    assert!(x.pattern_lines.is_empty() && !x.solid && x.gradient.is_none());

    // A second hatch loads EDITGRID from a .pat file's text; HPNAME then names it.
    let pat = "*EDITGRID,Edit grid\n0,0,0,0,.5\n90,0,0,0,.5\n";
    s.execute("rectang", &json!({ "p1": [30, 0], "p2": [40, 10] })).unwrap();
    s.execute("hatch", &json!({ "points": [[35, 5]], "pattern": "EDITGRID", "patternText": pat })).unwrap();
    s.execute("hatchedit", &json!({ "handles": [h], "type": "solid" })).unwrap();
    s.execute("setvar", &json!({ "name": "HPNAME", "value": "EDITGRID" })).unwrap();
    s.execute("hatchedit", &json!({ "handles": [h], "type": "pattern" })).unwrap();
    let x = hatch(&s, &h);
    assert_eq!(x.pattern, "EDITGRID");
    assert_eq!(x.pattern_lines.len(), 2, "the .pat pattern's lines come with it");
}

#[test]
fn background_colour_is_set_and_cleared() {
    let (mut s, _, _, h) = square_with_island();
    s.execute("hatchedit", &json!({ "handles": [h], "background": "3" })).unwrap();
    assert!(hatch(&s, &h).background.is_some());
    s.execute("hatchedit", &json!({ "handles": [h], "background": "none" })).unwrap();
    assert!(hatch(&s, &h).background.is_none());
    assert!(s.execute("hatchedit", &json!({ "handles": [h], "background": "nope" })).is_err());
}

#[test]
fn draw_order_sends_the_hatch_behind_or_in_front_of_its_boundary() {
    let (mut s, outer, _, h) = square_with_island();
    let order = |s: &Session| s.doc().unwrap().model.handles().iter().map(|x| x.hex()).collect::<Vec<_>>();
    let pos = |s: &Session, x: &str| order(s).iter().position(|y| y == x).unwrap();
    s.execute("hatchedit", &json!({ "handles": [h], "drawOrder": "behindBoundary" })).unwrap();
    assert!(pos(&s, &h) < pos(&s, &outer));
    s.execute("hatchedit", &json!({ "handles": [h], "drawOrder": "inFrontOfBoundary" })).unwrap();
    assert!(pos(&s, &h) > pos(&s, &outer));
    s.execute("hatchedit", &json!({ "handles": [h], "drawOrder": "back" })).unwrap();
    assert_eq!(pos(&s, &h), 0);
    assert!(s.execute("hatchedit", &json!({ "handles": [h], "drawOrder": "sideways" })).is_err());
}

#[test]
fn match_properties_copies_the_source_hatch_and_keeps_the_origin_unless_asked() {
    let (mut s, _, _, h) = square_with_island();
    let other = rect(&mut s, [30.0, 0.0], [40.0, 10.0]);
    let src = handle(&s.execute("hatch", &json!({ "handles": [other], "pattern": "ANSI37", "scale": 3, "angle": 30, "origin": [7, 7] })).unwrap());
    s.execute("properties.set", &json!({ "handles": [src], "color": "1" })).unwrap();
    let before = hatch(&s, &h).origin;
    s.execute("hatchedit", &json!({ "handles": [h], "inherit": src })).unwrap();
    let x = hatch(&s, &h);
    assert_eq!((x.pattern.as_str(), x.scale), ("ANSI37", 3.0));
    assert!((x.angle.to_degrees() - 30.0).abs() < 1e-9);
    assert_eq!(x.origin, before, "HPINHERIT 0: the current origin stays");
    let e = s.doc().unwrap().entity(Handle::parse_hex(&h).unwrap()).unwrap().common.color;
    assert_eq!(e.to_aci(), 1, "the colour comes along");
    s.execute("hatchedit", &json!({ "handles": [h], "inherit": src, "inheritOrigin": true })).unwrap();
    assert_eq!(hatch(&s, &h).origin, Vec2::new(7.0, 7.0));
    assert!(s.execute("hatchedit", &json!({ "handles": [h], "inherit": other })).is_err(), "the source must be a hatch");
}

#[test]
fn a_click_near_a_boundary_removes_that_loop() {
    let (mut s, _, _, h) = square_with_island();
    assert_eq!(hatch(&s, &h).loops.len(), 2);
    // On the island's right edge.
    s.execute("hatchedit", &json!({ "handles": [h], "removeAt": [[10.1, 7.0]] })).unwrap();
    let x = hatch(&s, &h);
    assert_eq!(x.loops.len(), 1);
    assert!(x.loops[0].outer, "the outer boundary stays");
    assert!(s.execute("hatchedit", &json!({ "handles": [h], "removeAt": [[0.0, 7.0]] })).is_err(), "the last loop stays");
}

#[test]
fn boundaries_are_recreated_as_regions() {
    let (mut s, _, _, h) = square_with_island();
    let r = s.execute("hatchedit", &json!({ "handles": [h], "recreate": "region" })).unwrap();
    let made = r["handles"].as_array().unwrap();
    assert_eq!(made.len(), 2);
    let d = s.doc().unwrap();
    for m in made {
        let e = d.entity(Handle::parse_hex(m.as_str().unwrap()).unwrap()).unwrap();
        assert!(matches!(e.kind, EntityKind::Region(_)));
    }
}

#[test]
fn display_boundary_objects_selects_them_with_the_hatch() {
    let (mut s, outer, island, h) = square_with_island();
    s.set_selection(vec![Handle::parse_hex(&h).unwrap()]);
    s.execute("hatchedit.showboundary", &json!({})).unwrap();
    let sel: Vec<String> = s.selection().iter().map(|x| x.hex()).collect();
    for x in [&h, &outer, &island] {
        assert!(sel.contains(x), "{x} selected");
    }
}

#[test]
fn a_boundary_set_limits_what_pick_points_find() {
    let mut s = Session::new();
    let outer = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    let inner = rect(&mut s, [5.0, 5.0], [10.0, 10.0]);
    s.execute("hatchboundaryset", &json!({ "handles": [outer] })).unwrap();
    // The inner square isn't in the set: the point inside it is bounded by the outer square.
    let h = handle(&s.execute("hatch", &json!({ "points": [[7, 7]] })).unwrap());
    let x = hatch(&s, &h);
    assert_eq!(x.loops.len(), 1);
    assert_eq!(x.loops[0].sources, vec![Handle::parse_hex(&outer).unwrap()]);
    s.execute("hatchboundaryset", &json!({ "clear": true })).unwrap();
    let h = handle(&s.execute("hatch", &json!({ "points": [[7, 7]] })).unwrap());
    assert_eq!(hatch(&s, &h).loops[0].sources, vec![Handle::parse_hex(&inner).unwrap()]);
}

#[test]
fn pick_points_from_the_ribbon_keep_adding_until_enter() {
    let (mut s, _, _, h) = square_with_island();
    rect(&mut s, [30.0, 0.0], [40.0, 10.0]);
    rect(&mut s, [50.0, 0.0], [60.0, 10.0]);
    s.set_selection(vec![Handle::parse_hex(&h).unwrap()]);
    s.start("hatchedit.points").unwrap();
    s.input(Input::Point(Vec2::new(35.0, 5.0))).unwrap();
    s.input(Input::Point(Vec2::new(55.0, 5.0))).unwrap();
    assert!(s.current_prompt().is_some(), "still picking");
    s.input(Input::Enter).unwrap();
    assert!(s.current_prompt().is_none());
    assert_eq!(hatch(&s, &h).loops.len(), 4);
}

#[test]
fn set_origin_from_the_ribbon_can_store_it_as_the_default() {
    let (mut s, _, _, h) = square_with_island();
    s.hatch_store_origin = true;
    s.set_selection(vec![Handle::parse_hex(&h).unwrap()]);
    s.start("hatchedit.origin").unwrap();
    s.input(Input::Point(Vec2::new(3.0, 4.0))).unwrap();
    assert_eq!(hatch(&s, &h).origin, Vec2::new(3.0, 4.0));
    let o = s.doc().unwrap().header.point("HPORIGIN").unwrap();
    assert_eq!((o.x, o.y), (3.0, 4.0));
}

#[test]
fn user_defined_double_hatches_are_saved_as_such() {
    let (mut s, _, _, h) = square_with_island();
    s.execute("hatchedit", &json!({ "handles": [h], "type": "user", "spacing": 2, "double": true, "angle": 45 })).unwrap();
    let text = cadcraft_io::write_dxf(s.doc().unwrap());
    let groups: Vec<&str> = text.lines().map(str::trim).collect();
    let has = |code: &str, value: &str| groups.chunks(2).any(|g| g == [code, value]);
    assert!(has("2", "_USER") && has("76", "0") && has("77", "1"));
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let x = back.model.iter().find_map(|e| if let EntityKind::Hatch(x) = &e.kind { Some(x.clone()) } else { None }).unwrap();
    assert!(x.is_double(), "read back as a double user-defined hatch");
    assert!((x.scale - 2.0).abs() < 1e-9);
}

/// HATCH follows the HP* variables the Hatch Creation tab sets while it runs.
#[test]
fn hatch_creation_follows_the_hp_variables_while_it_runs() {
    let mut s = Session::new();
    let outer = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    s.start("hatch").unwrap();
    s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    for (name, value) in
        [("HPNAME", json!("_USER")), ("HPDOUBLE", json!(1)), ("HPSCALE", json!(2.5)), ("HPCOLOR", json!("1")), ("HPBOUNDRETAIN", json!(1))]
    {
        s.execute("setvar", &json!({ "name": name, "value": value })).unwrap();
    }
    s.input(Input::Enter).unwrap();
    let d = s.doc().unwrap();
    let (h, color) = d.model.iter().find_map(|e| if let EntityKind::Hatch(x) = &e.kind { Some((x.clone(), e.common.color)) } else { None }).unwrap();
    assert!(h.is_double() && (h.scale - 2.5).abs() < 1e-9);
    assert_eq!(color.to_aci(), 1);
    // A retained boundary: a new polyline besides the rectangle, the hatch linked to it.
    let polys = d.model.iter().filter(|e| matches!(e.kind, EntityKind::LwPolyline(_))).count();
    assert_eq!(polys, 2);
    assert!(h.loops[0].sources.iter().all(|x| x.hex() != outer));
}

/// Hatch Creation's Remove and Match Properties, through HATCH's keywords.
#[test]
fn hatch_creation_removes_a_boundary_and_matches_a_hatch() {
    let mut s = Session::new();
    rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    rect(&mut s, [30.0, 0.0], [40.0, 10.0]);
    let other = rect(&mut s, [50.0, 0.0], [60.0, 10.0]);
    let src = handle(&s.execute("hatch", &json!({ "handles": [other], "pattern": "ANSI37", "scale": 3 })).unwrap());
    s.start("hatch").unwrap();
    s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    s.input(Input::Point(Vec2::new(35.0, 5.0))).unwrap();
    s.input(Input::Keyword("Remove boundaries".into())).unwrap();
    s.input(Input::Point(Vec2::new(40.1, 5.0))).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Keyword("Inherit properties".into())).unwrap();
    s.input(Input::Pick(vec![Handle::parse_hex(&src).unwrap()])).unwrap();
    s.input(Input::Enter).unwrap();
    let d = s.doc().unwrap();
    let made: Vec<Hatch> = d
        .model
        .iter()
        .filter(|e| e.handle.hex() != src)
        .filter_map(|e| if let EntityKind::Hatch(x) = &e.kind { Some(x.clone()) } else { None })
        .collect();
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].loops.len(), 1, "the second square's boundary was removed");
    assert_eq!((made[0].pattern.as_str(), made[0].scale), ("ANSI37", 3.0));
}

#[test]
fn a_boundary_set_also_limits_the_islands() {
    let mut s = Session::new();
    let outer = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    rect(&mut s, [5.0, 5.0], [10.0, 10.0]);
    s.execute("hatchboundaryset", &json!({ "handles": [outer] })).unwrap();
    // Picked between the squares: the inner one is not in the set, so it is no island.
    let h = handle(&s.execute("hatch", &json!({ "points": [[2, 2]] })).unwrap());
    let x = hatch(&s, &h);
    assert_eq!(x.loops.len(), 1, "only the outer square bounds the hatch");
    assert_eq!(x.loops[0].sources, vec![Handle::parse_hex(&outer).unwrap()]);
}

#[test]
fn an_empty_boundary_set_is_refused() {
    let mut s = Session::new();
    rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    assert!(s.execute("hatchboundaryset", &json!({})).is_err(), "no handles and nothing selected");
    // Pick points still find the square.
    s.execute("hatch", &json!({ "points": [[2, 2]] })).unwrap();
}

#[test]
fn a_boundary_set_belongs_to_its_drawing() {
    let mut s = Session::new();
    let outer = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    s.execute("hatchboundaryset", &json!({ "handles": [outer] })).unwrap();
    // Another drawing: its own objects bound its hatches. Handles are numbered per drawing, so
    // its first square may share the set's handle: hatch its second one.
    s.new_drawing(true);
    rect(&mut s, [-50.0, -50.0], [-40.0, -40.0]);
    let second = rect(&mut s, [100.0, 100.0], [120.0, 120.0]);
    assert_ne!(second, outer);
    s.execute("hatch", &json!({ "points": [[110, 110]] })).unwrap();
}

#[test]
fn a_pick_outside_the_boundary_set_says_so() {
    let mut s = Session::new();
    let a = rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    rect(&mut s, [50.0, 0.0], [70.0, 20.0]);
    s.execute("hatchboundaryset", &json!({ "handles": [a] })).unwrap();
    let e = s.execute("hatch", &json!({ "points": [[60, 10]] })).unwrap_err().to_string();
    assert!(e.contains("boundary set"), "{e}");
}
