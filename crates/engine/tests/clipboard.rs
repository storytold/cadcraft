//! The clipboard between open drawings (#325): pasted objects bring the layers and blocks they
//! use, PASTEORIG pastes into another drawing at the same coordinates, and the clipboard can
//! travel as text through the system clipboard.

use std::sync::{Arc, Mutex};

use cadcraft_engine::Session;
use cadcraft_engine::cmd::clipboard;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::color::Color;
use cadcraft_engine::doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Insert, Layer, Line};
use cadcraft_engine::geom::{Vec2, Vec3};
use serde_json::json;

fn line(d: &mut Drawing, layer: &str, a: (f64, f64), b: (f64, f64)) -> Entity {
    let kind = EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) });
    Entity { handle: d.new_handle(), common: Common { layer: layer.into(), ..Common::default() }, kind }
}

fn insert(d: &mut Drawing, block: &str, at: (f64, f64)) -> Entity {
    let kind = EntityKind::Insert(Insert {
        block: block.into(),
        insert: Vec3::new(at.0, at.1, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    });
    Entity { handle: d.new_handle(), common: Common::default(), kind }
}

fn block(d: &mut Drawing, name: &str, ents: Vec<Entity>) {
    let mut b = Block::new(name);
    b.anonymous = name.starts_with('*');
    for e in ents {
        b.entities.push(e);
    }
    d.blocks.insert(name.into(), Arc::new(b));
}

/// Drawing A: a line on WALLS and a DOOR block reference; DOOR holds a line on HINGE and a
/// reference to the generated block *U1.
fn drawing_a() -> Drawing {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { color: Color::Index(1), ..Layer::new("WALLS") });
    d.layers.push(Layer { color: Color::Index(5), ..Layer::new("HINGE") });
    let u = line(&mut d, "0", (0.0, 0.0), (1.0, 1.0));
    block(&mut d, "*U1", vec![u]);
    let leaf = line(&mut d, "HINGE", (0.0, 0.0), (0.0, 2.0));
    let nested = insert(&mut d, "*U1", (0.0, 0.0));
    block(&mut d, "DOOR", vec![leaf, nested]);
    let wall = line(&mut d, "WALLS", (10.0, 20.0), (30.0, 20.0));
    let door = insert(&mut d, "DOOR", (15.0, 20.0));
    d.model.push(wall);
    d.model.push(door);
    d
}

/// Drawing B already has a WALLS layer (green) and a different *U1.
fn drawing_b() -> Drawing {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { color: Color::Index(3), ..Layer::new("WALLS") });
    let other = line(&mut d, "0", (5.0, 5.0), (6.0, 6.0));
    block(&mut d, "*U1", vec![other]);
    d
}

fn handles(s: &Session) -> Vec<String> {
    s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect()
}

fn model(s: &Session) -> Vec<Entity> {
    s.doc().unwrap().model.iter().map(|e| (**e).clone()).collect()
}

