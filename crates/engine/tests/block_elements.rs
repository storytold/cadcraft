//! Picking, moving and erasing parameters and actions in the Block Editor, as AutoCAD's editor
//! does (recorded in AutoCAD: erasing a parameter erases its actions; moving one moves its
//! points, grips and label; moving an action moves only where it shows).

use cadcraft_engine::doc::dynblock::{ActionKind, ElementPoint, ParameterKind, dynamic_block};
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

/// Block "Box" (a 10 × 5 rectangle at the origin, a circle) in the Block Editor with a linear
/// parameter (node 1) under it, a stretch of the rectangle's right edge (node 8) and a point
/// parameter (node 9). Returns the rectangle's handle.
fn authored() -> (Session, Handle) {
    let mut s = Session::new();
    let made = |v: Value| if v["handle"].is_null() { v["handles"][0].clone() } else { v["handle"].clone() };
    let r = made(s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 5] })).unwrap());
    let c = made(s.execute("circle", &json!({ "center": [5, 2.5], "radius": 1 })).unwrap());
    s.execute("block", &json!({ "name": "Box", "base": [0, 0], "handles": [r, c], "keep": "delete" })).unwrap();
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    let rect = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::LwPolyline(_))).unwrap().handle;
    s.execute("bparameter", &json!({ "type": "linear", "start": [0, -1], "end": [10, -1], "labelAt": [5, -3] })).unwrap();
    let a = s
        .execute("baction", &json!({ "parameter": 1, "type": "stretch", "frame": [[8, -2], [12, 6]], "handles": [rect.hex()], "at": [12, -3] }))
        .unwrap();
    assert_eq!(a["node"], 8);
    let p = s.execute("bparameter", &json!({ "type": "point", "at": [2, 2], "labelAt": [3, 3] })).unwrap();
    assert_eq!(p["node"], 9);
    (s, rect)
}

fn names(s: &Session) -> Vec<String> {
    let m = dynamic_block(s.doc().unwrap(), "Box").unwrap();
    m.parameters.iter().map(|p| p.element.clone()).chain(m.actions.iter().map(|a| a.name.clone())).collect()
}

/// The graph's nodes (index, id) as written.
fn graph_nodes(s: &Session) -> Vec<(i32, i32)> {
    let d = s.doc().unwrap();
    let g = d.foreign.objects.iter().find(|o| o.kind == "ACAD_EVALUATION_GRAPH").unwrap();
    let at = g.tags.iter().position(|t| t.value == "AcDbEvalGraph").unwrap();
    let body = &g.tags[at + 3..];
    body.chunks(8).take_while(|c| c[0].code == 91).map(|c| (c[0].value.parse().unwrap(), c[2].value.parse().unwrap())).collect()
}

#[test]
fn deleting_a_picked_parameter_erases_its_actions_as_autocad_does() {
    let (mut s, _) = authored();
    s.execute("blocks.element.select", &json!({ "nodes": [1] })).unwrap();
    assert_eq!(s.picked_elements(), [1]);
    let objects = s.doc().unwrap().foreign.objects.len();
    s.execute("erase.selection", &json!({})).unwrap();
    assert_eq!(names(&s), ["Point"], "the stretch went with its parameter");
    assert_eq!(objects - s.doc().unwrap().foreign.objects.len(), 8, "parameter, 2 grips, 4 components, action");
    // As in AutoCAD's file: indices close up, graph ids stay.
    assert_eq!(graph_nodes(&s), [(0, 9), (1, 10), (2, 11), (3, 12)]);
    // The graph's id counter (96, 97) keeps the highest id given out, as AutoCAD's does; with the
    // node count there AutoCAD didn't take the block as dynamic from DXF.
    let d = s.doc().unwrap();
    let g = d.foreign.objects.iter().find(|o| o.kind == "ACAD_EVALUATION_GRAPH").unwrap();
    let counters: Vec<&str> = g.tags.iter().filter(|t| t.code == 96 || t.code == 97).map(|t| t.value.as_str()).collect();
    assert_eq!(counters, ["12", "12"]);
    assert!(s.picked_elements().is_empty());
    // Undo brings them back.
    s.execute("u", &json!({})).unwrap();
    assert_eq!(names(&s), ["Linear", "Point", "Stretch"]);
    // An action alone goes alone.
    s.execute("blocks.element.erase", &json!({ "nodes": [8] })).unwrap();
    assert_eq!(names(&s), ["Linear", "Point"]);
    // The block saves and stays dynamic.
    s.execute("bclose", &json!({})).unwrap();
    assert_eq!(dynamic_block(s.doc().unwrap(), "Box").unwrap().parameters.len(), 2);
}

