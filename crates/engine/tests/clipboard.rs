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
        clip: None,
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

#[test]
fn copy_paste_preserves_image_raster_and_mleader_renamed_block() {
    use cadcraft_engine::doc::{Image, MLeader, Raster};

    let mut d_a = Drawing::new_metric();
    let img_path = "assets/plan_scan.png";
    let raster = Arc::new(Raster::new(2, 2, vec![255; 16]).unwrap());
    d_a.rasters.insert(img_path, Arc::clone(&raster));
    let img = Entity {
        handle: d_a.new_handle(),
        common: Common::default(),
        kind: EntityKind::Image(Image { path: img_path.into(), size: Vec2::new(2.0, 2.0), ..Image::default() }),
    };

    // MLeader whose block content uses anonymous block `*U1` (also present in target Drawing B).
    let u = line(&mut d_a, "0", (0.0, 0.0), (2.0, 0.0));
    block(&mut d_a, "*U1", vec![u]);
    let mut blk_ref = insert(&mut d_a, "*U1", (5.0, 5.0));
    let EntityKind::Insert(blk_ins) = blk_ref.kind else { unreachable!() };
    blk_ref.kind = EntityKind::MLeader(MLeader {
        leaders: vec![vec![Vec3::ZERO]],
        landing: Vec3::new(5.0, 5.0, 0.0),
        dogleg: 1.0,
        text: None,
        style: "Standard".into(),
        arrow_size: 2.5,
        spline: false,
        block: Some(blk_ins),
    });

    let copy_hs = [img.handle, blk_ref.handle];
    d_a.model.push(img);
    d_a.model.push(blk_ref);

    let mut s = Session::empty();
    let _a = s.open_drawing(d_a, "A", None);
    let _b = s.open_drawing(drawing_b(), "B", None);
    s.execute("document.switch", &json!({ "index": 0 })).unwrap();
    s.execute("copyclip", &json!({ "handles": copy_hs.iter().map(|h| h.hex()).collect::<Vec<_>>() })).unwrap();

    // Switch to Drawing B (already has a different `*U1` and no rasters) and paste.
    s.execute("document.switch", &json!({ "index": 1 })).unwrap();
    let pasted = s.execute("pasteclip", &json!({ "at": [10, 10] })).unwrap();
    let pasted_hs: Vec<Handle> = pasted["handles"].as_array().unwrap().iter().map(|v| Handle::parse_hex(v.as_str().unwrap()).unwrap()).collect();
    assert_eq!(pasted_hs.len(), 2);

    let d_b = s.doc().unwrap();
    // 1. Raster for Image carried across.
    assert!(d_b.rasters.get(img_path).is_some(), "image raster pixels must copy across drawings");

    // 2. MLeader's block name renamed away from colliding `*U1` to the freshly allocated `*U...`.
    let EntityKind::MLeader(ml) = &d_b.entity(pasted_hs[1]).unwrap().kind else { panic!("expected MLeader") };
    let bname = &ml.block.as_ref().unwrap().block;
    assert_ne!(bname, "*U1", "colliding anonymous block on MLeader must be renamed");
    assert!(d_b.block(bname).is_some(), "renamed MLeader block {bname} must exist in target drawing");
}

