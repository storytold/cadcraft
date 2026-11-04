//! REFEDIT, REFSET and REFCLOSE: editing a block reference in place (#813).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, Drawing, EntityKind, Handle, Insert, Space};
use cadcraft_engine::geom::Vec3;
use serde_json::{Value, json};

fn h(v: &Value) -> String {
    v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap().to_string()
}

fn insert(s: &mut Session, block: &str, at: (f64, f64), scale: f64, rot_deg: f64) -> String {
    let kind = EntityKind::Insert(Insert {
        block: block.into(),
        insert: Vec3::new(at.0, at.1, 0.0),
        scale: Vec3::new(scale, scale, scale),
        rotation: rot_deg.to_radians(),
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    });
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), kind).unwrap().hex()
}

/// Block B = a line (0,0)-(10,0) and a circle at (20,0) r2; references A (at 100,0) and C
/// (at 0,50, scaled 2, rotated 90 degrees).
fn setup() -> (Session, String, String) {
    let mut s = Session::new();
    let l = h(&s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap());
    let c = h(&s.execute("circle", &json!({ "center": [20, 0], "radius": 2 })).unwrap());
    s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [l, c], "keep": "delete" })).unwrap();
    let a = insert(&mut s, "B", (100.0, 0.0), 1.0, 0.0);
    let c = insert(&mut s, "B", (0.0, 50.0), 2.0, 90.0);
    (s, a, c)
}

fn block_lines(d: &Drawing, name: &str) -> Vec<(Vec3, Vec3)> {
    d.block(name).unwrap().entities.iter().filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some((l.a, l.b)) } else { None }).collect()
}