#[test]
fn moving_picked_parameters_moves_their_points_grips_and_labels() {
    let (mut s, _) = authored();
    s.execute("blocks.element.select", &json!({ "nodes": [1, 8] })).unwrap();
    s.execute("move", &json!({ "delta": [0, -2] })).unwrap();
    s.execute("blocks.element.select", &json!({ "nodes": [9] })).unwrap();
    s.script("MOVE\n0,0\n1,1\n").unwrap();
    assert!(s.running.is_none(), "MOVE takes the picked parameter as its selection");
    let m = dynamic_block(s.doc().unwrap(), "Box").unwrap();
    let ParameterKind::Linear { base, end, .. } = &m.parameters[0].kind else { panic!() };
    assert_eq!((*base, *end), (Vec3::new(0.0, -3.0, 0.0), Vec3::new(10.0, -3.0, 0.0)));
    let ParameterKind::Point { at, .. } = &m.parameters[1].kind else { panic!() };
    assert_eq!(*at, Vec3::new(3.0, 3.0, 0.0));
    let a = &m.actions[0];
    assert_eq!(a.at, Vec3::new(12.0, -5.0, 0.0));
    let ActionKind::Stretch { frame, .. } = &a.kind else { panic!() };
    assert_eq!(frame, &[Vec2::new(8.0, -2.0), Vec2::new(12.0, 6.0)], "the frame stays, as in AutoCAD");
    // Grips follow (in the editor's coordinates).
    s.execute("blocks.element.select", &json!({ "nodes": [1, 9] })).unwrap();
    let grips = s.element_grips();
    assert!(grips.contains(&(1, ElementPoint::Base, Vec2::new(0.0, -3.0))), "{grips:?}");
    assert!(grips.contains(&(1, ElementPoint::Label, Vec2::new(5.0, -5.0))), "the label line stays 2 below: {grips:?}");
    assert!(grips.contains(&(9, ElementPoint::Label, Vec2::new(4.0, 4.0))), "{grips:?}");
}

#[test]
fn a_picked_parameters_grip_moves_one_point() {
    let (mut s, _) = authored();
    s.execute("blocks.element.select", &json!({ "nodes": [1] })).unwrap();
    s.start_element_grip(1, ElementPoint::End).unwrap();
    s.input(Input::Point(Vec2::new(14.0, -1.0))).unwrap();
    assert!(s.running.is_none());
    let m = dynamic_block(s.doc().unwrap(), "Box").unwrap();
    let ParameterKind::Linear { end, .. } = &m.parameters[0].kind else { panic!() };
    assert_eq!(*end, Vec3::new(14.0, -1.0, 0.0));
    // The block works with the new length: a reference's Distance1 starts at 14.
    s.execute("bclose", &json!({})).unwrap();
    let i = s.execute("insert", &json!({ "name": "Box", "at": [50, 0] })).unwrap()["handle"].clone();
    let props = s.execute("blocks.reference", &json!({ "handle": i })).unwrap();
    assert_eq!(props["properties"][0]["value"], "14.0000");
    // One undo step per grip edit.
    let mut s = authored().0;
    s.execute("blocks.element.select", &json!({ "nodes": [8] })).unwrap();
    s.execute("blocks.element.point", &json!({ "node": 8, "point": "at", "to": [20, 20] })).unwrap();
    assert_eq!(dynamic_block(s.doc().unwrap(), "Box").unwrap().actions[0].at, Vec3::new(20.0, 20.0, 0.0));
}

#[test]
fn escape_and_erase_work_on_picked_elements() {
    let (mut s, _) = authored();
    s.execute("blocks.element.select", &json!({ "nodes": [9] })).unwrap();
    s.execute("blocks.element.select", &json!({ "nodes": [1], "add": true })).unwrap();
    assert_eq!(s.picked_elements(), [9, 1]);
    s.cancel();
    assert!(s.picked_elements().is_empty(), "Escape unpicks");
    s.execute("blocks.element.select", &json!({ "nodes": [9] })).unwrap();
    s.script("ERASE\n").unwrap();
    assert!(s.running.is_none(), "ERASE takes the picked parameter");
    assert_eq!(names(&s), ["Linear", "Stretch"]);
}

#[test]
fn hostile_element_params_never_panic() {
    let (mut s, _) = authored();
    let many: Vec<i64> = (0..20_001).collect();
    for (cmd, p) in [
        ("blocks.element.select", json!({ "nodes": [99] })),
        ("blocks.element.select", json!({ "nodes": "1" })),
        ("blocks.element.select", json!({ "nodes": many })),
        ("blocks.element.move", json!({ "nodes": [1] })),
        ("blocks.element.move", json!({ "nodes": [1], "delta": "far" })),
        ("blocks.element.move", json!({ "nodes": [99], "delta": [1, 0] })),
        ("blocks.element.point", json!({ "node": 1, "point": "end", "to": [0, -1] })),
        ("blocks.element.point", json!({ "node": 1, "point": "middle", "to": [0, 0] })),
        ("blocks.element.point", json!({ "node": 8, "point": "base", "to": [0, 0] })),
        ("blocks.element.point", json!({ "node": 2, "point": "at", "to": [0, 0] })),
        ("blocks.element.erase", json!({ "nodes": [2] })),
        ("blocks.element.erase", json!({ "nodes": [99] })),
    ] {
        assert!(s.execute(cmd, &p).is_err(), "{cmd} {p}");
    }
    assert_eq!(names(&s), ["Linear", "Point", "Stretch"], "nothing changed");
    assert_eq!(s.execute("blocks.element.points", &json!({ "node": -5 })).unwrap(), json!({ "points": [] }));
    assert!(s.start_element_grip(42, ElementPoint::At).is_err());
    // Outside the editor.
    s.execute("bclose", &json!({ "save": false })).unwrap();
    assert!(s.execute("blocks.element.select", &json!({ "nodes": [1] })).is_err());
}

