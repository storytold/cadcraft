//! Lookup parameters and tables in the Block Editor (BPARAMETER Lookup, BACTION Lookup,
//! BLOOKUPTABLE): a table authored here sets a reference's distances, positions and visibility
//! state from the lookup value picked, and shows the lookup value its inputs match.

use cadcraft_engine::doc::dynblock::{ActionKind, ParameterKind, custom_properties, dynamic_block, parameter_objects};
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn h(v: &Value) -> Handle {
    cadcraft_engine::cmd::curves::handle_value(v).unwrap()
}

/// Block "Box": a 10 × 5 rectangle from (5,5) and a circle on its right edge, base point (5,5),
/// and a reference at (100,0). Returns the reference.
fn boxed(s: &mut Session) -> Value {
    let made = |v: Value| if v["handle"].is_null() { v["handles"][0].clone() } else { v["handle"].clone() };
    let r = made(s.execute("rectang", &json!({ "p1": [5, 5], "p2": [15, 10] })).unwrap());
    let c = made(s.execute("circle", &json!({ "center": [15, 7.5], "radius": 1 })).unwrap());
    s.execute("block", &json!({ "name": "Box", "base": [5, 5], "handles": [r, c], "keep": "delete" })).unwrap();
    s.execute("insert", &json!({ "name": "Box", "at": [100, 0] })).unwrap()["handle"].clone()
}

/// In the Block Editor on "Box" (editor coordinates: the rectangle from (0,0) to (10,5), the
/// circle at (10,2.5)): Distance1 stretching the right edge, Position1 moving the circle, states
/// VisibilityState0 (both) and Small (the rectangle only), and lookup parameter Size with its
/// table. Returns the lookup action's graph id.
fn authored(s: &mut Session) -> i32 {
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    let d = s.doc().unwrap();
    let of = |rect: bool| d.model.iter().find(|e| matches!(e.kind, EntityKind::LwPolyline(_)) == rect).unwrap().handle.hex();
    let (r, c) = (of(true), of(false));
    s.execute("bparameter", &json!({ "type": "linear", "start": [0, 0], "end": [10, 0], "grips": 1 })).unwrap();
    s.execute("baction", &json!({ "parameter": "Distance1", "type": "stretch", "frame": [[9, -1], [11, 6]], "handles": [r] })).unwrap();
    s.execute("bparameter", &json!({ "type": "point", "at": [10, 2.5] })).unwrap();
    s.execute("baction", &json!({ "parameter": "Position1", "type": "move", "handles": [c] })).unwrap();
    s.execute("bparameter", &json!({ "type": "visibility", "at": [-2, 5] })).unwrap();
    s.execute("bvstate", &json!({ "new": "Small", "visibility": "show" })).unwrap();
    s.execute("bvhide", &json!({ "handles": [c] })).unwrap();
    let p = s.execute("bparameter", &json!({ "type": "lookup", "at": [-2, 7], "label": "Size" })).unwrap();
    assert_eq!((p["element"].as_str(), p["label"].as_str()), (Some("Lookup"), Some("Size")));
    let a = s.execute("baction", &json!({ "parameter": "Size", "at": [-2, 8] })).unwrap();
    assert_eq!(a["name"], "Lookup1");
    a["node"].as_i64().unwrap() as i32
}

/// Reference `i`'s rectangle corners, circle centre and which of them show.
fn shape(s: &Session, i: &Value) -> (Vec<(f64, f64)>, (f64, f64), Vec<bool>) {
    let d = s.doc().unwrap();
    let EntityKind::Insert(ins) = &d.entity(h(i)).unwrap().kind else { panic!() };
    let b = d.block(&ins.block).unwrap();
    let rect = b
        .entities
        .iter()
        .find_map(|e| if let EntityKind::LwPolyline(p) = &e.kind { Some(p.vertices.iter().map(|v| (v.p.x, v.p.y)).collect()) } else { None });
    let circle = b.entities.iter().find_map(|e| if let EntityKind::Circle(c) = &e.kind { Some((c.center.x, c.center.y)) } else { None });
    (rect.unwrap(), circle.unwrap(), b.entities.iter().map(|e| e.common.visible).collect())
}

fn custom(s: &Session, i: &Value) -> Vec<(String, String, bool)> {
    custom_properties(s.doc().unwrap(), h(i)).unwrap().1.into_iter().map(|p| (p.name, p.value, p.editable)).collect()
}

