//! Authoring dynamic blocks in the Block Editor (BPARAMETER, BACTION): parameters and actions
//! added to a block make its references dynamic, here and in files saved for AutoCAD.

use cadcraft_engine::doc::dynblock::{ActionKind, Mark, ParameterKind, dynamic_block};
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
    for t in ["polar", "xy", "rotation", "flip", "visibility", "lookup", "base", "alignment"] {
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
