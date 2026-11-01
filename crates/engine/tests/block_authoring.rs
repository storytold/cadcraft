//! Authoring dynamic blocks in the Block Editor (BPARAMETER, BACTION): parameters and actions
//! added to a block make its references dynamic, here and in files saved for AutoCAD.

use cadcraft_engine::doc::dynblock::{ActionKind, Mark, ParameterKind, dynamic_block, parameter_objects};
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn h(v: &Value) -> Handle {
    cadcraft_engine::cmd::curves::handle_value(v).unwrap()
}

/// Block "Box": a 10 × 5 rectangle from (5,5) and a circle on its right edge, base point (5,5),
/// and a reference at (100,0). Returns (rectangle, circle, reference): the block's objects.
fn boxed(s: &mut Session) -> (Value, Value, Value) {
    let made = |v: Value| if v["handle"].is_null() { v["handles"][0].clone() } else { v["handle"].clone() };
    let r = made(s.execute("rectang", &json!({ "p1": [5, 5], "p2": [15, 10] })).unwrap());
    let c = made(s.execute("circle", &json!({ "center": [15, 7.5], "radius": 1 })).unwrap());
    s.execute("block", &json!({ "name": "Box", "base": [5, 5], "handles": [r, c], "keep": "delete" })).unwrap();
    let i = s.execute("insert", &json!({ "name": "Box", "at": [100, 0] })).unwrap()["handle"].clone();
    let d = s.doc().unwrap();
    let b = d.block("Box").unwrap();
    let of = |rect: bool| json!(b.entities.iter().find(|e| matches!(e.kind, EntityKind::LwPolyline(_)) == rect).unwrap().handle.hex());
    (of(true), of(false), i)
}

/// The rectangle's corners in reference `i`'s block (block coordinates).
fn corners(s: &Session, i: &Value) -> Vec<(f64, f64)> {
    let d = s.doc().unwrap();
    let EntityKind::Insert(ins) = &d.entity(h(i)).unwrap().kind else { panic!() };
    let b = d.block(&ins.block).unwrap();
    b.entities
        .iter()
        .find_map(|e| if let EntityKind::LwPolyline(p) = &e.kind { Some(p.vertices.iter().map(|v| (v.p.x, v.p.y)).collect()) } else { None })
        .unwrap()
}

#[test]
fn a_linear_stretch_authored_in_json_drives_references() {
    let mut s = Session::new();
    let (r, c, i) = boxed(&mut s);
    assert!(s.execute("bparameter", &json!({ "type": "linear", "start": [0, 0], "end": [10, 0] })).is_err(), "only in the Block Editor");
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    let p = s.execute("bparameter", &json!({ "type": "linear", "start": [0, 0], "end": [10, 0], "labelAt": [5, -2] })).unwrap();
    assert_eq!(p, json!({ "node": 1, "element": "Linear", "label": "Distance1" }));
    let a = s
        .execute("baction", &json!({ "parameter": "Distance1", "type": "stretch", "frame": [[8, -1], [12, 6]], "handles": [r, c], "at": [12, -2] }))
        .unwrap();
    assert_eq!(a, json!({ "node": 8, "name": "Stretch" }));

    // The editor draws them where they were given.
    let st = s.state().unwrap();
    let marks = st.block_edit.as_ref().unwrap().marks(&st.doc);
    assert!(
        matches!(&marks[0], Mark::Linear { base, end, offset, label, base_grip: true, end_grip: true, .. }
        if *base == Vec3::ZERO && *end == Vec3::new(10.0, 0.0, 0.0) && *offset == -2.0 && label == "Distance1"),
        "{marks:?}"
    );
    assert!(matches!(&marks[1], Mark::Action { at, name, .. } if *at == Vec3::new(12.0, -2.0, 0.0) && name == "Stretch"), "{marks:?}");

    s.execute("bclose", &json!({})).unwrap();
    // Stored in block coordinates: the base point (5,5) added back.
    let m = dynamic_block(s.doc().unwrap(), "Box").unwrap();
    let ParameterKind::Linear { base, end, .. } = &m.parameters[0].kind else { panic!() };
    assert_eq!((*base, *end), (Vec3::new(5.0, 5.0, 0.0), Vec3::new(15.0, 5.0, 0.0)));
    let ActionKind::Stretch { frame, vertices, .. } = &m.actions[0].kind else { panic!() };
    assert_eq!(frame, &[Vec2::new(13.0, 4.0), Vec2::new(17.0, 11.0)]);
    assert_eq!(vertices, &[(h(&r), vec![1, 2])], "the rectangle's right edge; the circle moves whole");
    assert_eq!(m.actions[0].selection, [h(&r), h(&c)]);

    s.execute("blocks.reference.set", &json!({ "handle": i, "name": "Distance1", "value": "15" })).unwrap();
    assert_eq!(corners(&s, &i), [(5.0, 5.0), (20.0, 5.0), (20.0, 10.0), (5.0, 10.0)]);
}

