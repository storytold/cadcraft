//! OOPS, as AutoCAD 2024 runs it (accoreconsole: a line A, a line B, a circle C and a line D, these
//! scripts, then the drawing's `entnext` order read back).

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{EntityCodec, set_entity_codec};
use cadcraft_engine::doc::{Common, EntityKind, Handle, Line, Space};
use cadcraft_engine::geom::Vec3;
use serde_json::{Value, json};

/// A session that `entmake` works in.
fn session() -> Session {
    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    Session::new()
}

/// A session with A, B, C and D in draw order, and their handles.
fn drawing() -> (Session, Vec<Handle>) {
    let mut s = session();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.execute("line", &json!({ "points": [[0, 10], [10, 10]] })).unwrap();
    s.execute("circle", &json!({ "center": [5, 25], "radius": 3 })).unwrap();
    s.execute("line", &json!({ "points": [[0, 40], [10, 40]] })).unwrap();
    let hs = handles(&s);
    (s, hs)
}

fn handles(s: &Session) -> Vec<Handle> {
    s.doc().unwrap().model.iter().map(|e| e.handle).collect()
}

/// The drawing's objects in draw order: A to D by their handles, E for any other.
fn order(s: &Session, hs: &[Handle]) -> String {
    let name = |h: &Handle| hs.iter().position(|x| x == h).map_or('E', |i| (b'A' + i as u8) as char);
    handles(s).iter().map(|h| name(h).to_string()).collect::<Vec<_>>().join(" ")
}

/// Run `script` with `@A`…`@D` standing for the objects.
fn run(s: &mut Session, hs: &[Handle], script: &[&str]) {
    for line in script {
        let mut line = line.to_string();
        for (i, h) in hs.iter().enumerate() {
            line = line.replace(&format!("@{}", (b'A' + i as u8) as char), &format!("(handent \"{}\")", h.hex()));
        }
        s.cmdline(&line).unwrap();
        assert!(s.running.is_none(), "{line}: {}", s.prompt_text());
    }
}

fn oops(s: &mut Session) -> Value {
    s.execute("oops", &json!({})).unwrap()
}

#[test]
fn the_last_erase_comes_back_where_it_was() {
    const LINE: &str = "(command \"_.LINE\" '(0.0 50.0) '(10.0 50.0) \"\")";
    const OOPS: &str = "(command \"_.OOPS\")";
    // (case, script, the drawing's order afterwards: E is the line drawn after the erase)
    let cases: [(&str, &[&str], &str); 14] = [
        ("OOPS-000: nothing erased yet", &[OOPS], "A B C D"),
        ("OOPS-001: two in the middle", &["(command \"_.ERASE\" @B @C \"\")", OOPS], "A B C D"),
        ("OOPS-002: picked in reverse", &["(command \"_.ERASE\" @C @B \"\")", OOPS], "A B C D"),
        ("OOPS-003: a new object after the erase", &["(command \"_.ERASE\" @B \"\")", LINE, OOPS], "A B C D E"),
        ("OOPS-004: the bottom object", &["(command \"_.ERASE\" @A \"\")", LINE, OOPS], "A B C D E"),
        ("OOPS-005: twice", &["(command \"_.ERASE\" @B \"\")", OOPS, OOPS], "A B C D"),
        ("OOPS-006: after U", &["(command \"_.ERASE\" @B \"\")", "U", OOPS], "A B C D"),
        ("OOPS-007: the last erase only", &["(command \"_.ERASE\" @B \"\")", "(command \"_.ERASE\" @C \"\")", OOPS], "A C D"),
        ("OOPS-008: U after OOPS", &["(command \"_.ERASE\" @B \"\")", OOPS, "U"], "A C D"),
        ("OOPS-009: entdel", &["(entdel @B)", OOPS], "A C D"),
        ("OOPS-010: -BLOCK", &["(command \"_.-BLOCK\" \"BKO\" '(0.0 0.0) @A @C \"\")", OOPS], "A B C D"),
        ("OOPS-011: U of another command", &["(command \"_.ERASE\" @B \"\")", LINE, "U", OOPS], "A B C D"),
        ("OOPS-012: the object under it erased since", &["(command \"_.ERASE\" @B \"\")", "(entdel @A)", OOPS], "B C D"),
        ("OOPS-013: everything", &["(command \"_.ERASE\" \"_ALL\" \"\")", OOPS], "A B C D"),
    ];
    for (case, script, want) in cases {
        let (mut s, hs) = drawing();
        run(&mut s, &hs, script);
        assert_eq!(order(&s, &hs), want, "{case}");
    }
}

