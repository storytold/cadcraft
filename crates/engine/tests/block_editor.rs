//! The Block Editor (BEDIT, BSAVE, BSAVEAS, BCLOSE): a block is edited with the ordinary
//! commands, its base point at the origin, and saving it updates every reference in the drawing.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::{EntityKind, Line};
use cadcraft_engine::geom::Vec3;
use serde_json::{Value, json};

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Value {
    s.execute("line", &json!({ "points": [a, b] })).unwrap()["handles"][0].clone()
}

/// A drawing with block "Door" (one line from (10,10) to (12,10), base point (10,10)) and two
/// references to it; returns the references.
fn door(s: &mut Session) -> (Value, Value) {
    let l = line(s, [10.0, 10.0], [12.0, 10.0]);
    let a = s.execute("block", &json!({ "name": "Door", "base": [10, 10], "handles": [l] })).unwrap()["insert"].clone();
    let b = s.execute("insert", &json!({ "name": "Door", "at": [50, 0] })).unwrap()["handle"].clone();
    (a, b)
}

fn lines_of(s: &Session, block: &str) -> Vec<Line> {
    let b = s.doc().unwrap().block(block).unwrap().clone();
    b.entities.iter().filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some(l.clone()) } else { None }).collect()
}

fn editor(s: &Session) -> Option<String> {
    s.state().unwrap().block_editor().map(str::to_string)
}

#[test]
fn saving_the_block_updates_every_reference() {
    let mut s = Session::new();
    let (a, b) = door(&mut s);
    let model = s.doc().unwrap().model.clone();

    s.execute("bedit", &json!({ "name": "door" })).unwrap();
    assert_eq!(editor(&s).as_deref(), Some("Door"));
    // The editor shows the block's objects with its base point at the origin.
    let shown: Vec<_> = s.doc().unwrap().model.iter().map(|e| e.kind.clone()).collect();
    assert!(matches!(&shown[..], [EntityKind::Line(l)] if l.a == Vec3::new(0.0, 0.0, 0.0) && l.b == Vec3::new(2.0, 0.0, 0.0)), "{shown:?}");
    assert_eq!(s.execute("getvar", &json!({ "name": "BLOCKEDITOR" })).unwrap()["value"], json!(1));

    line(&mut s, [0.0, 0.0], [0.0, 3.0]);
    assert!(s.state().unwrap().block_unsaved() && s.state().unwrap().is_dirty());
    let r = s.execute("bclose", &json!({})).unwrap();
    assert_eq!(r, json!({ "block": "Door", "saved": true }));
    assert_eq!(editor(&s), None);

    // The new line is in the definition, in block coordinates; the drawing's own objects and both
    // references are as they were.
    let ls = lines_of(&s, "Door");
    assert_eq!(ls.len(), 2);
    assert_eq!((ls[1].a, ls[1].b), (Vec3::new(10.0, 10.0, 0.0), Vec3::new(10.0, 13.0, 0.0)));
    let d = s.doc().unwrap();
    assert!(d.model.same_as(&model) || d.model == model);
    for h in [&a, &b] {
        let h = cadcraft_engine::cmd::curves::handle_value(h).unwrap();
        assert!(matches!(&d.entity(h).unwrap().kind, EntityKind::Insert(i) if i.block == "Door"));
    }

    // One undo step in the drawing takes the edit back.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(lines_of(&s, "Door").len(), 1);
}

#[test]
fn closing_without_saving_leaves_the_drawing_as_it_was() {
    let mut s = Session::new();
    door(&mut s);
    let before = s.doc().unwrap().clone();
    let undo = s.state().unwrap().undo.len();
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    s.script("ERASE ALL\n\n").unwrap();
    assert!(s.doc().unwrap().model.is_empty());
    s.execute("bclose", &json!({ "save": false })).unwrap();
    assert_eq!(*s.doc().unwrap(), before);
    assert_eq!(s.state().unwrap().undo.len(), undo);
    assert_eq!(lines_of(&s, "Door").len(), 1);
}