/// The handles of block `name`'s objects.
fn block_handles(s: &Session, name: &str) -> Vec<Handle> {
    s.doc().unwrap().block(name).unwrap().entities.iter().map(|e| e.handle).collect()
}

/// Saved under another name (BSAVEAS), a dynamic block keeps its parameters and actions in a graph
/// of its own, with objects of its own, and what was unsaved in the editor (a new object and an
/// action on it) is the copy's: the original is as the drawing had it. The editor goes on with the
/// copy, so a later BSAVE keeps its graph and handles; references to either change as before.
#[test]
fn bsaveas_gives_a_dynamic_block_its_own_graph() {
    let mut s = Session::new();
    let (r, c, i) = boxed(&mut s);
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    s.execute("bparameter", &json!({ "type": "linear", "start": [0, 0], "end": [10, 0], "labelAt": [5, -2] })).unwrap();
    s.execute("baction", &json!({ "parameter": "Distance1", "type": "stretch", "frame": [[8, -1], [12, 6]], "handles": [r, c], "at": [12, -2] }))
        .unwrap();
    s.execute("bsave", &json!({})).unwrap();
    // Unsaved: a new circle, moved by the distance.
    let made = |v: Value| if v["handle"].is_null() { v["handles"][0].clone() } else { v["handle"].clone() };
    let n = made(s.execute("circle", &json!({ "center": [2, 2], "radius": 0.5 })).unwrap());
    s.execute("baction", &json!({ "parameter": "Distance1", "type": "move", "handles": [n], "at": [2, -4] })).unwrap();
    s.execute("bsaveas", &json!({ "name": "Box2" })).unwrap();
    let check = |s: &Session| {
        let d = s.doc().unwrap();
        let (a, b) = (dynamic_block(d, "Box").unwrap(), dynamic_block(d, "Box2").expect("the copy has dynamic data"));
        assert_eq!((a.parameters.len(), a.actions.len()), (1, 1), "the original keeps what the drawing had");
        assert_eq!((b.parameters.len(), b.actions.len()), (1, 2), "the copy has what the editor had");
        assert_ne!(a.parameters[0].handle, b.parameters[0].handle, "its own graph objects");
        let (own, copy) = (block_handles(s, "Box"), block_handles(s, "Box2"));
        assert!(own.iter().all(|h| !copy.contains(h)), "no handle in two blocks");
        assert_eq!(a.actions[0].selection, [h(&r), h(&c)], "the original's action is unchanged");
        for act in &a.actions {
            assert!(act.selection.iter().all(|x| own.contains(x)), "the original's graph names only its own objects");
        }
        for act in &b.actions {
            assert!(act.selection.iter().all(|x| copy.contains(x)), "{:?} in {:?}", act.selection, copy);
        }
    };
    check(&s);
    // The editor edits the copy now: saving again keeps its graph and handles. Its history from
    // before the copy is gone, so an undo can't bring back handles the copy doesn't have.
    let _ = s.execute("undo", &json!({}));
    s.execute("bsave", &json!({})).unwrap();
    s.execute("bclose", &json!({})).unwrap();
    check(&s);

    // A reference to the copy stretches; the original's reference does too.
    let j = s.execute("insert", &json!({ "name": "Box2", "at": [200, 0] })).unwrap()["handle"].clone();
    s.execute("blocks.reference.set", &json!({ "handle": j, "name": "Distance1", "value": "15" })).unwrap();
    assert_eq!(corners(&s, &j), [(5.0, 5.0), (20.0, 5.0), (20.0, 10.0), (5.0, 10.0)]);
    s.execute("blocks.reference.set", &json!({ "handle": i, "name": "Distance1", "value": "12" })).unwrap();
    assert_eq!(corners(&s, &i), [(5.0, 5.0), (17.0, 5.0), (17.0, 10.0), (5.0, 10.0)]);
}

