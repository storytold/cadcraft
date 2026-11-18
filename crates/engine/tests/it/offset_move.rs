//! OFFSET Erase/Layer/Multiple/Undo and the MOVE/COPY Displacement option at the command line.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

/// Every line as ((ax, ay), (bx, by), layer), sorted by the first y.
fn lines(s: &Session) -> Vec<((f64, f64), (f64, f64), String)> {
    let mut v: Vec<_> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some(((l.a.x, l.a.y), (l.b.x, l.b.y), e.common.layer.clone())) } else { None })
        .collect();
    v.sort_by(|a, b| a.0.1.total_cmp(&b.0.1).then(a.0.0.total_cmp(&b.0.0)));
    v
}

fn one_line() -> Session {
    let mut s = Session::new();
    s.cmdline("LINE 0,0 10,0").unwrap();
    s.cmdline("").unwrap();
    s
}

fn l(a: (f64, f64), b: (f64, f64), layer: &str) -> ((f64, f64), (f64, f64), String) {
    (a, b, layer.to_string())
}

#[test]
fn offset_erase_source() {
    let mut s = one_line();
    s.script("OFFSET E Y 2 5,0 5,1\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), vec![l((0.0, 2.0), (10.0, 2.0), "0")]);
    // The choice is remembered: the next OFFSET erases the source too, until No.
    s.script("OFFSET 2 5,2 5,3\n\n").unwrap();
    assert_eq!(lines(&s), vec![l((0.0, 4.0), (10.0, 4.0), "0")]);
    s.script("OFFSET E N 2 5,4 5,5\n\n").unwrap();
    assert_eq!(lines(&s).len(), 2);
}

#[test]
fn offset_layer_current() {
    let mut s = one_line();
    s.execute("layer.new", &json!({ "name": "Other", "current": true })).unwrap();
    s.script("OFFSET L C 2 5,0 5,1\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), vec![l((0.0, 0.0), (10.0, 0.0), "0"), l((0.0, 2.0), (10.0, 2.0), "Other")]);
    // Source (the default) keeps the source object's layer.
    s.script("OFFSET L S 2 5,0 5,-1\n\n").unwrap();
    assert_eq!(lines(&s).first(), Some(&l((0.0, -2.0), (10.0, -2.0), "0")));
}

#[test]
fn offset_multiple_and_undo() {
    let mut s = one_line();
    // Multiple: each side point offsets the newest copy again; Undo takes the last one back.
    s.script("OFFSET 2 5,0 M 5,1 5,3 5,5 U\n\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), vec![l((0.0, 0.0), (10.0, 0.0), "0"), l((0.0, 2.0), (10.0, 2.0), "0"), l((0.0, 4.0), (10.0, 4.0), "0")]);
}

#[test]
fn offset_json_erase_and_layer() {
    let mut s = one_line();
    s.execute("layer.new", &json!({ "name": "Other", "current": true })).unwrap();
    let h = s.doc().unwrap().model.iter().next().unwrap().handle.hex();
    s.execute("offset", &json!({ "handle": h, "distance": 2, "side": [5, 1], "erase": true, "layer": "current" })).unwrap();
    assert_eq!(lines(&s), vec![l((0.0, 2.0), (10.0, 2.0), "Other")]);
}

#[test]
fn move_displacement() {
    // The Displacement keyword asks for the displacement directly.
    let mut s = one_line();
    s.script("MOVE 5,0  D 3,4\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), vec![l((3.0, 4.0), (13.0, 4.0), "0")]);
    // Enter at the base point prompt does the same.
    let mut s = one_line();
    s.script("MOVE 5,0\n\n\n3,4\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), vec![l((3.0, 4.0), (13.0, 4.0), "0")]);
    // Base and second point still work.
    let mut s = one_line();
    s.script("MOVE 5,0  1,1 2,3\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), vec![l((1.0, 2.0), (11.0, 2.0), "0")]);
}

#[test]
fn copy_displacement() {
    let mut s = one_line();
    s.script("COPY 5,0  D 0,3\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), vec![l((0.0, 0.0), (10.0, 0.0), "0"), l((0.0, 3.0), (10.0, 3.0), "0")]);
}