#[test]
fn unchanged_objects_keep_their_exact_coordinates() {
    let mut s = Session::new();
    // A base point whose coordinates don't survive x - b + b exactly.
    let l = line(&mut s, [0.1, 0.7], [0.3, 0.9]);
    s.execute("block", &json!({ "name": "B", "base": [0.7, 0.1], "handles": [l] })).unwrap();
    let original = lines_of(&s, "B");
    s.execute("bedit", &json!({ "name": "B" })).unwrap();
    line(&mut s, [0.0, 0.0], [1.0, 0.0]);
    s.execute("bsave", &json!({})).unwrap();
    assert!(!s.state().unwrap().block_unsaved());
    assert_eq!(editor(&s).as_deref(), Some("B"), "BSAVE keeps the editor open");
    assert_eq!(lines_of(&s, "B")[0], original[0]);
    s.execute("bclose", &json!({})).unwrap();
    let ls = lines_of(&s, "B");
    assert_eq!(ls[0], original[0]);
    assert_eq!(ls.len(), 2);
}

#[test]
fn bsaveas_makes_a_new_block_and_keeps_the_old_one() {
    let mut s = Session::new();
    door(&mut s);
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    line(&mut s, [0.0, 0.0], [0.0, 3.0]);
    let r = s.execute("bsaveas", &json!({ "name": "Door2" })).unwrap();
    assert_eq!(r["block"], "Door2");
    assert_eq!(editor(&s).as_deref(), Some("Door2"));
    assert!(s.execute("bsaveas", &json!({ "name": "Door" })).is_err(), "an existing block is redefined only with replace");
    assert!(s.execute("bsaveas", &json!({ "name": "bad*name" })).is_err());
    s.execute("bclose", &json!({})).unwrap();
    assert_eq!(lines_of(&s, "Door").len(), 1);
    let new = lines_of(&s, "Door2");
    assert_eq!(new.len(), 2);
    assert_eq!(s.doc().unwrap().block("Door2").unwrap().base, s.doc().unwrap().block("Door").unwrap().base);

    // Redefining another block (typed in another case) keeps its name and takes the edited
    // block's base point, so the objects keep their places relative to it.
    let l = line(&mut s, [100.0, 100.0], [101.0, 100.0]);
    s.execute("block", &json!({ "name": "Other", "base": [100, 100], "handles": [l] })).unwrap();
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    s.execute("bsaveas", &json!({ "name": "OTHER", "replace": true })).unwrap();
    s.execute("bclose", &json!({})).unwrap();
    let d = s.doc().unwrap();
    let other = d.block("Other").unwrap();
    assert_eq!(other.name, "Other");
    assert_eq!(other.base, d.block("Door").unwrap().base);
    assert_eq!(lines_of(&s, "Other"), lines_of(&s, "Door"));
}

#[test]
fn what_the_block_editor_refuses() {
    let mut s = Session::new();
    door(&mut s);
    assert!(s.execute("bedit", &json!({ "name": "Nope" })).is_err());
    assert!(s.execute("bedit", &json!({})).is_err(), "no name and no block reference selected");
    assert!(s.execute("bsave", &json!({})).is_err(), "not open");
    assert!(s.execute("bclose", &json!({})).is_err(), "not open");

    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    s.execute("bedit", &json!({ "name": "DOOR" })).unwrap();
    assert_eq!(editor(&s).as_deref(), Some("Door"), "the block already open stays open");
    let r = s.execute("layout.set", &json!({ "name": "Layout1" }));
    assert!(r.is_err_and(|e| e.to_string().contains("Block Editor")));
    assert_eq!(s.layout_space(), cadcraft_engine::doc::Space::Model);

    // A reference to the block itself can't be saved into it.
    s.execute("insert", &json!({ "name": "Door", "at": [5, 5] })).unwrap();
    assert!(s.execute("bsave", &json!({})).is_err_and(|e| e.to_string().contains("itself")));
    assert!(s.execute("bclose", &json!({})).is_err());
    assert!(editor(&s).is_some(), "a failed save keeps the editor open");
    s.execute("bclose", &json!({ "save": false })).unwrap();
    assert_eq!(lines_of(&s, "Door").len(), 1);

    // Anonymous blocks are made by other objects.
    let mut d = s.doc().unwrap().clone();
    d.blocks.insert("*U1".into(), std::sync::Arc::new(cadcraft_engine::doc::Block::new("*U1")));
    s.open_drawing(d, "anon", None);
    assert!(s.execute("bedit", &json!({ "name": "*U1" })).is_err_and(|e| e.to_string().contains("anonymous")));
}