/// A dynamic block replaced by BSAVEAS with a block that isn't dynamic keeps none of its old graph.
#[test]
fn bsaveas_over_a_dynamic_block_leaves_none_of_its_old_graph() {
    let mut s = Session::new();
    let (r, c, _) = boxed(&mut s);
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    s.execute("bparameter", &json!({ "type": "linear", "start": [0, 0], "end": [10, 0], "labelAt": [5, -2] })).unwrap();
    s.execute("baction", &json!({ "parameter": "Distance1", "type": "stretch", "frame": [[8, -1], [12, 6]], "handles": [r, c], "at": [12, -2] }))
        .unwrap();
    s.execute("bsave", &json!({})).unwrap();
    s.execute("bclose", &json!({})).unwrap();
    let graph_objects = s.doc().unwrap().foreign.objects.len();
    // A copy, with a graph of its own.
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    s.execute("bsaveas", &json!({ "name": "Box2" })).unwrap();
    s.execute("bclose", &json!({})).unwrap();
    assert!(dynamic_block(s.doc().unwrap(), "Box2").is_some());
    assert!(s.doc().unwrap().foreign.objects.len() > graph_objects, "the copy's graph objects");
    // Plain block "Plain" replaces it.
    let made = |v: Value| if v["handle"].is_null() { v["handles"][0].clone() } else { v["handle"].clone() };
    let ci = made(s.execute("circle", &json!({ "center": [50, 50], "radius": 2 })).unwrap());
    s.execute("block", &json!({ "name": "Plain", "base": [50, 50], "handles": [ci], "keep": "delete" })).unwrap();
    s.execute("bedit", &json!({ "name": "Plain" })).unwrap();
    s.execute("bsaveas", &json!({ "name": "Box2", "replace": true })).unwrap();
    s.execute("bclose", &json!({})).unwrap();
    let d = s.doc().unwrap();
    assert!(dynamic_block(d, "Box2").is_none(), "Box2 is the plain block now");
    assert_eq!(d.foreign.objects.len(), graph_objects, "only Box's graph is left");
    assert!(dynamic_block(d, "Box").is_some(), "Box is untouched");
}

#[test]
fn bparameter_and_baction_ask_as_autocad_does() {
    let mut s = Session::new();
    let (r, _, i) = boxed(&mut s);
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    s.script("BPARAMETER L 0,0 10,0 5,-2\n").unwrap();
    assert_eq!(s.current_prompt().map(|p| p.message), Some("Enter number of grips".to_string()));
    s.script("\n").unwrap();
    assert!(s.running.is_none(), "Enter takes the default two grips");
    s.script("BACTION 10,0\nT\n\n8,-1 12,6\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Select objects");
    s.input(Input::Pick(vec![h(&r)])).unwrap();
    s.script("\n12,-2\n").unwrap();
    assert!(s.running.is_none());
    // A point parameter and a move of the circle, its prompts' own order.
    s.script("BPARAMETER O\n10,2.5 11,3.5 1\n").unwrap();
    s.script("BACTION 10,2.5\nM\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Select objects", "a point parameter asks no parameter point");
    let circle = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Circle(_))).unwrap().handle;
    s.input(Input::Pick(vec![circle])).unwrap();
    s.script("\n12,1\n").unwrap();
    s.script("BCLOSE\n\n").unwrap();

    let m = dynamic_block(s.doc().unwrap(), "Box").unwrap();
    let names: Vec<_> = m.parameters.iter().map(|p| p.kind.label().unwrap_or("-")).chain(m.actions.iter().map(|a| a.name.as_str())).collect();
    assert_eq!(names, ["Distance1", "Position1", "Stretch", "Move"]);
    s.execute("blocks.reference.set", &json!({ "handle": i, "name": "Distance1", "value": "12" })).unwrap();
    assert_eq!(corners(&s, &i), [(5.0, 5.0), (17.0, 5.0), (17.0, 10.0), (5.0, 10.0)]);
}

#[test]
fn types_not_available_yet_say_so() {
    let mut s = Session::new();
    boxed(&mut s);
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    for t in ["polar", "xy", "rotation", "flip", "lookup", "base", "alignment"] {
        let e = s.execute("bparameter", &json!({ "type": t, "start": [0, 0], "end": [1, 0] })).unwrap_err().to_string();
        assert!(e.contains("not available yet"), "{t}: {e}");
    }
    s.execute("bparameter", &json!({ "type": "linear", "start": [0, 0], "end": [10, 0] })).unwrap();
    let e = s.execute("baction", &json!({ "parameter": 1, "type": "scale", "handles": [] })).unwrap_err().to_string();
    assert!(e.contains("handles"), "{e}");
    let any = s.doc().unwrap().model.iter().next().unwrap().handle.hex();
    let e = s.execute("baction", &json!({ "parameter": 1, "type": "scale", "handles": [any] })).unwrap_err().to_string();
    assert!(e.contains("not available yet"), "{e}");
}