#[test]
fn oops_says_nothing_and_counts_what_came_back() {
    let (mut s, hs) = drawing();
    s.cmdline("oops").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("Command: OOPS"));
    s.execute("erase", &json!({ "handles": [hs[1].hex(), hs[3].hex()] })).unwrap();
    s.cmdline("oops").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("Command: OOPS"));
    assert_eq!(order(&s, &hs), "A B C D");
    assert_eq!(oops(&mut s), json!({ "restored": 0 }));
    s.execute("u", &json!({})).unwrap();
    assert_eq!(order(&s, &hs), "A C");
    assert_eq!(oops(&mut s), json!({ "restored": 2 }));
    // They come back as they were.
    let d = s.doc().unwrap();
    assert!(matches!(d.entity(hs[3]).map(|e| &e.kind), Some(EntityKind::Line(l)) if l.a.y == 40.0 && l.b.x == 10.0));
}

#[test]
fn the_delete_key_and_block_feed_oops_too() {
    // Edit > Clear, the Delete key.
    let (mut s, hs) = drawing();
    s.set_selection(vec![hs[2]]);
    s.execute("erase.selection", &json!({})).unwrap();
    assert_eq!(order(&s, &hs), "A B D");
    assert_eq!(oops(&mut s), json!({ "restored": 1 }));
    assert_eq!(order(&s, &hs), "A B C D");
    // BLOCK deleting its objects.
    let (mut s, hs) = drawing();
    s.execute("block", &json!({ "name": "BK", "base": [0, 0], "handles": [hs[0].hex(), hs[3].hex()], "keep": "delete" })).unwrap();
    assert_eq!(order(&s, &hs), "B C");
    assert_eq!(oops(&mut s), json!({ "restored": 2 }));
    assert_eq!(order(&s, &hs), "A B C D");
}

#[test]
fn an_object_an_undo_took_away_stays_away() {
    // U past the line's creation: its handle is free again.
    let mut s = session();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let x = handles(&s)[0];
    s.execute("erase", &json!({ "handles": [x.hex()] })).unwrap();
    s.execute("u", &json!({})).unwrap();
    s.execute("u", &json!({})).unwrap();
    assert!(handles(&s).is_empty());
    assert_eq!(oops(&mut s), json!({ "restored": 0 }));
    assert!(handles(&s).is_empty());
    // A new object takes the handle.
    s.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap();
    assert_eq!(handles(&s), [x]);
    assert_eq!(oops(&mut s), json!({ "restored": 0 }));
    assert!(matches!(s.doc().unwrap().entity(x).map(|e| &e.kind), Some(EntityKind::Circle(_))));
    // A block's object takes the handle: one erased object of two, one of a block's two.
    for (erased, in_block) in [(2, 1), (1, 2)] {
        let mut s = session();
        for y in 0..erased {
            s.execute("line", &json!({ "points": [[0, y], [10, y]] })).unwrap();
        }
        let hs = handles(&s);
        s.execute("erase", &json!({ "handles": hs.iter().map(|h| h.hex()).collect::<Vec<_>>() })).unwrap();
        for _ in 0..=erased {
            s.execute("u", &json!({})).unwrap();
        }
        assert!(handles(&s).is_empty());
        s.cmdline("(entmake '((0 . \"BLOCK\") (2 . \"BX\") (10 0.0 0.0 0.0) (70 . 0)))").unwrap();
        for _ in 0..in_block {
            s.cmdline("(entmake '((0 . \"LINE\") (8 . \"0\") (10 0.0 0.0 0.0) (11 1.0 0.0 0.0)))").unwrap();
        }
        s.cmdline("(entmake '((0 . \"ENDBLK\")))").unwrap();
        assert!(s.doc().unwrap().block("BX").is_some_and(|b| b.entities.contains(hs[0])), "{erased} {in_block}");
        assert_eq!(oops(&mut s), json!({ "restored": 0 }), "{erased} {in_block}");
        assert!(handles(&s).is_empty());
    }
}