#[test]
fn paste_into_another_drawing_brings_layers_and_blocks() {
    let mut s = Session::empty();
    let a = s.open_drawing(drawing_a(), "A", None);
    let b = s.open_drawing(drawing_b(), "B", None);
    s.active = a;
    s.execute("copyclip", &json!({ "handles": handles(&s) })).unwrap();
    assert_eq!(s.clipboard.len(), 2);
    // The base point is the lower-left corner of the copied objects (the wall starts at 10,20).
    assert_eq!(s.clipboard_base, Vec2::new(10.0, 20.0));

    s.active = b;
    let before: Vec<Handle> = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
    let r = s.execute("pasteclip", &json!({ "at": [110, 20] })).unwrap();
    assert_eq!(r["handles"].as_array().map(Vec::len), Some(2));
    let d = s.doc().unwrap();
    // HINGE comes from A; WALLS keeps B's definition.
    assert_eq!(d.layer("HINGE").map(|l| l.color), Some(Color::Index(5)));
    assert_eq!(d.layer("WALLS").map(|l| l.color), Some(Color::Index(3)));
    // DOOR comes along, its nested *U1 under a fresh name because B has a *U1 of its own.
    let door = d.block("DOOR").expect("DOOR copied");
    let nested: Vec<String> =
        door.entities.iter().filter_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.block.clone()) } else { None }).collect();
    assert_eq!(nested, ["*U2"]);
    assert_eq!(d.block("*U2").map(|b| b.entities.len()), Some(1));
    let theirs = d.block("*U1").and_then(|b| b.entities.iter().next().cloned()).unwrap();
    assert!(matches!(&theirs.kind, EntityKind::Line(l) if l.a.x == 5.0), "B's *U1 is unchanged");
    // The objects moved by 100 in X, with new handles.
    let pasted = model(&s);
    assert_eq!(pasted.len(), 2);
    assert!(pasted.iter().all(|e| !before.contains(&e.handle)));
    assert!(matches!(&pasted[0].kind, EntityKind::Line(l) if l.a.x == 110.0 && l.a.y == 20.0));
    assert_eq!(pasted[0].common.layer, "WALLS");
    assert!(matches!(&pasted[1].kind, EntityKind::Insert(i) if i.block == "DOOR" && i.insert.x == 115.0));
    assert_eq!(s.selection().len(), 2, "the pasted objects are selected");

    // Undo removes the paste.
    s.execute("u", &json!({})).unwrap();
    assert!(s.doc().unwrap().model.is_empty());
}