#[test]
fn hostile_authoring_params_never_panic() {
    let mut s = Session::new();
    let (r, _, _) = boxed(&mut s);
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    for p in [
        json!(null),
        json!([]),
        json!({ "type": 5 }),
        json!({ "type": "linear" }),
        json!({ "type": "linear", "start": [0, 0], "end": [0, 0] }),
        json!({ "type": "linear", "start": [1e308, 0], "end": [-1e308, 0], "labelAt": [1e308, 1e308] }),
        json!({ "type": "linear", "start": [1e308, 0], "end": [-1e308, 0] }),
        json!({ "type": "linear", "start": [0, 0], "end": [1, 0], "grips": 3 }),
        json!({ "type": "linear", "start": [0, 0], "end": [1, 0], "grips": -1 }),
        json!({ "type": "point", "at": "nan,nan" }),
        json!({ "type": "point", "at": [0, 0], "grips": 2 }),
        json!({ "type": "point", "at": [0, 0], "label": 7 }),
    ] {
        let _ = s.execute("bparameter", &p);
    }
    let far = s.execute("bparameter", &json!({ "type": "linear", "start": [1e308, 0], "end": [-1e308, 0] }));
    assert!(far.is_err(), "an infinite distance between finite points is refused: {far:?}");
    s.execute("bparameter", &json!({ "type": "linear", "start": [0, 0], "end": [10, 0] })).unwrap();
    let many: Vec<Value> = (0..20_001).map(|_| r.clone()).collect();
    let poly: Vec<[f64; 2]> = (0..20_001).map(|i| [i as f64, (i % 2) as f64]).collect();
    for p in [
        json!(null),
        json!({ "parameter": 99, "type": "move", "handles": [r] }),
        json!({ "parameter": "Nope", "type": "move", "handles": [r] }),
        json!({ "parameter": [], "type": "move", "handles": [r] }),
        json!({ "parameter": 1, "type": "move", "handles": ["zz"] }),
        json!({ "parameter": 1, "type": "move", "handles": ["FFFFFF"] }),
        json!({ "parameter": 1, "type": "move", "handles": many }),
        json!({ "parameter": 1, "type": "stretch", "handles": [r] }),
        json!({ "parameter": 1, "type": "stretch", "handles": [r], "frame": [[0, 0]] }),
        json!({ "parameter": 1, "type": "stretch", "handles": [r], "polygon": poly }),
        json!({ "parameter": 1, "type": "stretch", "handles": [r], "frame": [[100, 100], [101, 101]] }),
        json!({ "parameter": 1, "type": "move", "point": "middle", "handles": [r] }),
    ] {
        assert!(s.execute("baction", &p).is_err(), "{p}");
    }
    // Cancelling either command midway changes nothing.
    let before = s.doc().unwrap().foreign.clone();
    s.script("BPARAMETER L 0,0\n").unwrap();
    s.input(Input::Cancel).unwrap();
    s.script("BACTION 5,0\nT\n").unwrap();
    s.input(Input::Cancel).unwrap();
    assert_eq!(s.doc().unwrap().foreign, before);
}

/// The edited block's visibility states: (name, visible objects, active parameter objects).
fn states(s: &Session) -> Vec<(String, Vec<Handle>, Vec<Handle>)> {
    let st = s.state().unwrap();
    let name = st.block_edit.as_ref().map_or("Box".to_string(), |b| b.name.clone());
    let m = dynamic_block(s.doc().unwrap(), &name).unwrap();
    m.parameters
        .into_iter()
        .find_map(|p| match p.kind {
            ParameterKind::Visibility { states, .. } => Some(states.into_iter().map(|s| (s.name, s.visible, s.active)).collect()),
            _ => None,
        })
        .unwrap()
}

fn hidden(s: &Session) -> Vec<Handle> {
    s.doc().unwrap().hidden_objects.iter().copied().collect()
}

/// A parameter's object and its grips' (what a visibility state lists as active).
fn param_objects(s: &Session, node: i32) -> Vec<Handle> {
    parameter_objects(&dynamic_block(s.doc().unwrap(), "Box").unwrap(), Some(node))
}