#[test]
fn hostile_params_never_panic() {
    let mut s = Session::new();
    door(&mut s);
    for p in
        [json!({ "name": 5 }), json!({ "handle": "zz" }), json!({ "handle": -1 }), json!({ "handle": "FFFFFFFFFFFFFFFFFF" }), json!(null), json!([])]
    {
        assert!(s.execute("bedit", &p).is_err(), "{p}");
    }
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    for p in [json!({ "name": "" }), json!({ "name": "  " }), json!({ "name": 7 }), json!({ "name": "x".repeat(300) })] {
        assert!(s.execute("bsaveas", &p).is_err(), "{p}");
    }
    assert!(s.execute("bclose", &json!({ "save": "maybe" })).is_ok());
}

#[test]
fn a_selected_reference_opens_its_block_and_bclose_asks_to_save() {
    let mut s = Session::new();
    let (a, _) = door(&mut s);
    s.set_selection(vec![cadcraft_engine::cmd::curves::handle_value(&a).unwrap()]);
    s.start("bedit").unwrap();
    assert!(s.running.is_none());
    assert_eq!(editor(&s).as_deref(), Some("Door"));

    // Nothing changed: BCLOSE closes at once.
    s.script("BCLOSE\n").unwrap();
    assert_eq!(editor(&s), None);

    // Typed name, a change, then BCLOSE No discards it.
    s.script("BEDIT Door\nLINE 0,0 5,5\n\nBCLOSE\nN\n").unwrap();
    assert_eq!(editor(&s), None);
    assert_eq!(lines_of(&s, "Door").len(), 1);
    // BCLOSE Yes saves it.
    s.script("BEDIT Door\nLINE 0,0 5,5\n\nBCLOSE\n\n").unwrap();
    assert_eq!(lines_of(&s, "Door").len(), 2);
    assert_eq!(s.execute("getvar", &json!({ "name": "BLOCKEDITOR" })).map(|v| v["value"].clone()).unwrap_or(Value::Null), json!(0));
}

#[test]
fn a_file_save_writes_the_drawing_outside_the_editor() {
    // The writer records how many objects the drawing's model space has, and block Door's.
    set_io(IoHooks {
        read: |_, _| Err("no reader in this test".into()),
        write: |d, _| Ok(format!("{} {}", d.model.len(), d.block("Door").map_or(0, |b| b.entities.len())).into_bytes()),
        plot: None,
    });
    let mut s = Session::new();
    door(&mut s);
    let path = std::env::temp_dir().join(format!("cadcraft-bedit-{}.dxf", std::process::id()));
    let path = path.to_string_lossy().to_string();
    s.execute("saveas", &json!({ "path": path })).unwrap();
    assert!(!s.state().unwrap().is_dirty());

    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    line(&mut s, [0.0, 0.0], [0.0, 3.0]);
    // Autosave keeps the editor's unsaved change in the block.
    let rec = s.state().unwrap().recovery_drawing().unwrap();
    assert_eq!(rec.block("Door").unwrap().entities.len(), 2);
    assert_eq!(rec.model.len(), 2);

    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "2 1", "the drawing, without the unsaved block change");
    assert!(s.state().unwrap().is_dirty(), "the block change is still unsaved");
    s.execute("bsave", &json!({})).unwrap();
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "2 2");
    assert!(!s.state().unwrap().is_dirty());
    s.execute("bclose", &json!({})).unwrap();
    assert!(!s.state().unwrap().is_dirty());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_dynamic_block_from_another_program_says_its_parameters_are_kept() {
    use cadcraft_engine::doc::foreign::{Foreign, ForeignExtra, ForeignObject};
    use cadcraft_engine::doc::{Handle, RawTag};
    let mut s = Session::new();
    door(&mut s);
    // As a DXF from AutoCAD has it: the block record's extension dictionary holds ACAD_ENHANCEDBLOCK.
    let mut f = Foreign::default();
    let xdict = Handle(0xF00);
    f.records.insert(("BLOCK_RECORD".into(), "DOOR".into()), ForeignExtra { xdict: Some(xdict), ..Default::default() });
    let tag = |code, value: &str| RawTag { code, value: value.into() };
    f.objects.push(ForeignObject { handle: xdict, kind: "DICTIONARY".into(), tags: vec![tag(3, "ACAD_ENHANCEDBLOCK"), tag(360, "F01")] });
    let mut d = s.doc().unwrap().clone();
    d.foreign = std::sync::Arc::new(f);
    s.open_drawing(d, "dynamic", None);
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    assert!(s.log.iter().any(|l| l.contains("dynamic block")), "{:?}", s.log);
    // A plain block says nothing of the kind.
    s.execute("bclose", &json!({})).unwrap();
    let mut s = Session::new();
    door(&mut s);
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    assert!(!s.log.iter().any(|l| l.contains("dynamic block")), "{:?}", s.log);
}