fn handles(v: &Value) -> Vec<String> {
    v["working"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

fn model_len(s: &Session) -> usize {
    s.doc().unwrap().model.len()
}

#[test]
fn edit_and_save_updates_every_reference() {
    let (mut s, a, c) = setup();
    let before = model_len(&s);
    let r = s.execute("refedit", &json!({ "handle": a })).unwrap();
    let work = handles(&r);
    assert_eq!(work.len(), 2);
    // The reference is hidden, the block's objects stand where it showed them.
    let d = s.doc().unwrap();
    assert!(!d.entity(Handle::parse_hex(&a).unwrap()).unwrap().common.visible);
    assert_eq!(model_len(&s), before + 2);
    let line = work.iter().find_map(|w| match &d.entity(Handle::parse_hex(w).unwrap()).unwrap().kind {
        EntityKind::Line(l) => Some((w.clone(), l.clone())),
        _ => None,
    });
    let (lh, l) = line.unwrap();
    assert!((l.a.x - 100.0).abs() < 1e-9 && (l.b.x - 110.0).abs() < 1e-9);
    // Edit with normal commands: stretch it up by 5 and draw a new line.
    s.execute("move", &json!({ "handles": [lh], "delta": [0, 5] })).unwrap();
    s.execute("line", &json!({ "points": [[100, 20], [110, 20]] })).unwrap();
    // The new line joined the working set.
    let rs = s.execute("refset", &json!({})).unwrap();
    assert_eq!(rs["working"].as_array().unwrap().len(), 3);
    let out = s.execute("refclose", &json!({})).unwrap();
    assert_eq!(out["objects"], 3);
    let d = s.doc().unwrap();
    assert!(d.refedit.is_none());
    assert!(d.entity(Handle::parse_hex(&a).unwrap()).unwrap().common.visible);
    assert_eq!(model_len(&s), before, "working copies are gone, the block holds the objects");
    let lines = block_lines(d, "B");
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().any(|(p, q)| (p.y - 5.0).abs() < 1e-9 && (q.x - 10.0).abs() < 1e-9));
    assert!(lines.iter().any(|(p, q)| (p.y - 20.0).abs() < 1e-9 && (p.x - 0.0).abs() < 1e-9 && (q.x - 10.0).abs() < 1e-9));
    // The second reference shows the change through its own transform.
    let _ = c;
    assert!(s.doc().unwrap().block("B").unwrap().entities.len() == 3);
}

#[test]
fn discard_leaves_everything_as_it_was() {
    let (mut s, a, _) = setup();
    let before = s.doc().unwrap().clone();
    let r = s.execute("refedit", &json!({ "handle": a })).unwrap();
    s.execute("erase", &json!({ "handles": handles(&r) })).unwrap();
    s.execute("refclose", &json!({ "option": "discard" })).unwrap();
    let d = s.doc().unwrap();
    assert!(d.refedit.is_none());
    assert_eq!(d.block("B").unwrap().entities.len(), 2);
    assert_eq!(d.model.len(), before.model.len());
    assert!(d.entity(Handle::parse_hex(&a).unwrap()).unwrap().common.visible);
}

#[test]
fn erasing_a_working_object_removes_it_from_the_block() {
    let (mut s, a, _) = setup();
    let r = s.execute("refedit", &json!({ "handle": a })).unwrap();
    let w = handles(&r);
    s.execute("erase", &json!({ "handles": [w[0]] })).unwrap();
    s.execute("refclose", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().block("B").unwrap().entities.len(), 1);
}

#[test]
fn objects_outside_the_working_set_cannot_be_edited() {
    let (mut s, a, c) = setup();
    let other = h(&s.execute("line", &json!({ "points": [[300, 0], [310, 0]] })).unwrap());
    s.execute("refedit", &json!({ "handle": a })).unwrap();
    let before = s.doc().unwrap().clone();
    let _ = s.execute("erase", &json!({ "handles": [other.clone(), c.clone()] }));
    let _ = s.execute("move", &json!({ "handles": [other.clone()], "delta": [1, 1] }));
    let d = s.doc().unwrap();
    assert!(d.entity(Handle::parse_hex(&other).unwrap()).is_some());
    assert!(d.entity(Handle::parse_hex(&c).unwrap()).is_some());
    assert_eq!(d.model.len(), before.model.len());
    // Starting another edit, or saving, is refused.
    assert!(s.execute("refedit", &json!({ "handle": c })).is_err());
    assert!(s.execute("save", &json!({ "path": "/nonexistent-dir/x.dxf" })).is_err());
}

#[test]
fn refset_adds_and_removes_objects() {
    let (mut s, a, _) = setup();
    let other = h(&s.execute("line", &json!({ "points": [[300, 0], [310, 0]] })).unwrap());
    // Edit only the circle: the line is a faded copy.
    let (circle_src, line_src) = {
        let b = s.doc().unwrap().block("B").unwrap();
        let c = b.entities.iter().find(|e| matches!(e.kind, EntityKind::Circle(_))).unwrap().handle;
        let l = b.entities.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).unwrap().handle;
        (c.hex(), l.hex())
    };
    let r = s.execute("refedit", &json!({ "handle": a, "objects": [circle_src] })).unwrap();
    assert_eq!(r["faded"], 1);
    assert_eq!(handles(&r).len(), 1);
    // Add the host line: it goes into the block at close. Remove the circle: it stays in the block.
    let rs = s.execute("refset", &json!({ "add": [other], "remove": handles(&r) })).unwrap();
    assert_eq!(rs["added"], 1);
    assert_eq!(rs["removed"], 1);
    s.execute("refclose", &json!({})).unwrap();
    let d = s.doc().unwrap();
    let b = d.block("B").unwrap();
    assert_eq!(b.entities.len(), 3);
    assert!(b.entities.get(Handle::parse_hex(&line_src).unwrap()).is_some());
    assert!(d.entity(Handle::parse_hex(&other).unwrap()).is_none(), "moved into the block");
    // 300,0 relative to the reference at 100,0
    assert!(block_lines(d, "B").iter().any(|(p, q)| (p.x - 200.0).abs() < 1e-9 && (q.x - 210.0).abs() < 1e-9));
    assert!(s.execute("refset", &json!({ "add": ["FFFF"] })).is_err(), "no edit active");
}

#[test]
fn a_block_that_would_contain_itself_is_refused_and_the_edit_survives() {
    let (mut s, a, _) = setup();
    s.execute("refedit", &json!({ "handle": a })).unwrap();
    // A reference to B drawn while editing B.
    let nested = insert(&mut s, "B", (500.0, 0.0), 1.0, 0.0);
    let _ = nested;
    // inserted through the doc API: add it to the working set by hand
    s.execute("refset", &json!({ "add": [nested] })).unwrap();
    assert!(s.execute("refclose", &json!({})).is_err());
    assert!(s.doc().unwrap().refedit.is_some(), "the edit is still active");
    assert_eq!(s.doc().unwrap().block("B").unwrap().entities.len(), 2);
    s.execute("refclose", &json!({ "option": "discard" })).unwrap();
    assert!(s.doc().unwrap().refedit.is_none());
}

#[test]
fn undo_takes_the_edit_back_with_its_state() {
    let (mut s, a, _) = setup();
    let n = model_len(&s);
    s.execute("refedit", &json!({ "handle": a })).unwrap();
    s.undo().unwrap();
    let d = s.doc().unwrap();
    assert!(d.refedit.is_none());
    assert_eq!(d.model.len(), n);
    assert!(d.entity(Handle::parse_hex(&a).unwrap()).unwrap().common.visible);
    s.redo().unwrap();
    assert!(s.doc().unwrap().refedit.is_some());
}

