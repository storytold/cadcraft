//! INSERT of exploded blocks (`*NAME`) and of drawing files (`-INSERT path`, `NAME=path`, JSON
//! `file`), read through the io hooks (#340).

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::color::Color;
use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Block, Circle, Drawing, Entity, EntityKind, HVal, Handle, Insert, Layer, Line};
use serde_json::json;

/// A drawing file in a test format: its bytes are "x,y", the $INSBASE. Model space: a line from
/// 10,10 to 20,10 on layer PARTS (colour 1), and a reference to block NUT (a circle) at 10,10.
fn read(bytes: &[u8], _name: &str) -> Result<Drawing, String> {
    let text = String::from_utf8_lossy(bytes);
    let (x, y) = text.trim().split_once(',').ok_or("bad test file")?;
    let (x, y): (f64, f64) = (x.parse().map_err(|_| "x")?, y.parse().map_err(|_| "y")?);
    let mut d = Drawing::new_imperial();
    d.header.set("INSBASE", HVal::Point(Vec3::new(x, y, 0.0)));
    let mut parts = Layer::new("PARTS");
    parts.color = Color::Index(1);
    d.layers.push(parts);
    let mut nut = Block::new("NUT");
    nut.entities.push(Entity::new(Handle(0x50), EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 1.0 })));
    d.blocks.insert("NUT".into(), std::sync::Arc::new(nut));
    let mut line = Entity::new(Handle(0x60), EntityKind::Line(Line { a: Vec3::new(10.0, 10.0, 0.0), b: Vec3::new(20.0, 10.0, 0.0) }));
    line.common.layer = "PARTS".into();
    d.model.push(line);
    let ins = Insert {
        block: "NUT".into(),
        insert: Vec3::new(10.0, 10.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    d.model.push(Entity::new(Handle(0x61), EntityKind::Insert(ins)));
    Ok(d)
}

fn hooks() {
    set_io(IoHooks { read, write: |_, _| Err("no writer in this test".into()), plot: None });
}

/// Write a test drawing file with $INSBASE `base`; returns its path.
fn file(name: &str, base: &str) -> String {
    let dir = std::env::temp_dir().join(format!("cadcraft-insert-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, base).unwrap();
    p.to_string_lossy().into_owned()
}

/// Lines in model space as (ax, ay, bx, by), rounded.
fn lines(s: &Session) -> Vec<(f64, f64, f64, f64)> {
    let r = |v: f64| (v * 1e6).round() / 1e6;
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some((r(l.a.x), r(l.a.y), r(l.b.x), r(l.b.y))),
            _ => None,
        })
        .collect()
}

/// Block references in model space as (block, x, y).
fn inserts(s: &Session) -> Vec<(String, f64, f64)> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(i) => Some((i.block.clone(), i.insert.x, i.insert.y)),
            _ => None,
        })
        .collect()
}

#[test]
fn star_name_inserts_the_blocks_objects_with_one_scale_factor() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.execute("block", &json!({ "name": "B1", "base": [0, 0], "keep": "delete", "handles": ["100"] })).unwrap();

    // Insertion point, then one scale factor and the rotation; no block reference is left.
    s.script("-INSERT *B1 5,5 2 90\n").unwrap();
    assert!(s.running.is_none(), "INSERT finished: {}", s.prompt_text());
    assert!(inserts(&s).is_empty());
    assert_eq!(lines(&s), vec![(5.0, 5.0, 5.0, 25.0)]);

    // A plain name after an exploded one inserts a block reference again.
    s.script("-INSERT B1 0,0 1 1 0\n").unwrap();
    assert_eq!(inserts(&s).len(), 1);

    // JSON: "*NAME" explodes too.
    s.execute("insert", &json!({ "name": "*B1", "at": [0, 50] })).unwrap();
    assert_eq!(inserts(&s).len(), 1);
    assert_eq!(lines(&s).last(), Some(&(0.0, 50.0, 10.0, 50.0)));
}

