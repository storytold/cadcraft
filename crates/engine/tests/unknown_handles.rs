//! A `handle`/`handles` param that names no object in the drawing (never made, or erased) is an
//! error that lists those handles, and the command changes nothing. Before, unknown handles were
//! dropped without a word (`move {handles: ["FFFFF"]}` answered `{"moved": 0}`), so a script or
//! an agent never learned it had missed.

use cadcraft_engine::Session;
use serde_json::{Value, json};

fn circles(s: &mut Session) -> Vec<Value> {
    s.execute("entities", &json!({ "type": "Circle" })).unwrap()["entities"].as_array().cloned().unwrap_or_default()
}

fn circle(s: &mut Session, x: f64) -> String {
    s.execute("circle", &json!({ "center": [x, 0], "radius": 1 })).unwrap()["handle"].as_str().unwrap().to_string()
}

#[test]
fn unknown_handles_are_an_error_and_change_nothing() {
    let mut s = Session::new();
    let a = circle(&mut s, 0.0);
    let b = circle(&mut s, 10.0);
    let gone = circle(&mut s, 20.0);
    assert_eq!(s.execute("erase", &json!({ "handle": gone })).unwrap()["erased"], 1);
    s.execute("qselect", &json!({ "type": "Circle" })).unwrap();
    assert_eq!(s.selection().len(), 2);

    for (cmd, params, named) in [
        ("move", json!({ "handles": ["FFFFF"], "delta": [5, 5] }), vec!["FFFFF"]),
        ("move", json!({ "handles": [a, "FFFFF", b, "1A2BC", "FFFFF"], "delta": [5, 5] }), vec!["FFFFF", "1A2BC"]),
        ("move", json!({ "handles": [a, gone], "delta": [5, 5] }), vec![gone.as_str()]),
        ("move", json!({ "handle": "FFFFF", "delta": [5, 5] }), vec!["FFFFF"]),
        ("erase", json!({ "handles": [a, b, 1_048_575] }), vec!["FFFFF"]),
        ("copy", json!({ "handles": [a, "FFFFF"], "delta": [0, 5] }), vec!["FFFFF"]),
        ("properties.set", json!({ "handles": [a, "FFFFF"], "color": 1 }), vec!["FFFFF"]),
        ("select", json!({ "handles": [a, "FFFFF"] }), vec!["FFFFF"]),
        ("datalinkupdate", json!({ "handles": ["FFFFF"] }), vec!["FFFFF"]),
    ] {
        let e = s.execute(cmd, &params).expect_err(&format!("{cmd} {params}")).to_string();
        assert!(e.contains("unknown handle"), "{cmd} {params}: {e}");
        for h in &named {
            assert!(e.contains(h), "{cmd} {params}: {e} must name {h}");
        }
        assert_eq!(e.matches("FFFFF").count(), usize::from(named.contains(&"FFFFF")), "{cmd} {params}: each handle once: {e}");
        let c = circles(&mut s);
        assert_eq!(c.len(), 2, "{cmd} {params} must change nothing");
        assert!(c.iter().all(|e| e["geometry"]["center"]["y"] == json!(0.0)), "{cmd} {params} must move nothing");
        assert!(c.iter().all(|e| e["color"] != json!("red")), "{cmd} {params} must recolour nothing");
        assert_eq!(s.selection().len(), 2, "{cmd} {params} must keep the selection");
    }

    // Known handles still act on exactly those objects; an explicit empty list is no objects, and
    // without `handle`/`handles` the selection is used, as before.
    assert_eq!(s.execute("move", &json!({ "handles": [a], "delta": [0, 5] })).unwrap()["moved"], 1);
    assert_eq!(s.execute("move", &json!({ "handles": [], "delta": [0, 5] })).unwrap()["moved"], 0);
    assert_eq!(s.execute("move", &json!({ "delta": [0, 1] })).unwrap()["moved"], 2);
}

#[test]
fn an_object_erased_while_selected_drops_out_of_the_selection() {
    let mut s = Session::new();
    let a = circle(&mut s, 0.0);
    let b = circle(&mut s, 10.0);
    s.execute("select", &json!({ "handles": [a, b] })).unwrap();
    // JOIN, EXPLODE, a script or an agent can remove a selected object without clearing the
    // selection; the next command on the selection acts on what is left instead of failing.
    s.doc_mut().unwrap().remove_entity(cadcraft_doc::Handle::parse_hex(&b).unwrap());
    assert_eq!(s.selection().len(), 1);
    assert_eq!(s.execute("move", &json!({ "delta": [0, 5] })).unwrap()["moved"], 1);
}

#[test]
fn handles_are_found_in_every_space() {
    // An object on a layout is addressed from the Model tab (and the other way round): the check
    // looks in model space and every layout, not only the space on screen.
    let mut s = Session::new();
    s.new_drawing(true);
    let m = circle(&mut s, 0.0);
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    let p = circle(&mut s, 5.0);
    assert_eq!(s.execute("move", &json!({ "handles": [m], "delta": [0, 1] })).unwrap()["moved"], 1);
    s.execute("layout.set", &json!({ "name": "Model" })).unwrap();
    assert_eq!(s.execute("move", &json!({ "handles": [p], "delta": [0, 1] })).unwrap()["moved"], 1);
    assert_eq!(s.execute("move", &json!({ "handles": [m, p], "delta": [0, 1] })).unwrap()["moved"], 2);
}

#[test]
fn handles_are_found_in_the_block_editor() {
    let mut s = Session::new();
    let l = s.execute("line", &json!({ "points": [[10, 10], [12, 10]] })).unwrap()["handles"][0].clone();
    s.execute("block", &json!({ "name": "Door", "base": [10, 10], "handles": [l] })).unwrap();
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    let inside: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    assert_eq!(inside.len(), 1);
    assert_eq!(s.execute("move", &json!({ "handles": inside, "delta": [0, 1] })).unwrap()["moved"], 1);
    let e = s.execute("move", &json!({ "handles": ["FFFFF"], "delta": [0, 1] })).expect_err("unknown in the editor");
    assert!(e.to_string().contains("FFFFF"), "{e}");
}

#[test]
fn previous_leaves_out_erased_objects() {
    // AutoCAD: an object erased since the last selection silently drops out of Previous; only
    // explicit JSON handles are strict.
    let mut s = Session::new();
    let a = circle(&mut s, 0.0);
    let b = circle(&mut s, 10.0);
    let h = |x: &str| cadcraft_doc::Handle::parse_hex(x).unwrap();
    s.cmdline("move").unwrap();
    s.input(cadcraft_engine::Input::Pick(vec![h(&a), h(&b)])).unwrap();
    for t in ["", "0,0", "0,1"] {
        s.cmdline(t).unwrap();
    }
    assert!(s.running.is_none());
    s.execute("erase", &json!({ "handles": [a] })).unwrap();

    // Previous now holds one erased and one live object: MOVE acts on the live one.
    s.cmdline("move").unwrap();
    for t in ["p", "", "0,0", "0,1"] {
        s.cmdline(t).unwrap();
    }
    assert!(s.running.is_none());
    let c = circles(&mut s);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["geometry"]["center"]["y"], json!(2.0));

    // Everything in Previous erased: "0 found", and Enter ends the command without an error.
    s.execute("erase", &json!({ "handles": [b] })).unwrap();
    s.cmdline("move").unwrap();
    s.cmdline("p").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
}