/// The states AutoCAD made in the same steps (recorded with -BVSTATE, BVHIDE and BVSHOW):
/// Hide all hides objects and parameters; Show all shows them all; Current visibility copies;
/// an object drawn or a parameter added shows in the state the editor shows only.
#[test]
fn visibility_states_follow_autocads_rules() {
    let mut s = Session::new();
    let (r, c, i) = boxed(&mut s);
    let (r, c) = (h(&r), h(&c));
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    assert!(s.execute("bvstate", &json!({})).is_err(), "no visibility parameter yet");
    s.execute("bparameter", &json!({ "type": "linear", "start": [0, -1], "end": [10, -1] })).unwrap();
    let v = s.execute("bparameter", &json!({ "type": "visibility", "at": [0, 6] })).unwrap();
    assert_eq!(v["label"], "Visibility1");
    let linear = param_objects(&s, 1);
    assert_eq!(linear.len(), 3, "the parameter and its two grips");
    assert_eq!(states(&s), [("VisibilityState0".to_string(), vec![r, c], linear.clone())]);

    let out = s.execute("bvstate", &json!({ "new": "Hid", "visibility": "hide" })).unwrap();
    assert_eq!(out, json!({ "current": "Hid", "states": ["VisibilityState0", "Hid"] }));
    assert_eq!(hidden(&s), [r, c], "the editor shows the new state");
    s.execute("bvshow", &json!({ "handles": [c.hex()] })).unwrap();
    assert_eq!(hidden(&s), [r]);
    s.execute("bvstate", &json!({ "new": "Shown", "visibility": "show" })).unwrap();
    s.execute("bvhide", &json!({ "handles": [r.hex()] })).unwrap();
    s.execute("bvstate", &json!({ "new": "Copy" })).unwrap();
    s.execute("bvhide", &json!({ "handles": [c.hex()], "all": true })).unwrap();
    s.execute("bvstate", &json!({ "set": "hid" })).unwrap();
    let p = s.execute("bparameter", &json!({ "type": "point", "at": [2, 2] })).unwrap()["node"].as_i64().unwrap() as i32;
    let point = param_objects(&s, p);
    let line = s.execute("line", &json!({ "points": [[0, 0], [10, 5]] })).unwrap();
    let line = h(&if line["handle"].is_null() { line["handles"][0].clone() } else { line["handle"].clone() });
    s.execute("bsave", &json!({})).unwrap();
    let want = [
        ("VisibilityState0".to_string(), vec![r], linear.clone()),
        ("Hid".to_string(), vec![line], point),
        ("Shown".to_string(), vec![], linear.clone()),
        ("Copy".to_string(), vec![], linear.clone()),
    ];
    assert_eq!(states(&s), want);

    // Rename and delete; the last state stays.
    s.execute("bvstate", &json!({ "rename": "Copy", "to": "Spare" })).unwrap();
    let out = s.execute("bvstate", &json!({ "delete": ["Spare", "Shown"] })).unwrap();
    assert_eq!(out, json!({ "current": "Hid", "states": ["VisibilityState0", "Hid"] }));
    assert!(s.execute("bvstate", &json!({ "delete": ["VisibilityState0", "Hid"] })).is_err());
    assert!(s.execute("bvstate", &json!({ "new": "hid" })).is_err(), "names are case-insensitive");
    s.execute("bclose", &json!({})).unwrap();
    assert!(s.doc().unwrap().hidden_objects.is_empty(), "the editor's hiding stays in the editor");

    // A reference shows each state's objects.
    s.execute("blocks.reference.set", &json!({ "handle": i, "name": "Visibility1", "value": "Hid" })).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Insert(ins) = &d.entity(h(&i)).unwrap().kind else { panic!() };
    let shown: Vec<bool> = d.block(&ins.block).unwrap().entities.iter().map(|e| e.common.visible).collect();
    assert_eq!(shown, [false, false, true], "only the line drawn in Hid");
}