#[test]
fn a_drawing_file_becomes_a_block_with_its_insbase_and_tables() {
    hooks();
    let bolt = file("bolt.dxf", "10,10");
    let mut s = Session::new();
    // This drawing's own layer PARTS keeps its colour.
    s.execute("layer.new", &json!({ "name": "PARTS", "color": 3 })).unwrap();

    s.script(&format!("-INSERT {bolt} 100,100 1 1 0\n")).unwrap();
    assert!(s.running.is_none(), "INSERT finished: {}", s.prompt_text());
    let d = s.doc().unwrap();
    let b = d.block("bolt").expect("block named after the file");
    assert_eq!(b.base, Vec3::new(10.0, 10.0, 0.0), "$INSBASE is the base point");
    assert_eq!(b.entities.len(), 2);
    assert!(d.block("NUT").is_some(), "nested blocks come along");
    assert_eq!(d.layer("PARTS").map(|l| l.color), Some(Color::Index(3)), "an existing layer keeps its definition");
    assert_eq!(inserts(&s), vec![("bolt".into(), 100.0, 100.0)]);

    // The file again: its block exists, so INSERT asks; No (the default) keeps the definition.
    s.cmdline(&format!("-INSERT {bolt}")).unwrap();
    assert!(s.prompt_text().contains("Redefine"), "{}", s.prompt_text());
    s.script("N 0,0 1 1 0\n").unwrap();
    assert!(s.running.is_none(), "INSERT finished: {}", s.prompt_text());
    assert_eq!(inserts(&s).len(), 2);

    // NAME=path defines the block under that name without asking; *path inserts the file's objects.
    let pin = file("pin.dxf", "10,10");
    s.script(&format!("-INSERT M8={bolt} 0,0 1 1 0\n-INSERT *{pin} 50,0 1 0\n")).unwrap();
    assert!(s.running.is_none(), "INSERT finished: {}", s.prompt_text());
    assert!(s.doc().unwrap().block("M8").is_some());
    assert_eq!(inserts(&s).iter().filter(|i| i.0 == "M8").count(), 1);
    // The exploded line, moved from the base point 10,10 to 50,0; the nested NUT stays a reference.
    assert_eq!(lines(&s), vec![(50.0, 0.0, 60.0, 0.0)]);
    assert!(inserts(&s).iter().any(|i| i.0 == "NUT" && i.1 == 50.0 && i.2 == 0.0));

    // JSON: `file` with an optional block name, never a dialog.
    let r = s.execute("insert", &json!({ "file": file("washer.dxf", "0,0"), "at": [5, 5] })).unwrap();
    assert!(r["handle"].is_string(), "{r}");
    assert!(inserts(&s).iter().any(|i| i.0 == "washer"));

    // A file that can't be read is reported; nothing is inserted.
    let n = inserts(&s).len();
    assert!(s.execute("insert", &json!({ "file": "/no/such/part.dxf", "at": [0, 0] })).is_err());
    s.cmdline("-INSERT /no/such/part.dxf").unwrap();
    assert!(s.log.iter().any(|m| m.contains("/no/such/part.dxf")), "{:?}", s.log.last());
    assert!(s.prompt_text().contains("block name"), "{}", s.prompt_text());
    assert_eq!(inserts(&s).len(), n);
}

#[test]
fn exploded_json_insert_is_one_undo_step() {
    for params in [json!({ "name": "*B", "at": [5, 5] }), json!({ "name": "B", "at": [5, 5], "explode": true })] {
        let mut s = Session::new();
        s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
        s.execute("block", &json!({ "name": "B", "base": [0, 0], "keep": "delete", "handles": ["100"] })).unwrap();
        let undo = s.state().unwrap().undo.len();
        s.execute("insert", &params).unwrap();
        assert_eq!(lines(&s), vec![(5.0, 5.0, 15.0, 5.0)], "{params}");
        assert_eq!(s.state().unwrap().undo.len(), undo + 1, "{params}");

        s.execute("undo", &json!({})).unwrap();
        assert!(lines(&s).is_empty(), "{params}");
        assert!(inserts(&s).is_empty(), "{params}");
        assert!(s.doc().unwrap().block("B").is_some(), "{params}");

        // The second undo reverts the block creation, not a leftover EXPLODE step.
        s.execute("undo", &json!({})).unwrap();
        assert!(inserts(&s).is_empty(), "{params}");
        assert!(s.doc().unwrap().block("B").is_none(), "{params}");
        assert_eq!(lines(&s), vec![(0.0, 0.0, 10.0, 0.0)], "{params}");
    }
}

/// #1997: a file drawn in inches comes into a drawing in metres at 0.0254, on top of the scale asked for.
#[test]
fn a_file_in_other_units_is_scaled_to_this_drawings_units() {
    hooks();
    let part = file("inches.dxf", "0,0");
    let scale_of = |s: &Session| {
        let d = s.doc().unwrap();
        d.model.iter().rev().find_map(|e| if let EntityKind::Insert(i) = &e.kind { Some((i.scale.x, i.scale.y)) } else { None }).unwrap()
    };
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-12 && (a.1 - b.1).abs() < 1e-12;
    let mut s = Session::new();
    s.execute("units", &json!({ "insunits": 6 })).unwrap();
    s.execute("insert", &json!({ "file": part, "at": [0, 0] })).unwrap();
    assert!(near(scale_of(&s), (0.0254, 0.0254)), "{:?}", scale_of(&s));
    s.execute("insert", &json!({ "file": part, "at": [0, 0], "scale": [2, 3] })).unwrap();
    assert!(near(scale_of(&s), (0.0508, 0.0762)), "{:?}", scale_of(&s));
    // Interactive INSERT converts too.
    s.script(&format!("-INSERT inches={part} 0,0 1 1 0\n")).unwrap();
    assert!(near(scale_of(&s), (0.0254, 0.0254)), "{:?}", scale_of(&s));
    // A drawing in the same units, or without any, takes the file as is.
    for units in [1, 0] {
        let mut s = Session::new();
        s.execute("units", &json!({ "insunits": units })).unwrap();
        s.execute("insert", &json!({ "file": part, "at": [0, 0] })).unwrap();
        assert!(near(scale_of(&s), (1.0, 1.0)), "INSUNITS {units}: {:?}", scale_of(&s));
    }
}