/// A picked parameter of a kind that can't be moved yet (here a polar parameter, as AutoCAD's
/// blocks have) stays put; MOVE still moves the objects and the other picked ones, once.
#[test]
fn move_leaves_a_parameter_it_cannot_move_and_finishes() {
    let (mut s, rect) = authored();
    {
        let d = s.doc_mut().unwrap();
        let f = std::sync::Arc::make_mut(&mut d.foreign);
        let o = f.objects.iter_mut().find(|o| o.kind == "BLOCKLINEARPARAMETER").unwrap();
        o.kind = "BLOCKPOLARPARAMETER".into();
    }
    s.execute("blocks.element.select", &json!({ "nodes": [1, 9] })).unwrap();
    s.execute("select", &json!({ "handles": [rect.hex()] })).unwrap();
    s.script("MOVE\n0,0\n1,0\n").unwrap();
    assert!(s.running.is_none(), "MOVE finished");
    let m = dynamic_block(s.doc().unwrap(), "Box").unwrap();
    let ParameterKind::Polar { base, .. } = &m.parameters[0].kind else { panic!("{:?}", m.parameters[0].kind) };
    assert_eq!(*base, Vec3::new(0.0, -1.0, 0.0), "the polar parameter stayed");
    let ParameterKind::Point { at, .. } = &m.parameters[1].kind else { panic!() };
    assert_eq!(*at, Vec3::new(3.0, 2.0, 0.0), "the point parameter moved");
    let EntityKind::LwPolyline(pl) = &s.doc().unwrap().entity(rect).unwrap().kind else { panic!() };
    assert_eq!(pl.vertices[0].p, Vec2::new(1.0, 0.0), "the rectangle moved once");
}

/// BVMODE can be set before any drawing is open (saved settings load at startup), like the
/// other session variables.
#[test]
fn bvmode_is_set_without_a_drawing() {
    let mut s = Session::empty();
    cadcraft_engine::sysvars::set(&mut s, "BVMODE", &json!(1)).unwrap();
    assert!(s.settings.bvmode);
    cadcraft_engine::sysvars::set(&mut s, "OSMODE", &json!(3)).unwrap();
}

/// A JSON MOVE or ERASE given `handles` acts on those objects only; picked parameters and
/// actions come along only when the command uses the selection.
#[test]
fn json_handles_leave_picked_elements_alone() {
    let (mut s, rect) = authored();
    s.execute("blocks.element.select", &json!({ "nodes": [9] })).unwrap();
    s.execute("erase", &json!({ "handles": [] })).unwrap();
    assert_eq!(names(&s), ["Linear", "Point", "Stretch"], "an empty list erases nothing");
    s.execute("move", &json!({ "handles": [rect.hex()], "delta": [1, 0] })).unwrap();
    let m = dynamic_block(s.doc().unwrap(), "Box").unwrap();
    let ParameterKind::Point { at, .. } = &m.parameters[1].kind else { panic!() };
    assert_eq!(*at, Vec3::new(2.0, 2.0, 0.0), "the picked point parameter stayed");
    s.execute("move", &json!({ "delta": [1, 0] })).unwrap();
    let m = dynamic_block(s.doc().unwrap(), "Box").unwrap();
    let ParameterKind::Point { at, .. } = &m.parameters[1].kind else { panic!() };
    assert_eq!(*at, Vec3::new(3.0, 2.0, 0.0), "with the selection, it moves");
}

/// With PICKFIRST off a selection made before MOVE doesn't count; picked parameters still do.
#[test]
fn pickfirst_off_moves_picked_elements_without_the_selection() {
    let (mut s, rect) = authored();
    s.settings.pickfirst = false;
    s.execute("select", &json!({ "handles": [rect.hex()] })).unwrap();
    s.execute("blocks.element.select", &json!({ "nodes": [9] })).unwrap();
    s.script("MOVE\n0,0\n1,0\n").unwrap();
    assert!(s.running.is_none());
    let EntityKind::LwPolyline(pl) = &s.doc().unwrap().entity(rect).unwrap().kind else { panic!() };
    assert_eq!(pl.vertices[0].p, Vec2::new(0.0, 0.0), "the selected rectangle stayed");
    let m = dynamic_block(s.doc().unwrap(), "Box").unwrap();
    let ParameterKind::Point { at, .. } = &m.parameters[1].kind else { panic!() };
    assert_eq!(*at, Vec3::new(3.0, 2.0, 0.0));
}
