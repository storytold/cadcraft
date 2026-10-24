//! BLOCKREPLACE (Express Tools): every reference of one block, in model space, the layouts and
//! other blocks, becomes a reference of another at the same insertion point, scale and rotation.

use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Block, Common, Entity, EntityKind, Handle, Insert, Space};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

/// Make block `name` from what is in model space now (the objects are removed).
fn make_block(s: &mut Session, name: &str) {
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": name, "base": [0, 0], "handles": hs, "keep": "delete"})).unwrap();
}

/// Block A: a circle and attribute TAG. Block B: a line, attributes TAG and NEW. Block N holds a
/// reference of A. Model space: a reference of A on layer R at (10, 5), scaled 2 and rotated
/// 30°, with TAG "kept", and a reference of N. Layout1: a reference of A.
fn drawing() -> (Session, Handle) {
    let mut s = Session::new();
    s.execute("attdef", &json!({"tag": "TAG", "default": "a", "at": [0, 2], "height": 1.0})).unwrap();
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    make_block(&mut s, "A");
    s.execute("attdef", &json!({"tag": "TAG", "default": "b", "at": [0, 3], "height": 1.0})).unwrap();
    s.execute("attdef", &json!({"tag": "NEW", "default": "new", "at": [0, 5], "height": 1.0})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    make_block(&mut s, "B");
    s.execute("insert", &json!({"name": "A", "at": [0, 0]})).unwrap();
    make_block(&mut s, "N");
    s.execute("insert", &json!({"name": "N", "at": [50, 0]})).unwrap();
    s.execute("layer.new", &json!({"name": "R", "current": true})).unwrap();
    let r = s.execute("insert", &json!({"name": "A", "at": [10, 5], "scale": 2, "rotation": 30, "attribs": {"TAG": "kept"}})).unwrap();
    let ins = Insert {
        block: "A".into(),
        insert: Vec3::new(0.0, 0.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    s.doc_mut().unwrap().add(&Space::Paper("Layout1".into()), Common::default(), EntityKind::Insert(ins)).unwrap();
    (s, Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap())
}

/// The block names of the references in `ents`.
fn refs<'a>(ents: impl Iterator<Item = &'a std::sync::Arc<Entity>>) -> Vec<String> {
    ents.filter_map(|e| match &e.kind {
        EntityKind::Insert(i) => Some(i.block.clone()),
        _ => None,
    })
    .collect()
}

fn insert(s: &Session, h: Handle) -> (Insert, String) {
    let e = s.doc().unwrap().entity(h).unwrap();
    let EntityKind::Insert(i) = &e.kind else { panic!("not a block reference") };
    (i.clone(), e.common.layer.clone())
}

#[test]
fn blockreplace_replaces_every_reference_and_keeps_its_placement() {
    let (mut s, h) = drawing();
    let before = insert(&s, h).0;
    let r = s.execute("blockreplace", &json!({"block": "a", "with": "b"})).unwrap();
    assert_eq!(r, json!({"block": "A", "with": "B", "replaced": 3, "purged": false}));

    let (after, layer) = insert(&s, h);
    assert_eq!(after.block, "B");
    assert_eq!((after.insert, after.scale, after.rotation, layer.as_str()), (before.insert, before.scale, before.rotation, "R"));
    // The replacement's attributes, with the value of the tag both blocks define.
    let values: Vec<(String, String)> = after.attribs.iter().map(|a| (a.tag.clone(), a.text.value.clone())).collect();
    assert_eq!(values, [("TAG".to_string(), "kept".to_string()), ("NEW".to_string(), "new".to_string())]);

    let d = s.doc().unwrap();
    assert_eq!(refs(d.model.iter()), ["N", "B"]);
    assert_eq!(refs(d.block("N").unwrap().entities.iter()), ["B"]);
    assert_eq!(refs(d.space(&Space::Paper("Layout1".into())).unwrap().iter()), ["B"]);
    assert!(d.block("A").is_some(), "the definition stays without `purge`");

    s.execute("undo", &json!({})).unwrap();
    assert_eq!(insert(&s, h).0, before);
}

#[test]
fn blockreplace_references_follow_the_replacement_blocks_annotative_setting() {
    let (mut s, h) = drawing();
    let c = s.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap()["handle"].clone();
    s.execute("block", &json!({"name": "AN", "base": [0, 0], "handles": [c], "keep": "delete", "annotative": true})).unwrap();
    let scales = |s: &Session| s.doc().unwrap().entity(h).unwrap().common.anno_scales.len();
    assert_eq!(scales(&s), 0);
    s.execute("blockreplace", &json!({"block": "A", "with": "AN"})).unwrap();
    assert_eq!(scales(&s), 1, "a reference of an annotative block supports the current scale");
    s.execute("blockreplace", &json!({"block": "AN", "with": "B"})).unwrap();
    assert_eq!(scales(&s), 0);
}

#[test]
fn blockreplace_purges_the_replaced_definition_on_request() {
    let (mut s, _) = drawing();
    let r = s.execute("blockreplace", &json!({"block": "A", "with": "B", "purge": true})).unwrap();
    assert_eq!(r["purged"], json!(true));
    assert!(s.doc().unwrap().block("A").is_none());
}

#[test]
fn blockreplace_keeps_a_definition_an_unused_block_still_names() {
    let (mut s, _) = drawing();
    // Block U, never inserted, holds a multileader whose content is block A.
    let m = s.execute("mleader", &json!({"points": [[0, 0], [10, 5]], "block": "A"})).unwrap()["handle"].clone();
    s.execute("block", &json!({"name": "U", "base": [0, 0], "handles": [m], "keep": "delete"})).unwrap();
    let r = s.execute("blockreplace", &json!({"block": "A", "with": "B", "purge": true})).unwrap();
    assert_eq!((r["replaced"].clone(), r["purged"].clone()), (json!(3), json!(false)));
    assert!(s.doc().unwrap().block("A").is_some());
}

#[test]
fn blockreplace_refuses_what_it_cannot_replace() {
    let (mut s, _) = drawing();
    let err = |s: &mut Session, p: Value| s.execute("blockreplace", &p).unwrap_err().to_string();
    assert!(err(&mut s, json!({"block": "A", "with": "a"})).contains("different block"));
    assert!(err(&mut s, json!({"block": "A", "with": "MISSING"})).contains("not found"));
    assert!(err(&mut s, json!({"block": "MISSING", "with": "B"})).contains("not found"));
    // N holds a reference of A, so N replacing A would reference itself.
    assert!(err(&mut s, json!({"block": "A", "with": "N"})).contains("references"));
    // M holds a multileader whose content is N, so M reaches A too.
    let m = s.execute("mleader", &json!({"points": [[0, 0], [10, 5]], "block": "N"})).unwrap()["handle"].clone();
    s.execute("block", &json!({"name": "M", "base": [0, 0], "handles": [m], "keep": "delete"})).unwrap();
    assert!(err(&mut s, json!({"block": "A", "with": "M"})).contains("references"));
    assert!(err(&mut s, json!({"block": "A", "with": "B", "purge": "yes"})).contains("`purge`"));
    assert!(err(&mut s, json!({"with": "B"})).contains("`block`"));
    // A block an xref brings in is not saved with the drawing.
    s.doc_mut().unwrap().blocks.insert("X|A".into(), std::sync::Arc::new(Block::new("X|A")));
    assert!(err(&mut s, json!({"block": "X|A", "with": "B"})).contains("xref"));
    assert!(err(&mut s, json!({"block": "A", "with": "X|A"})).contains("xref"));
    assert_eq!(refs(s.doc().unwrap().model.iter()), ["N", "A"]);
    // The Block Editor shows the block, not the drawing's references.
    s.execute("bedit", &json!({"name": "B"})).unwrap();
    assert!(err(&mut s, json!({"block": "A", "with": "B"})).contains("Block Editor"));
}

#[test]
fn blockreplace_asks_for_both_blocks_and_whether_to_purge() {
    let (mut s, h) = drawing();
    s.cmdline("-blockreplace").unwrap();
    assert!(s.prompt_text().contains("Enter the name of the block to replace or [?/Select]"), "{}", s.prompt_text());
    s.cmdline("?").unwrap();
    assert!(s.log.iter().any(|l| l == "Blocks: A, B, N"), "{:?}", s.log);
    s.cmdline("a").unwrap();
    assert!(s.prompt_text().contains("Enter the name of the replacement block for A or [?/Select]"), "{}", s.prompt_text());
    // A cannot replace itself: the prompt asks again.
    s.cmdline("A").unwrap();
    assert!(s.prompt_text().contains("replacement block for A"), "{}", s.prompt_text());
    s.input(Input::Cancel).unwrap();
    assert!(s.running.is_none());
    assert_eq!(refs(s.doc().unwrap().model.iter()), ["N", "A"]);

    // The block to replace picked from one of its references; Enter purges it.
    s.cmdline("blockreplace").unwrap();
    s.cmdline("Select").unwrap();
    s.input(Input::Pick(vec![h])).unwrap();
    assert!(s.prompt_text().contains("replacement block for A"), "{}", s.prompt_text());
    s.cmdline("B").unwrap();
    assert!(s.prompt_text().contains("Purge the unreferenced block definition A? [Yes/No] <Yes>"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert!(s.log.iter().any(|l| l == "3 references of block A replaced with B. Block A purged."), "{:?}", s.log);
    let d = s.doc().unwrap();
    assert!(d.block("A").is_none());
    assert_eq!(refs(d.model.iter()), ["N", "B"]);
    assert_eq!(insert(&s, h).0.block, "B");
}

#[test]
fn blockreplace_no_keeps_the_replaced_definition() {
    let (mut s, _) = drawing();
    s.cmdline("blockreplace").unwrap();
    s.cmdline("A").unwrap();
    s.cmdline("B").unwrap();
    s.cmdline("No").unwrap();
    assert!(s.running.is_none());
    let d = s.doc().unwrap();
    assert!(d.block("A").is_some());
    assert_eq!(refs(d.model.iter()), ["N", "B"]);
}