#[test]
fn pasteorig_pastes_only_into_another_drawing() {
    let mut s = Session::empty();
    let a = s.open_drawing(drawing_a(), "A", None);
    let b = s.open_drawing(drawing_b(), "B", None);
    s.active = a;
    s.execute("copyclip", &json!({ "handles": handles(&s) })).unwrap();

    // In the drawing it came from, PASTEORIG is unavailable (greyed in menus).
    assert!(clipboard::from_active(&s));
    assert!(s.execute("pasteorig", &json!({})).is_err());
    assert!(s.start("pasteorig").is_err());
    assert_eq!(s.doc().unwrap().model.len(), 2);
    let info = cadcraft_engine::find_command("pasteorig").unwrap().info(&s);
    assert!(!info.enabled);

    // In another drawing it pastes at the same coordinates without asking.
    s.active = b;
    s.start("pasteorig").unwrap();
    assert!(s.running.is_none());
    let pasted = model(&s);
    assert!(matches!(&pasted[0].kind, EntityKind::Line(l) if l.a.x == 10.0 && l.b.x == 30.0 && l.a.y == 20.0));
    assert!(s.doc().unwrap().block("DOOR").is_some());

    // PASTECLIP asks for the insertion point and places the base point there.
    s.start("pasteclip").unwrap();
    assert!(s.prompt_text().contains("insertion point"), "{}", s.prompt_text());
    s.input(cadcraft_engine::Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    assert!(s.running.is_none());
    let pasted = model(&s);
    assert_eq!(pasted.len(), 4);
    assert!(matches!(&pasted[2].kind, EntityKind::Line(l) if l.a.x == 0.0 && l.a.y == 0.0));
}

#[test]
fn pasteblock_makes_a_block_of_the_clipboard() {
    let mut s = Session::empty();
    let a = s.open_drawing(drawing_a(), "A", None);
    let b = s.open_drawing(drawing_b(), "B", None);
    s.active = a;
    s.execute("copyclip", &json!({ "handles": handles(&s) })).unwrap();
    s.active = b;
    let r = s.execute("pasteblock", &json!({ "at": [0, 0] })).unwrap();
    let name = r["block"].as_str().unwrap().to_string();
    assert!(name.starts_with("A$C"), "{name}");
    let d = s.doc().unwrap();
    assert_eq!(d.model.len(), 1);
    let blk = d.block(&name).unwrap();
    assert_eq!(blk.entities.len(), 2);
    assert_eq!(blk.base, Vec3::new(10.0, 20.0, 0.0), "the clipboard's base point");
    assert!(d.block("DOOR").is_some() && d.layer("HINGE").is_some());
}

#[test]
fn cutbase_cuts_with_the_given_base_point() {
    let mut s = Session::empty();
    let mut da = drawing_a();
    if let Some(l) = da.layer_mut("WALLS") {
        l.locked = true;
    }
    let a = s.open_drawing(da, "A", None);
    let b = s.open_drawing(drawing_b(), "B", None);
    s.active = a;
    let r = s.execute("cutbase", &json!({ "handles": handles(&s), "base": [5, 5] })).unwrap();
    // The wall is on a locked layer, so only the door is cut.
    assert_eq!(r, json!({ "cut": 1, "message": "1 object was on a locked layer." }));
    assert_eq!(s.clipboard.len(), 1);
    assert_eq!(s.clipboard_base, Vec2::new(5.0, 5.0));
    let left = model(&s);
    assert_eq!(left.len(), 1);
    assert!(matches!(&left[0].kind, EntityKind::Line(_)), "the wall stays");

    // PASTECLIP places the base point at the insertion point.
    s.active = b;
    s.execute("pasteclip", &json!({ "at": [105, 5] })).unwrap();
    let pasted = model(&s);
    assert!(matches!(&pasted[0].kind, EntityKind::Insert(i) if i.block == "DOOR" && i.insert.x == 115.0 && i.insert.y == 20.0));

    // Undo in A brings the door back.
    s.active = a;
    s.execute("u", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 2);
}

#[test]
fn cutting_only_locked_objects_keeps_the_clipboard() {
    let mut s = Session::empty();
    let mut da = drawing_a();
    for l in &mut da.layers {
        l.locked = true;
    }
    s.open_drawing(da, "A", None);
    let hs = handles(&s);
    s.execute("copyclip", &json!({ "handles": [hs[0]] })).unwrap();
    let held = s.clipboard.clone();
    let r = s.execute("cutclip", &json!({ "handles": hs })).unwrap();
    assert_eq!(r, json!({ "cut": 0, "message": "2 objects were on a locked layer. Nothing was cut." }));
    assert_eq!(model(&s).len(), 2, "nothing was erased");
    assert_eq!(s.clipboard.len(), 1, "the clipboard keeps what it held");
    assert_eq!(s.clipboard[0].handle, held[0].handle);

    // CUTBASE at the command line says so too.
    s.start("cutbase").unwrap();
    s.input(cadcraft_engine::Input::Point(Vec2::new(3.0, 4.0))).unwrap();
    let wall = s.doc().unwrap().model.iter().next().unwrap().handle;
    s.input(cadcraft_engine::Input::Pick(vec![wall])).unwrap();
    s.input(cadcraft_engine::Input::Enter).unwrap();
    assert!(s.running.is_none());
    assert!(s.log.iter().any(|l| l.contains("1 object was on a locked layer. Nothing was cut.")), "{:?}", s.log);
    assert_eq!(s.clipboard.len(), 1);
    assert_eq!(model(&s).len(), 2);
}

#[test]
fn cutbase_and_copybase_need_a_base_point_and_take_each_object_once() {
    let mut s = Session::empty();
    s.open_drawing(drawing_a(), "A", None);
    let hs = handles(&s);
    for cmd in ["cutbase", "copybase"] {
        for p in [json!({ "handles": hs }), json!({ "handles": hs, "base": [5] })] {
            let e = s.execute(cmd, &p).unwrap_err().to_string();
            assert!(e.contains("`base` ([x, y]) is required"), "{cmd}: {e}");
        }
    }
    assert_eq!(model(&s).len(), 2, "nothing was cut");
    assert!(s.clipboard.is_empty(), "nothing was copied");

    // A handle listed twice is copied and cut once.
    let twice = json!({ "handles": [hs[1], hs[1]], "base": [0, 0] });
    assert_eq!(s.execute("copybase", &twice).unwrap(), json!({ "copied": 1 }));
    assert_eq!(s.clipboard.len(), 1);
    assert_eq!(s.execute("cutbase", &twice).unwrap(), json!({ "cut": 1 }));
    assert_eq!(s.clipboard.len(), 1);
    assert_eq!(model(&s).len(), 1);
}

#[test]
fn cutbase_asks_for_the_base_point_then_the_objects() {
    let mut s = Session::empty();
    s.open_drawing(drawing_a(), "A", None);
    let hs: Vec<Handle> = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
    let undo_before = s.state().unwrap().undo.len();
    s.start("cutbase").unwrap();
    assert!(s.prompt_text().contains("Specify base point"), "{}", s.prompt_text());
    s.input(cadcraft_engine::Input::Point(Vec2::new(3.0, 4.0))).unwrap();
    assert!(s.prompt_text().contains("Select objects"), "{}", s.prompt_text());
    s.input(cadcraft_engine::Input::Pick(vec![hs[1]])).unwrap();
    s.input(cadcraft_engine::Input::Enter).unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.clipboard.len(), 1);
    assert_eq!(s.clipboard_base, Vec2::new(3.0, 4.0));
    assert_eq!(model(&s).iter().map(|e| e.handle).collect::<Vec<_>>(), [hs[0]]);
    assert_eq!(s.state().unwrap().undo.len(), undo_before + 1, "one undo step");

    // With a pickfirst selection, the base point is the only prompt.
    s.set_selection(vec![hs[0]]);
    s.start("cutbase").unwrap();
    assert!(s.prompt_text().contains("Specify base point"), "{}", s.prompt_text());
    s.input(cadcraft_engine::Input::Point(Vec2::new(1.0, 2.0))).unwrap();
    assert!(s.running.is_none());
    assert!(s.doc().unwrap().model.is_empty());
    assert_eq!(s.clipboard_base, Vec2::new(1.0, 2.0));
    assert!(s.selection().is_empty());
}