#[test]
fn copy_paste_handles_case_insensitive_block_renames_and_colliding_raster_keys() {
    use cadcraft_engine::doc::{Image, MLeader, Raster};

    // Source drawing defines `*u1` (lowercase), while target `drawing_b()` already has `*U1`.
    let mut d_a = Drawing::new_metric();
    let u_line = line(&mut d_a, "0", (0.0, 0.0), (2.0, 0.0));
    block(&mut d_a, "*u1", vec![u_line]);

    // An Image with `logo.png` (red 1x1) and a block containing an Image with the same `logo.png`.
    let src_raster = Arc::new(Raster::new(1, 1, vec![255, 0, 0, 255]).unwrap());
    d_a.rasters.insert("logo.png", Arc::clone(&src_raster));
    let img = Entity {
        handle: d_a.new_handle(),
        common: Common::default(),
        kind: EntityKind::Image(Image { path: "logo.png".into(), size: Vec2::new(2.0, 2.0), ..Image::default() }),
    };
    let blk_img = Entity {
        handle: d_a.new_handle(),
        common: Common::default(),
        kind: EntityKind::Image(Image { path: "logo.png".into(), size: Vec2::new(1.0, 1.0), ..Image::default() }),
    };
    block(&mut d_a, "IMG_BLK", vec![blk_img]);
    let blk_ins = insert(&mut d_a, "IMG_BLK", (1.0, 1.0));

    // Insert and MLeader referencing `*U1` with different casing than `*u1`.
    let ins_upper = insert(&mut d_a, "*U1", (3.0, 3.0));
    let mut ml_ref = insert(&mut d_a, "*U1", (5.0, 5.0));
    let EntityKind::Insert(ml_ins) = ml_ref.kind else { unreachable!() };
    ml_ref.kind = EntityKind::MLeader(MLeader {
        leaders: vec![vec![Vec3::ZERO]],
        landing: Vec3::new(5.0, 5.0, 0.0),
        dogleg: 1.0,
        text: None,
        style: "Standard".into(),
        arrow_size: 2.5,
        spline: false,
        block: Some(ml_ins),
    });

    let copy_hs = [img.handle, blk_ins.handle, ins_upper.handle, ml_ref.handle];
    d_a.model.push(img);
    d_a.model.push(blk_ins);
    d_a.model.push(ins_upper);
    d_a.model.push(ml_ref);

    // Target drawing already has a different `logo.png` (blue 1x1) and `*U1`.
    let mut d_b = drawing_b();
    let dst_raster = Arc::new(Raster::new(1, 1, vec![0, 0, 255, 255]).unwrap());
    d_b.rasters.insert("logo.png", Arc::clone(&dst_raster));

    let mut s = Session::empty();
    let _a = s.open_drawing(d_a, "A", None);
    let _b = s.open_drawing(d_b, "B", None);
    s.execute("document.switch", &json!({ "index": 0 })).unwrap();
    s.execute("copyclip", &json!({ "handles": copy_hs.iter().map(|h| h.hex()).collect::<Vec<_>>() })).unwrap();

    s.execute("document.switch", &json!({ "index": 1 })).unwrap();
    let pasted = s.execute("pasteclip", &json!({ "at": [10, 10] })).unwrap();
    let pasted_hs: Vec<Handle> = pasted["handles"].as_array().unwrap().iter().map(|v| Handle::parse_hex(v.as_str().unwrap()).unwrap()).collect();
    assert_eq!(pasted_hs.len(), 4);

    let d_b = s.doc().unwrap();
    // 1. Target's original `logo.png` must remain untouched.
    assert_eq!(d_b.rasters.get("logo.png").unwrap().rgba(), dst_raster.rgba());

    // 2. Pasted Image (and Image inside copied block) use the disambiguated key with source pixels.
    let EntityKind::Image(pasted_img) = &d_b.entity(pasted_hs[0]).unwrap().kind else { panic!("expected Image") };
    assert_ne!(pasted_img.path, "logo.png");
    assert_eq!(d_b.rasters.get(&pasted_img.path).unwrap().rgba(), src_raster.rgba());

    let blk = d_b.block("IMG_BLK").unwrap();
    let EntityKind::Image(blk_pasted_img) = &blk.entities.iter().next().unwrap().kind else { panic!("expected Image in block") };
    assert_eq!(blk_pasted_img.path, pasted_img.path);

    // 3. Both Insert and MLeader referencing `*U1` (case-insensitive vs `*u1`) point to the renamed block.
    let EntityKind::Insert(pasted_ins) = &d_b.entity(pasted_hs[2]).unwrap().kind else { panic!("expected Insert") };
    let EntityKind::MLeader(pasted_ml) = &d_b.entity(pasted_hs[3]).unwrap().kind else { panic!("expected MLeader") };
    assert!(!pasted_ins.block.eq_ignore_ascii_case("*U1"));
    assert_eq!(pasted_ml.block.as_ref().unwrap().block, pasted_ins.block);
    assert!(d_b.block(&pasted_ins.block).is_some());
}

#[test]
fn copy_paste_keeps_skipped_first_case_block_name_over_later_anonymous_collision() {
    let mut d_a = Drawing::new_metric();
    let foo_line = line(&mut d_a, "0", (0.0, 0.0), (1.0, 0.0));
    block(&mut d_a, "FOO", vec![foo_line]);

    let anon_line = line(&mut d_a, "0", (0.0, 0.0), (0.0, 2.0));
    let mut anon_b = Block::new("foo");
    anon_b.anonymous = true;
    anon_b.entities.push(anon_line);
    d_a.blocks.insert("foo".into(), Arc::new(anon_b));

    let ref_upper = insert(&mut d_a, "FOO", (0.0, 0.0));
    let ref_mixed = insert(&mut d_a, "Foo", (1.0, 0.0));
    let ref_lower = insert(&mut d_a, "foo", (2.0, 0.0));
    let copy_hs = [ref_upper.handle, ref_mixed.handle, ref_lower.handle];
    d_a.model.push(ref_upper);
    d_a.model.push(ref_mixed);
    d_a.model.push(ref_lower);

    let mut d_b = drawing_b();
    let b_line = line(&mut d_b, "0", (0.0, 0.0), (5.0, 0.0));
    block(&mut d_b, "FOO", vec![b_line]);

    let mut s = Session::empty();
    let _a = s.open_drawing(d_a, "A", None);
    let _b = s.open_drawing(d_b, "B", None);
    s.execute("document.switch", &json!({ "index": 0 })).unwrap();
    s.execute("copyclip", &json!({ "handles": copy_hs.iter().map(|h| h.hex()).collect::<Vec<_>>() })).unwrap();

    s.execute("document.switch", &json!({ "index": 1 })).unwrap();
    let pasted = s.execute("pasteclip", &json!({ "at": [10, 10] })).unwrap();
    let pasted_hs: Vec<Handle> = pasted["handles"].as_array().unwrap().iter().map(|v| Handle::parse_hex(v.as_str().unwrap()).unwrap()).collect();
    assert_eq!(pasted_hs.len(), 3);

    let d_b = s.doc().unwrap();
    let EntityKind::Insert(p_upper) = &d_b.entity(pasted_hs[0]).unwrap().kind else { panic!("expected Insert") };
    let EntityKind::Insert(p_mixed) = &d_b.entity(pasted_hs[1]).unwrap().kind else { panic!("expected Insert") };
    let EntityKind::Insert(p_lower) = &d_b.entity(pasted_hs[2]).unwrap().kind else { panic!("expected Insert") };
    assert_eq!(p_upper.block, "FOO");
    assert_eq!(p_mixed.block, "Foo");
    assert_ne!(p_lower.block, "foo");
    assert!(d_b.block(&p_lower.block).is_some());
}