#[test]
fn a_dynamic_reference_with_its_own_values_opens_its_dynamic_block() {
    use cadcraft_engine::doc::foreign::{Foreign, ForeignExtra};
    use cadcraft_engine::doc::{Block, Handle, RawTag};
    let mut s = Session::new();
    let (a, _) = door(&mut s);
    // As AutoCAD leaves it: the reference shows anonymous block *U7, whose record's
    // AcDbBlockRepBTag xdata names Door's record (file handle 1D).
    let mut d = s.doc().unwrap().clone();
    let door_block = d.block("Door").unwrap().clone();
    d.blocks.insert("*U7".into(), std::sync::Arc::new(Block { name: "*U7".into(), anonymous: true, ..(*door_block).clone() }));
    let h = cadcraft_engine::cmd::curves::handle_value(&a).unwrap();
    d.model.modify(h, |e| {
        if let EntityKind::Insert(i) = &mut e.kind {
            i.block = "*U7".into();
        }
    });
    let mut f = Foreign::default();
    let tag = |code, value: &str| RawTag { code, value: value.into() };
    f.records.insert(
        ("BLOCK_RECORD".into(), "*U7".into()),
        ForeignExtra { xdata: vec![tag(1001, "AcDbBlockRepBTag"), tag(1070, "1"), tag(1005, "1D")], ..Default::default() },
    );
    f.targets.insert(Handle(0x1D), ("BLOCK_RECORD".into(), "DOOR".into()));
    d.foreign = std::sync::Arc::new(f);
    s.open_drawing(d, "dynamic", None);

    // Selected (the right-click Block Editor) and by handle: both open Door.
    s.set_selection(vec![h]);
    s.start("bedit").unwrap();
    assert_eq!(editor(&s).as_deref(), Some("Door"));
    s.execute("bclose", &json!({})).unwrap();
    s.execute("bedit", &json!({ "handle": h.hex() })).unwrap();
    assert_eq!(editor(&s).as_deref(), Some("Door"));
    s.execute("bclose", &json!({})).unwrap();
    // Other anonymous blocks are still refused by name.
    assert!(s.execute("bedit", &json!({ "name": "*U7" })).is_err());
}

#[test]
fn bedit_can_start_a_new_block() {
    let mut s = Session::new();
    door(&mut s);
    // An existing name edits that block.
    let r = s.execute("bedit", &json!({ "name": "door", "create": true })).unwrap();
    assert_eq!(r, json!({ "block": "Door", "new": false }));
    s.execute("bclose", &json!({})).unwrap();

    // A new name starts an empty block; closing without saving creates nothing.
    let r = s.execute("bedit", &json!({ "name": " Window ", "create": true })).unwrap();
    assert_eq!(r, json!({ "block": "Window", "new": true }));
    assert!(s.doc().unwrap().model.is_empty());
    s.execute("bclose", &json!({ "save": false })).unwrap();
    assert!(s.doc().unwrap().block("Window").is_none());

    // Saving creates it, base point at the origin.
    s.execute("bedit", &json!({ "name": "Window", "create": true })).unwrap();
    line(&mut s, [0.0, 0.0], [3.0, 0.0]);
    s.execute("bclose", &json!({})).unwrap();
    let d = s.doc().unwrap();
    let w = d.block("Window").unwrap();
    assert_eq!((w.name.as_str(), w.entities.len(), w.base), ("Window", 1, Vec3::new(0.0, 0.0, 0.0)));

    // Without `create` a missing block is an error; invalid new names are refused.
    assert!(s.execute("bedit", &json!({ "name": "Skylight" })).is_err());
    for bad in ["", "a*b", "*U9", "x<y"] {
        assert!(s.execute("bedit", &json!({ "name": bad, "create": true })).is_err(), "{bad}");
    }
    assert!(s.state().unwrap().block_editor().is_none());
}