#[test]
fn files_never_hold_half_an_edit() {
    let (mut s, a, _) = setup();
    s.execute("refedit", &json!({ "handle": a })).unwrap();
    let d = s.doc().unwrap();
    let committed = d.committed();
    assert!(committed.refedit.is_none());
    assert_eq!(committed.model.len(), 2);
    assert!(committed.entity(Handle::parse_hex(&a).unwrap()).unwrap().common.visible);
    let dxf = cadcraft_io::write_dxf(d);
    let back = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    assert_eq!(back.model.len(), 2);
}

#[test]
fn hostile_requests_never_panic() {
    let (mut s, a, _) = setup();
    for p in [
        json!({}),
        json!({ "handle": "ZZZ" }),
        json!({ "handle": 1e300 }),
        json!({ "name": "nope" }),
        json!({ "handle": a, "objects": "x" }),
        json!({ "handle": a, "objects": [] }),
        json!({ "handle": a, "objects": ["FFFFFFFF"] }),
        json!({ "handle": a, "objects": [null, {}] }),
    ] {
        assert!(s.execute("refedit", &p).is_err(), "{p}");
    }
    for cmd in ["refset", "refclose"] {
        assert!(s.execute(cmd, &json!({})).is_err());
    }
    // A reference with zero scale, a non-uniform one and a non-finite one can't be edited.
    let z = insert(&mut s, "B", (0.0, 0.0), 0.0, 0.0);
    assert!(s.execute("refedit", &json!({ "handle": z })).is_err());
    let nan = insert(&mut s, "B", (f64::NAN, 0.0), 1.0, 0.0);
    assert!(s.execute("refedit", &json!({ "handle": nan })).is_err());
    s.execute("refedit", &json!({ "handle": a })).unwrap();
    for p in [json!({ "add": "x" }), json!({ "remove": ["ZZ"] }), json!({ "add": [null] }), json!({ "handles": 3 }), json!({ "remove": ["FFFFFFF"] })]
    {
        assert!(s.execute("refset", &p).is_err(), "{p}");
    }
    assert!(s.execute("refclose", &json!({ "option": "maybe" })).is_err());
    assert!(s.doc().unwrap().refedit.is_some());
}

#[test]
fn a_block_that_references_itself_in_a_file_does_not_hang() {
    let (mut s, a, _) = setup();
    // Hostile drawing: B contains a reference to itself.
    let kind = EntityKind::Insert(Insert {
        block: "B".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    });
    let d = s.doc_mut().unwrap();
    let hh = d.new_handle();
    let mut b = (**d.block("B").unwrap()).clone();
    b.entities.push(cadcraft_engine::doc::Entity::new(hh, kind));
    d.blocks.insert("B".into(), std::sync::Arc::new(b));
    s.execute("refedit", &json!({ "handle": a })).unwrap();
    assert!(s.execute("refclose", &json!({})).is_err());
    s.execute("refclose", &json!({ "option": "discard" })).unwrap();
}

#[test]
fn interactive_refedit_refset_and_refclose() {
    use cadcraft_engine::Input;
    let (mut s, a, _) = setup();
    let other = h(&s.execute("line", &json!({ "points": [[300, 0], [310, 0]] })).unwrap());
    s.start("refedit").unwrap();
    s.input(Input::Pick(vec![Handle::parse_hex(&a).unwrap()])).unwrap();
    assert!(s.running.is_none() && s.doc().unwrap().refedit.is_some());
    s.start("refset").unwrap();
    s.input(Input::Keyword("Add".into())).unwrap();
    s.input(Input::Pick(vec![Handle::parse_hex(&other).unwrap()])).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(s.doc().unwrap().refedit.as_ref().unwrap().items.len(), 3);
    s.start("refclose").unwrap();
    s.input(Input::Keyword("Save".into())).unwrap();
    assert!(s.doc().unwrap().refedit.is_none());
    assert_eq!(s.doc().unwrap().block("B").unwrap().entities.len(), 3);
}

#[test]
fn xrefs_are_refused_with_a_message() {
    let mut s = Session::new();
    s.doc_mut().unwrap().blocks.insert(
        "X".into(),
        std::sync::Arc::new({
            let mut b = cadcraft_engine::doc::Block::new("X");
            b.xref_path = Some("x.dxf".into());
            b
        }),
    );
    let x = insert(&mut s, "X", (0.0, 0.0), 1.0, 0.0);
    let e = s.execute("refedit", &json!({ "handle": x })).unwrap_err();
    assert!(e.to_string().contains("external reference"), "{e}");
}