#[test]
fn a_lookup_table_authored_in_json_drives_references() {
    let mut s = Session::new();
    let i = boxed(&mut s);
    let act = authored(&mut s);
    // Empty, as BACTION makes it.
    let t = s.execute("blookuptable", &json!({})).unwrap();
    assert_eq!(t["name"], "Lookup1");
    assert_eq!(
        t["columns"],
        json!([{ "name": "Size", "parameter": t["columns"][0]["parameter"], "property": "lookupString", "lookup": true, "unmatched": "Custom", "reverse": false }])
    );
    let undo = s.state().unwrap().undo.len();
    let t = s
        .execute(
            "blookuptable",
            &json!({
                "inputs": ["Distance1", "Position1 X", "Visibility1"],
                "rows": [[10, 10, "VisibilityState0", "Big"], ["6", "6", "small", "Small"]],
                "reverse": true,
            }),
        )
        .unwrap();
    assert_eq!(s.state().unwrap().undo.len(), undo + 1, "one undo step");
    assert_eq!(t["action"], act);
    let names: Vec<&str> = t["columns"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Distance1", "Position1 X", "Visibility1", "Size"]);
    assert_eq!(t["rows"], json!([["10", "10", "VisibilityState0", "Big"], ["6", "6", "Small", "Small"]]));
    assert_eq!(t["columns"][3]["reverse"], true);
    s.execute("bclose", &json!({})).unwrap();

    // The block's own values match the first row.
    assert!(custom(&s, &i).contains(&("Size".to_string(), "Big".to_string(), true)), "{:?}", custom(&s, &i));
    // Size → Small sets the row's distance, position and state.
    s.execute("blocks.reference.set", &json!({ "handle": i, "name": "Size", "value": "Small" })).unwrap();
    let (rect, circle, shown) = shape(&s, &i);
    assert_eq!(rect, [(5.0, 5.0), (11.0, 5.0), (11.0, 10.0), (5.0, 10.0)]);
    assert_eq!(circle, (11.0, 7.5));
    assert_eq!(shown, [true, false], "Small hides the circle");
    // Inputs matching no row show Custom; matching one again, its value.
    s.execute("blocks.reference.set", &json!({ "handle": i, "name": "Distance1", "value": "8" })).unwrap();
    assert!(custom(&s, &i).contains(&("Size".to_string(), "Custom".to_string(), true)));
    s.execute("blocks.reference.set", &json!({ "handle": i, "name": "Distance1", "value": "6" })).unwrap();
    assert!(custom(&s, &i).contains(&("Size".to_string(), "Small".to_string(), true)));
}

#[test]
fn a_read_only_lookup_cannot_be_set() {
    let mut s = Session::new();
    let i = boxed(&mut s);
    authored(&mut s);
    s.execute("blookuptable", &json!({ "inputs": ["Distance1"], "rows": [[10, "Big"], [6, "Small"]] })).unwrap();
    s.execute("bclose", &json!({})).unwrap();
    assert!(custom(&s, &i).contains(&("Size".to_string(), "Big".to_string(), false)), "read only unless reverse lookup is allowed");
    assert!(s.execute("blocks.reference.set", &json!({ "handle": i, "name": "Distance1", "value": "6" })).is_ok());
    assert!(custom(&s, &i).contains(&("Size".to_string(), "Small".to_string(), false)));
}

#[test]
fn lookup_prompts_ask_as_autocad_does() {
    let mut s = Session::new();
    boxed(&mut s);
    s.execute("bedit", &json!({ "name": "Box" })).unwrap();
    s.execute("bparameter", &json!({ "type": "lookup", "at": [-2, 9] })).unwrap();
    s.script("BPARAMETER K\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Specify parameter location");
    s.script("N\n").unwrap();
    let p = s.current_prompt().unwrap();
    assert_eq!((p.message.as_str(), p.default.as_deref()), ("Enter parameter name", Some("Lookup1")));
    s.script("Look2\nL\n").unwrap();
    let p = s.current_prompt().unwrap();
    assert_eq!((p.message.as_str(), p.default.as_deref()), ("Enter lookup property label", Some("Lookup2")));
    s.script("Size\nD\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Enter property description");
    s.script("desc\nP\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Display property in Properties palette");
    s.script("N\n-2,7\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Enter number of grips");
    s.script("0\n").unwrap();
    assert!(s.running.is_none());
    let m = dynamic_block(s.doc().unwrap(), "Box").unwrap();
    let p = m.parameters.iter().find(|p| p.element == "Look2").unwrap();
    assert!(matches!(&p.kind, ParameterKind::Lookup { label, description, .. } if label == "Size" && description == "desc"));
    assert!(!p.show_properties && p.grips.iter().all(|g| *g == 0));

    // BACTION on it asks only where the action goes; a second one is refused.
    s.script("BACTION -2,7\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Specify action location");
    s.script("-2,8\n").unwrap();
    assert!(s.running.is_none());
    s.script("BACTION -2,7\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Select parameter", "already has its table");
    s.input(Input::Cancel).unwrap();
    let m = dynamic_block(s.doc().unwrap(), "Box").unwrap();
    assert_eq!(m.actions.iter().filter(|a| matches!(a.kind, ActionKind::Lookup { .. })).count(), 1);

    // BLOOKUPTABLE on the command line: pick the action; its table is listed.
    s.script("BLOOKUPTABLE\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Select lookup action");
    s.script("-2,8\n").unwrap();
    assert!(s.running.is_none());
}

#[test]
fn the_lookup_action_is_active_in_the_state_shown() {
    let mut s = Session::new();
    boxed(&mut s);
    let act = authored(&mut s);
    let m = dynamic_block(s.doc().unwrap(), "Box").unwrap();
    let action = m.actions.iter().find(|a| a.node == act).unwrap().handle;
    let Some(ParameterKind::Visibility { states, .. }) =
        m.parameters.iter().map(|p| p.kind.clone()).find(|k| matches!(k, ParameterKind::Visibility { .. }))
    else {
        panic!()
    };
    let small = states.iter().find(|st| st.name == "Small").unwrap();
    assert!(small.active.contains(&action), "added while Small is shown");
    assert!(!states[0].active.contains(&action));
    // A visibility state made to show everything lists the actions too, as AutoCAD's does.
    s.execute("bvstate", &json!({ "new": "All", "visibility": "show" })).unwrap();
    let m = dynamic_block(s.doc().unwrap(), "Box").unwrap();
    let all = parameter_objects(&m, None);
    let Some(ParameterKind::Visibility { states, .. }) =
        m.parameters.iter().map(|p| p.kind.clone()).find(|k| matches!(k, ParameterKind::Visibility { .. }))
    else {
        panic!()
    };
    assert_eq!(states.last().unwrap().active, all);
    assert!(all.contains(&action));
}

#[test]
fn erasing_the_lookup_parameter_erases_its_table() {
    let mut s = Session::new();
    boxed(&mut s);
    authored(&mut s);
    s.execute("blookuptable", &json!({ "inputs": ["Distance1", "Visibility1"], "rows": [[10, "VisibilityState0", "Big"], [6, "Small", "Small"]] }))
        .unwrap();
    // Erasing an input takes its column.
    let lin = dynamic_block(s.doc().unwrap(), "Box").unwrap().parameters.iter().find(|p| p.kind.label() == Some("Distance1")).unwrap().node;
    s.execute("blocks.element.erase", &json!({ "nodes": [lin] })).unwrap();
    let t = s.execute("blookuptable", &json!({})).unwrap();
    assert_eq!(t["rows"], json!([["VisibilityState0", "Big"], ["Small", "Small"]]));
    let size = dynamic_block(s.doc().unwrap(), "Box").unwrap().parameters.iter().find(|p| p.kind.label() == Some("Size")).unwrap().node;
    s.execute("blocks.element.erase", &json!({ "nodes": [size] })).unwrap();
    assert!(s.execute("blookuptable", &json!({})).is_err(), "no lookup action left");
}

#[test]
fn hostile_lookup_params_never_panic() {
    let mut s = Session::new();
    boxed(&mut s);
    assert!(s.execute("blookuptable", &json!({})).is_err(), "only in the Block Editor");
    authored(&mut s);
    let many: Vec<Value> = (0..20_001).map(|_| json!(["1", "a"])).collect();
    for p in [
        json!(null),
        json!([]),
        json!({ "action": "Nope" }),
        json!({ "action": 9999 }),
        json!({ "action": [1] }),
        json!({ "inputs": "Distance1" }),
        json!({ "inputs": [5] }),
        json!({ "inputs": ["Nope"], "rows": [] }),
        json!({ "inputs": ["Distance1"], "rows": 5 }),
        json!({ "inputs": ["Distance1"], "rows": [5] }),
        json!({ "inputs": ["Distance1"], "rows": [[{}, "a"]] }),
        json!({ "inputs": ["Distance1"], "rows": [["1"]] }),
        json!({ "inputs": ["Distance1"], "rows": [["NaN", "a"]] }),
        json!({ "inputs": ["Distance1"], "rows": [["1e999", "a"]] }),
        json!({ "inputs": ["Distance1"], "rows": many }),
        json!({ "inputs": ["Distance1", "Distance1"], "rows": [] }),
        json!({ "inputs": ["Distance1"], "lookups": ["Nope"], "rows": [] }),
        json!({ "inputs": ["Distance1"], "lookups": [], "rows": [] }),
        json!({ "unmatched": 5 }),
        json!({ "unmatched": { "Nope": "x" } }),
        json!({ "reverse": "yes" }),
        json!({ "inputs": ["Distance1"] }),
    ] {
        let _ = s.execute("blookuptable", &p);
    }
    let before = s.doc().unwrap().foreign.clone();
    assert!(s.execute("blookuptable", &json!({ "inputs": ["Distance1"], "rows": [["1", "a"], ["1.0", "b"]] })).is_err());
    assert_eq!(s.doc().unwrap().foreign, before, "a refused table changes nothing");
    for p in [
        json!({ "type": "lookup" }),
        json!({ "type": "lookup", "at": [0, 0], "grips": 2 }),
        json!({ "type": "lookup", "at": [0, 0], "palette": "no" }),
        json!({ "type": "lookup", "at": [0, 0], "name": "Lookup" }),
    ] {
        assert!(s.execute("bparameter", &p).is_err(), "{p}");
    }
    let _ = s.execute("bparameter", &json!({ "type": "lookup", "at": [1e308, f64::MAX] }));
    for p in [
        json!({ "parameter": "Size" }),
        json!({ "parameter": "Size", "type": "move", "handles": [] }),
        json!({ "parameter": "Distance1", "type": "lookup" }),
    ] {
        assert!(s.execute("baction", &p).is_err(), "{p}");
    }
    s.script("BLOOKUPTABLE\n1000,1000\n").unwrap();
    s.input(Input::Cancel).unwrap();
    s.script("BPARAMETER K N\n\nP\nmaybe\n").unwrap();
    s.input(Input::Cancel).unwrap();
}

#[test]
fn a_lookup_on_a_position_follows_its_point() {
    let mut s = Session::new();
    let i = boxed(&mut s);
    authored(&mut s);
    s.execute("blookuptable", &json!({ "inputs": ["Position1 X"], "rows": [[10, "Big"], [6, "Small"]], "reverse": true })).unwrap();
    s.execute("bclose", &json!({})).unwrap();
    assert!(custom(&s, &i).contains(&("Size".to_string(), "Big".to_string(), true)));
    let m = dynamic_block(s.doc().unwrap(), "Box").unwrap();
    let size = m.parameters.iter().find(|p| matches!(p.kind, ParameterKind::Lookup { .. })).unwrap().node;
    // The lookup value the reference keeps (written to its data, as AutoCAD writes it).
    let kept = |s: &Session| match cadcraft_engine::doc::dynblock::reference(s.doc().unwrap(), h(&i)).unwrap().values.get(&size) {
        Some(cadcraft_engine::doc::dynblock::Value::Text { value }) => value.clone(),
        v => format!("{v:?}"),
    };
    // Typed in Properties.
    s.execute("blocks.reference.set", &json!({ "handle": i, "name": "Position1 X", "value": "6" })).unwrap();
    assert!(custom(&s, &i).contains(&("Size".to_string(), "Small".to_string(), true)), "{:?}", custom(&s, &i));
    assert_eq!(kept(&s), "Small");
    // Dragged by its grip (the reference is at (100,0), so the block's x = 10 is at 110).
    let node = m.parameters.iter().find(|p| matches!(p.kind, ParameterKind::Point { .. })).unwrap().node;
    cadcraft_engine::doc::dynblock::drag_grip(
        s.doc_mut().unwrap(),
        h(&i),
        node,
        cadcraft_engine::doc::dynblock::GripPart::At,
        cadcraft_engine::geom::Vec2::new(110.0, 2.5),
    )
    .unwrap();
    assert!(custom(&s, &i).contains(&("Size".to_string(), "Big".to_string(), true)), "{:?}", custom(&s, &i));
    assert_eq!(kept(&s), "Big");
}