#[test]
fn the_current_drawing_opens_in_the_block_editor() {
    use cadcraft_engine::blockedit::{CURRENT_DRAWING, has_authoring_elements};
    use cadcraft_engine::doc::foreign::{Foreign, ForeignExtra, ForeignObject};
    use cadcraft_engine::doc::{Handle, RawTag};
    let mut s = Session::new();
    line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    // A dynamic block's own drawing: model space's record carries ACAD_ENHANCEDBLOCK.
    let mut d = s.doc().unwrap().clone();
    assert!(!has_authoring_elements(&d));
    let mut f = Foreign::default();
    let tag = |code, value: &str| RawTag { code, value: value.into() };
    f.records.insert(("BLOCK_RECORD".into(), "*MODEL_SPACE".into()), ForeignExtra { xdict: Some(Handle(0xF00)), ..Default::default() });
    f.objects.push(ForeignObject { handle: Handle(0xF00), kind: "DICTIONARY".into(), tags: vec![tag(3, "ACAD_ENHANCEDBLOCK"), tag(360, "F01")] });
    d.foreign = std::sync::Arc::new(f);
    assert!(has_authoring_elements(&d));
    s.open_drawing(d, "symbol", None);

    // Opened on the drawing itself: its objects are there to edit, and it says it is dynamic.
    let r = s.execute("bedit", &json!({ "name": "<current drawing>", "create": true })).unwrap();
    assert_eq!(r, json!({ "block": CURRENT_DRAWING, "new": false }));
    assert_eq!(editor(&s).as_deref(), Some(CURRENT_DRAWING));
    assert_eq!(s.doc().unwrap().model.len(), 1);
    assert!(s.log.iter().any(|l| l.contains("dynamic block")), "{:?}", s.log);
    // A block of it can't be saved under another name here.
    assert!(s.execute("bsaveas", &json!({ "name": "Copy" })).is_err());
    // BCLOSE without saving puts the drawing back.
    line(&mut s, [0.0, 5.0], [10.0, 5.0]);
    s.execute("bclose", &json!({ "save": false })).unwrap();
    assert_eq!(editor(&s), None);
    assert_eq!(s.doc().unwrap().model.len(), 1);

    // Saved, the change is the drawing's: one undo step takes it back.
    s.execute("bedit", &json!({ "name": CURRENT_DRAWING })).unwrap();
    line(&mut s, [0.0, 5.0], [10.0, 5.0]);
    s.execute("bclose", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 2);
    assert!(s.doc().unwrap().block(CURRENT_DRAWING).is_none(), "no block is made");
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 1);
}

/// BEDIT in the Block Editor switches to another block, as AutoCAD's does: changes not saved yet
/// are saved or discarded first (`save`), and a block it can't open leaves the editor as it was.
#[test]
fn bedit_in_the_block_editor_switches_blocks() {
    let mut s = Session::new();
    door(&mut s);
    let l = line(&mut s, [0.0, 50.0], [5.0, 50.0]);
    s.execute("block", &json!({ "name": "Frame", "base": [0, 50], "handles": [l] })).unwrap();
    let model = s.doc().unwrap().model.len();

    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    line(&mut s, [0.0, 0.0], [0.0, 3.0]);
    // Changes not saved: JSON must say what to do with them.
    let e = s.execute("bedit", &json!({ "name": "Frame" })).unwrap_err().to_string();
    assert!(e.contains("save"), "{e}");
    assert_eq!(editor(&s).as_deref(), Some("Door"));
    assert!(s.state().unwrap().block_unsaved(), "nothing was lost");
    // A block that isn't there, or `save` that isn't a bool: refused, nothing changes.
    assert!(s.execute("bedit", &json!({ "name": "Nope", "save": true })).is_err());
    assert!(s.execute("bedit", &json!({ "name": "Frame", "save": "yes" })).is_err());
    assert_eq!(editor(&s).as_deref(), Some("Door"));
    assert!(s.state().unwrap().block_unsaved());

    // Saved, then switched.
    let r = s.execute("bedit", &json!({ "name": "frame", "save": true })).unwrap();
    assert_eq!((r["block"].as_str(), r["saved"].as_bool()), (Some("Frame"), Some(true)));
    assert_eq!(editor(&s).as_deref(), Some("Frame"));
    assert_eq!(lines_of(&s, "Door").len(), 2, "Door keeps the new line");

    // Discarded, then switched; with nothing to save `save` isn't needed.
    line(&mut s, [1.0, 1.0], [2.0, 2.0]);
    s.execute("bedit", &json!({ "name": "Door", "save": false })).unwrap();
    assert_eq!(lines_of(&s, "Frame").len(), 1, "Frame's change was discarded");
    s.execute("bedit", &json!({ "name": "Frame" })).unwrap();
    assert_eq!(editor(&s).as_deref(), Some("Frame"));
    // A new block, and the drawing itself.
    s.execute("bedit", &json!({ "name": "Gate", "create": true })).unwrap();
    assert_eq!(editor(&s).as_deref(), Some("Gate"));
    s.execute("bedit", &json!({ "name": "<Current Drawing>" })).unwrap();
    s.execute("bclose", &json!({ "save": false })).unwrap();
    assert!(editor(&s).is_none());
    assert_eq!(s.doc().unwrap().model.len(), model, "the drawing as it was");
}