/// A stand-in for the DXF writer and reader: "writes" the drawing to memory.
static WRITTEN: Mutex<Option<Drawing>> = Mutex::new(None);

#[test]
fn clipboard_travels_as_text() {
    set_io(IoHooks {
        read: |bytes, _| {
            if bytes.starts_with(b"0\nSECTION") {
                WRITTEN.lock().unwrap().clone().ok_or_else(|| "nothing written".into())
            } else {
                Err("not DXF".into())
            }
        },
        write: |d, name| {
            assert!(name.ends_with(".dxf"));
            *WRITTEN.lock().unwrap() = Some(d.clone());
            Ok(b"0\nSECTION\n2\nHEADER\n0\nENDSEC\n0\nEOF\n".to_vec())
        },
        plot: None,
    });
    let mut s = Session::empty();
    let a = s.open_drawing(drawing_a(), "A", None);
    s.execute("copybase", &json!({ "handles": handles(&s), "base": [10, 0] })).unwrap();
    let text = clipboard::system_text(&s).expect("DXF text");
    let written = WRITTEN.lock().unwrap().clone().unwrap();
    assert_eq!(written.model.len(), 2);
    assert!(written.block("DOOR").is_some() && written.layer("HINGE").is_some());
    assert_eq!(written.header.point("INSBASE"), Some(Vec3::new(10.0, 0.0, 0.0)));

    // Another CADCraft (here: a fresh session) pastes it, with its layers and blocks.
    let mut t = Session::empty();
    t.open_drawing(drawing_b(), "B", None);
    assert!(!clipboard::load_system_text(&mut t, "just some words"));
    assert!(t.clipboard.is_empty());
    assert!(clipboard::load_system_text(&mut t, &text));
    assert_eq!(t.clipboard.len(), 2);
    assert_eq!(t.clipboard_base, Vec2::new(10.0, 0.0));
    t.execute("pasteorig", &json!({})).unwrap();
    assert_eq!(t.doc().unwrap().model.len(), 2);
    assert!(t.doc().unwrap().block("DOOR").is_some() && t.doc().unwrap().layer("HINGE").is_some());
    let _ = a;
}