#[test]
fn an_object_whose_layer_or_block_is_gone_stays_away() {
    // Its layer purged.
    let (mut s, hs) = drawing();
    run(&mut s, &hs, &["(command \"_.-LAYER\" \"_M\" \"L1\" \"\")"]);
    s.execute("line", &json!({ "points": [[0, 60], [10, 60]] })).unwrap();
    let on_l1 = *handles(&s).last().unwrap();
    run(&mut s, &hs, &["(setvar \"CLAYER\" \"0\")"]);
    s.execute("erase", &json!({ "handles": [on_l1.hex()] })).unwrap();
    s.execute("purge", &json!({ "type": "layers", "names": "L1" })).unwrap();
    assert!(s.doc().unwrap().layer("L1").is_none());
    assert_eq!(oops(&mut s), json!({ "restored": 0 }));
    // Its block purged.
    let (mut s, hs) = drawing();
    s.execute("block", &json!({ "name": "BK", "base": [0, 0], "handles": [hs[0].hex()], "keep": "retain" })).unwrap();
    s.execute("insert", &json!({ "name": "BK", "at": [50, 0] })).unwrap();
    let ins = *handles(&s).last().unwrap();
    s.execute("erase", &json!({ "handles": [ins.hex()] })).unwrap();
    s.execute("purge", &json!({ "type": "blocks", "names": "BK" })).unwrap();
    assert!(s.doc().unwrap().block("BK").is_none());
    assert_eq!(oops(&mut s), json!({ "restored": 0 }));
    assert_eq!(order(&s, &hs), "A B C D");
}

#[test]
fn the_block_editor_has_an_oops_of_its_own() {
    let (mut s, hs) = drawing();
    s.execute("block", &json!({ "name": "BK", "base": [0, 0], "handles": [hs[0].hex()], "keep": "retain" })).unwrap();
    s.execute("erase", &json!({ "handles": [hs[1].hex()] })).unwrap();
    s.execute("bedit", &json!({ "name": "BK" })).unwrap();
    // The drawing's erased line doesn't come back into the block.
    assert_eq!(oops(&mut s), json!({ "restored": 0 }));
    s.execute("bclose", &json!({ "save": false })).unwrap();
    // Back in the drawing, it does.
    assert_eq!(oops(&mut s), json!({ "restored": 1 }));
    assert_eq!(order(&s, &hs), "A B C D");
}

#[test]
fn a_layouts_objects_come_back_to_it() {
    let mut s = session();
    let paper = Space::Paper("Layout1".into());
    let line = |y: f64| EntityKind::Line(Line { a: Vec3::new(0.0, y, 0.0), b: Vec3::new(10.0, y, 0.0) });
    let hs: Vec<Handle> = (0..3).map(|y| s.doc_mut().unwrap().add(&paper, Common::default(), line(f64::from(y))).unwrap()).collect();
    s.execute("erase", &json!({ "handles": [hs[1].hex()] })).unwrap();
    assert_eq!(oops(&mut s), json!({ "restored": 1 }));
    let d = s.doc().unwrap();
    assert!(d.model.is_empty());
    assert_eq!(d.space(&paper).unwrap().handles(), hs);
}

#[test]
fn a_reference_edited_in_place_keeps_its_objects_and_the_drawings_apart() {
    let (mut s, hs) = drawing();
    s.execute("block", &json!({ "name": "BK", "base": [0, 0], "handles": [hs[0].hex(), hs[1].hex()], "keep": "convert" })).unwrap();
    let ins = *handles(&s).last().unwrap();
    s.execute("erase", &json!({ "handles": [hs[3].hex()] })).unwrap();
    let r = s.execute("refedit", &json!({ "handle": ins.hex() })).unwrap();
    // The drawing's line stays out while the reference is edited...
    assert_eq!(oops(&mut s), json!({ "restored": 0 }));
    // ...and a working copy comes back.
    let copy = r["working"][0].as_str().unwrap().to_string();
    s.execute("erase", &json!({ "handles": [copy] })).unwrap();
    assert_eq!(oops(&mut s), json!({ "restored": 1 }));
    // Once the edit is over, its copies don't come back into the drawing.
    s.execute("erase", &json!({ "handles": [copy] })).unwrap();
    s.execute("refclose", &json!({ "save": true })).unwrap();
    assert_eq!(s.doc().unwrap().block("BK").unwrap().entities.len(), 1);
    assert_eq!(oops(&mut s), json!({ "restored": 0 }));
    assert_eq!(order(&s, &hs), "C E");
    // The drawing's own erased line does, after an edit that ends.
    s.execute("erase", &json!({ "handles": [hs[2].hex()] })).unwrap();
    s.execute("refedit", &json!({ "handle": ins.hex() })).unwrap();
    s.execute("refclose", &json!({ "save": false })).unwrap();
    assert_eq!(oops(&mut s), json!({ "restored": 1 }));
    assert_eq!(order(&s, &hs), "C E");
}