/// A block BEDIT can't switch to is refused before it asks about the changes not saved, so the
/// question never traps the user; `check` asks the same of JSON callers.
#[test]
fn bedit_refuses_a_bad_target_before_asking_to_save() {
    let mut s = Session::new();
    door(&mut s);
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    line(&mut s, [1.0, 1.0], [2.0, 2.0]);
    assert!(s.state().unwrap().block_unsaved());
    s.script(
        "BEDIT
Nope
",
    )
    .unwrap();
    let message = s.current_prompt().map(|p| p.message).unwrap_or_default();
    assert!(!message.contains("Save changes"), "no question for a block it can't open: {message}");
    s.input(cadcraft_engine::Input::Cancel).unwrap();
    assert_eq!(editor(&s).as_deref(), Some("Door"));
    assert!(s.state().unwrap().block_unsaved(), "nothing lost");
    // The check, without switching or saving.
    assert!(s.execute("bedit", &json!({ "name": "Nope", "check": true })).is_err());
    assert!(s.execute("bedit", &json!({ "name": "Nope", "create": true, "check": true })).is_ok(), "a new name is fine");
    assert_eq!(editor(&s).as_deref(), Some("Door"));
    assert!(s.state().unwrap().block_unsaved());
}

/// Typed, BEDIT in the Block Editor asks whether to save the changes before it switches.
#[test]
fn bedit_typed_in_the_block_editor_asks_to_save_first() {
    let mut s = Session::new();
    door(&mut s);
    let l = line(&mut s, [0.0, 50.0], [5.0, 50.0]);
    s.execute("block", &json!({ "name": "Frame", "base": [0, 50], "handles": [l] })).unwrap();
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();

    // Nothing changed: it switches at once.
    s.script(
        "BEDIT
Frame
",
    )
    .unwrap();
    assert!(s.running.is_none());
    assert_eq!(editor(&s).as_deref(), Some("Frame"));

    line(&mut s, [1.0, 1.0], [2.0, 2.0]);
    s.script(
        "BEDIT
Door
",
    )
    .unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Save changes to block \"Frame\"?");
    // Escape keeps the editor on Frame with its change.
    s.input(cadcraft_engine::Input::Cancel).unwrap();
    assert_eq!(editor(&s).as_deref(), Some("Frame"));
    assert!(s.state().unwrap().block_unsaved());

    s.script(
        "BEDIT
Door
Y
",
    )
    .unwrap();
    assert!(s.running.is_none());
    assert_eq!(editor(&s).as_deref(), Some("Door"));
    assert_eq!(lines_of(&s, "Frame").len(), 2, "saved before switching");

    line(&mut s, [3.0, 3.0], [4.0, 4.0]);
    s.script(
        "BEDIT
Frame
N
",
    )
    .unwrap();
    assert_eq!(editor(&s).as_deref(), Some("Frame"));
    assert_eq!(lines_of(&s, "Door").len(), 1, "discarded");
}