#[test]
fn visibility_commands_ask_as_autocad_does() {
    let mut s = Session::new();
    let (r, c, _) = boxed(&mut s);
    let (r, c) = (h(&r), h(&c));
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    s.script("BPARAMETER V\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Specify parameter location");
    s.script("0,6\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Enter number of grips");
    s.script("\n").unwrap();
    assert!(s.running.is_none());
    s.script("-BVSTATE\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Enter an option");
    s.script("N\nHid\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Enter object visibility for new state");
    s.script("H\n").unwrap();
    assert!(s.running.is_none());
    assert_eq!(hidden(&s), [r, c]);
    s.script("BVSTATE\nS\n?\n").unwrap();
    s.script("VisibilityState0\n").unwrap();
    assert!(s.running.is_none());
    assert!(hidden(&s).is_empty());
    s.script("BVHIDE\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Select objects");
    s.input(Input::Pick(vec![c])).unwrap();
    s.script("\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Hide for current state or all visibility states");
    s.script("A\n").unwrap();
    assert_eq!(hidden(&s), [c]);
    // BVMODE 1 leaves hidden objects on the canvas (dimmed there) so BVSHOW can pick them.
    s.execute("setvar", &json!({ "name": "BVMODE", "value": 1 })).unwrap();
    assert!(hidden(&s).is_empty());
    s.script("BVSHOW\n").unwrap();
    s.input(Input::Pick(vec![c])).unwrap();
    s.script("\n\n").unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.execute("getvar", &json!({ "name": "BVMODE" })).unwrap()["value"], json!(1));
    s.execute("setvar", &json!({ "name": "BVMODE", "value": 0 })).unwrap();
    assert!(hidden(&s).is_empty(), "the circle shows in the current state again");
    s.script("BVHIDE\n").unwrap();
    s.input(Input::Pick(vec![c])).unwrap();
    s.script("\nA\n").unwrap();
    assert_eq!(hidden(&s), [c]);
    s.script("BVSTATE\nD\nHid\n").unwrap();
    assert_eq!(states(&s), [("VisibilityState0".to_string(), vec![r], vec![])]);
}

#[test]
fn hostile_visibility_params_never_panic() {
    let mut s = Session::new();
    let (r, _, _) = boxed(&mut s);
    assert!(s.execute("bvhide", &json!({ "handles": [r] })).is_err(), "only in the Block Editor");
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    s.execute("bparameter", &json!({ "type": "visibility", "at": [0, 6] })).unwrap();
    assert!(s.execute("bparameter", &json!({ "type": "visibility", "at": [0, 6] })).is_err(), "one per block");
    let many: Vec<Value> = (0..20_001).map(|_| r.clone()).collect();
    for (cmd, p) in [
        ("bvstate", json!(null)),
        ("bvstate", json!({ "new": "" })),
        ("bvstate", json!({ "new": 5 })),
        ("bvstate", json!({ "new": "X", "visibility": "sideways" })),
        ("bvstate", json!({ "set": "Nope" })),
        ("bvstate", json!({ "delete": 7 })),
        ("bvstate", json!({ "delete": [] })),
        ("bvstate", json!({ "delete": "Nope" })),
        ("bvstate", json!({ "rename": "VisibilityState0" })),
        ("bvstate", json!({ "rename": "Nope", "to": "X" })),
        ("bvstate", json!({ "ops": 5 })),
        ("bvstate", json!({ "ops": [{ "ops": [] }] })),
        ("bvstate", json!({ "ops": [7] })),
        ("bvstate", json!({ "ops": (0..1_001).map(|_| json!({})).collect::<Vec<_>>() })),
        ("bvstate", json!({ "order": ["Nope"] })),
        ("bvstate", json!({ "order": ["VisibilityState0", "VisibilityState0"] })),
        ("bvstate", json!({ "order": [] })),
        ("bvstate", json!({ "order": 3 })),
        ("bvhide", json!({ "handles": [] })),
        ("bvhide", json!({ "handles": ["zz"] })),
        ("bvhide", json!({ "handles": ["FFFFFF"] })),
        ("bvhide", json!({ "handles": many })),
        ("bvshow", json!({ "handles": [r], "all": "yes" })),
        ("bparameter", json!({ "type": "visibility", "at": [f64::MAX, 0], "grips": 9 })),
    ] {
        let _ = s.execute(cmd, &p);
    }
    for i in 0..50 {
        s.execute("bvstate", &json!({ "new": format!("S{i}"), "visibility": "show" })).unwrap();
    }
    // Reordering keeps every state and the one shown.
    s.execute("bvstate", &json!({ "set": "S3" })).unwrap();
    let mut names: Vec<String> = (0..50).rev().map(|i| format!("S{i}")).collect();
    names.push("VisibilityState0".into());
    let out = s.execute("bvstate", &json!({ "order": names })).unwrap();
    assert_eq!((&out["current"], &out["states"][0], &out["states"][50]), (&json!("S3"), &json!("S49"), &json!("VisibilityState0")));
    s.script("-BVSTATE\nD\n,,,\n").unwrap();
    s.input(Input::Cancel).unwrap();
    assert!(s.running.is_none());
}
