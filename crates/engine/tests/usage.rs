//! USAGE: a read-only cross-reference of the layers, blocks, text styles and linetypes a drawing
//! uses (#1439), built on PURGE's usage scan.

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Insert, Layer, Line, Linetype, Text, TextStyle};
use cadcraft_engine::geom::Vec3;
use serde_json::{Value, json};

/// A LINE on `layer` with an explicit `linetype`, ten units long so it is never degenerate.
fn line(handle: u64, layer: &str, linetype: &str) -> Entity {
    let mut e = Entity::new(Handle(handle), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) }));
    e.common = Common { layer: layer.into(), linetype: linetype.into(), ..Common::default() };
    e
}

/// A TEXT reading NOTE in `style` on `layer`, so the text-style table has a visible user.
fn text(handle: u64, layer: &str, style: &str) -> Entity {
    let mut e = Entity::new(
        Handle(handle),
        EntityKind::Text(Text {
            insert: Vec3::ZERO,
            align_pt: None,
            height: 1.0,
            value: "NOTE".into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: style.into(),
            halign: Default::default(),
            valign: Default::default(),
        }),
    );
    e.common = Common { layer: layer.into(), ..Common::default() };
    e
}

/// An INSERT of `block` on `layer`, at unit scale and without attributes.
fn insert(handle: u64, block: &str, layer: &str) -> Entity {
    let mut e = Entity::new(
        Handle(handle),
        EntityKind::Insert(Insert {
            block: block.into(),
            insert: Vec3::ZERO,
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        }),
    );
    e.common = Common { layer: layer.into(), ..Common::default() };
    e
}

/// A block definition named `name` holding `entities`, shared the way the drawing's block table
/// stores it.
fn block(name: &str, entities: Vec<Entity>) -> Arc<Block> {
    let mut b = Block::new(name);
    for e in entities {
        b.entities.push(e);
    }
    Arc::new(b)
}

/// Layers 0, Defpoints, Used, Empty and InBlock; linetypes Continuous, DASHED and SPARE; text
/// styles Standard, InBlockStyle and UnusedStyle; blocks WALLS (inserted) holding a line on
/// InBlock and a text in InBlockStyle and an insert of INNER, INNER (used only inside WALLS) and
/// the unreferenced OLD. Model space: a DASHED line and a text on Used, and WALLS.
fn drawing() -> Session {
    let mut s = Session::new();
    let d = s.doc_mut().unwrap();
    d.layers.push(Layer::new("Used"));
    d.layers.push(Layer::new("Empty"));
    d.layers.push(Layer::new("InBlock"));
    for (name, dashes) in [("DASHED", [0.5, 0.25]), ("SPARE", [0.1, 0.1])] {
        d.linetypes.push(Linetype::simple(name, "", &dashes));
    }
    d.text_styles.push(TextStyle { name: "InBlockStyle".into(), ..TextStyle::default() });
    d.text_styles.push(TextStyle { name: "UnusedStyle".into(), ..TextStyle::default() });
    d.blocks.insert(
        "WALLS".into(),
        block("WALLS", vec![line(0x3000, "InBlock", "ByLayer"), text(0x3001, "InBlock", "InBlockStyle"), insert(0x3002, "INNER", "0")]),
    );
    d.blocks.insert("INNER".into(), block("INNER", vec![line(0x3010, "0", "ByLayer")]));
    d.blocks.insert("OLD".into(), block("OLD", vec![line(0x3020, "0", "ByLayer")]));
    d.model.push(line(0x3100, "Used", "DASHED"));
    d.model.push(text(0x3101, "Used", "Standard"));
    d.model.push(insert(0x3102, "WALLS", "0"));
    s
}

/// Runs the `usage` command with `p` and unwraps the result, so a rejected call fails the test.
fn usage(s: &mut Session, p: Value) -> Value {
    s.execute("usage", &p).unwrap()
}

/// The report item of kind `ty` named `name`, failing with the whole item list when it is absent.
fn item<'a>(r: &'a Value, ty: &str, name: &str) -> &'a Value {
    r["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["type"] == ty && i["name"] == name)
        .unwrap_or_else(|| panic!("no {ty} \"{name}\" in {}", r["items"]))
}

/// The `unused` names of kind `kind` as plain strings, in the order USAGE returned them.
fn names(r: &Value, kind: &str) -> Vec<String> {
    r["unused"][kind]["names"].as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_string()).collect()
}

/// Guards that each layer separates uses in model space and the layouts from uses inside block
/// definitions, that Defpoints is empty yet kept by PURGE, and that the current layer is never
/// offered as unused.
#[test]
fn layers_report_uses_and_where() {
    let mut s = drawing();
    let r = usage(&mut s, json!({}));
    assert_eq!(r["schema"], 1);

    // A line and a text in model space.
    let used = item(&r, "layer", "Used");
    assert_eq!((used["entities"].clone(), used["blockEntities"].clone()), (json!(2), json!(0)));
    assert_eq!((used["used"].clone(), used["insideBlocksOnly"].clone()), (json!(true), json!(false)));
    assert_eq!((used["empty"].clone(), used["unused"].clone()), (json!(false), json!(false)));
    assert_eq!(used["current"], false);

    // Only a line inside the inserted block WALLS.
    let inblock = item(&r, "layer", "InBlock");
    assert_eq!((inblock["entities"].clone(), inblock["blockEntities"].clone()), (json!(0), json!(2)));
    assert_eq!((inblock["used"].clone(), inblock["insideBlocksOnly"].clone()), (json!(true), json!(true)));

    // Empty and purgeable.
    let empty = item(&r, "layer", "Empty");
    assert_eq!((empty["used"].clone(), empty["empty"].clone()), (json!(false), json!(true)));
    assert_eq!(empty["unused"], true);

    // Defpoints is empty but PURGE keeps it.
    let defpoints = item(&r, "layer", "Defpoints");
    assert_eq!((defpoints["empty"].clone(), defpoints["unused"].clone()), (json!(true), json!(false)));

    // The current layer is not offered as unused even with nothing on it.
    let current = &r["items"].as_array().unwrap()[0];
    assert_eq!(current["name"], "0");
    assert_eq!((current["used"].clone(), current["unused"].clone(), current["current"].clone()), (json!(true), json!(false), json!(true)));
    assert_eq!(names(&r, "layers"), ["Empty"]);
}

/// Guards that a block's `used` follows reachability from model space — INNER, referenced only
/// by the inserted WALLS, is used but inside blocks only — and that an unreferenced definition
/// such as OLD is empty and purgeable.
#[test]
fn blocks_report_references_and_reachability() {
    let mut s = drawing();
    let r = usage(&mut s, json!({}));

    let walls = item(&r, "block", "WALLS");
    assert_eq!((walls["entities"].clone(), walls["blockEntities"].clone()), (json!(1), json!(0)));
    assert_eq!((walls["used"].clone(), walls["insideBlocksOnly"].clone()), (json!(true), json!(false)));

    // Referenced by WALLS only, which model space inserts.
    let inner = item(&r, "block", "INNER");
    assert_eq!((inner["entities"].clone(), inner["blockEntities"].clone()), (json!(0), json!(1)));
    assert_eq!((inner["used"].clone(), inner["insideBlocksOnly"].clone()), (json!(true), json!(true)));

    let old = item(&r, "block", "OLD");
    assert_eq!((old["used"].clone(), old["empty"].clone(), old["unused"].clone()), (json!(false), json!(true), json!(true)));
    assert_eq!(names(&r, "blocks"), ["OLD"]);
    assert_eq!(r["summary"]["blocks"]["unused"], 1);
}

/// Guards that text styles count the entities that use them and the dimension/multileader styles
/// that name them, that ByLayer and ByBlock are values rather than uses of the linetype table,
/// and that a layer's linetype counts as a reference to it.
#[test]
fn styles_and_linetypes_report_uses() {
    let mut s = drawing();
    let r = usage(&mut s, json!({}));

    let standard = item(&r, "textStyle", "Standard");
    assert_eq!((standard["entities"].clone(), standard["blockEntities"].clone()), (json!(1), json!(0)));
    assert_eq!((standard["used"].clone(), standard["unused"].clone()), (json!(true), json!(false)));

    let inblock = item(&r, "textStyle", "InBlockStyle");
    assert_eq!((inblock["entities"].clone(), inblock["blockEntities"].clone()), (json!(0), json!(1)));
    assert_eq!((inblock["used"].clone(), inblock["insideBlocksOnly"].clone()), (json!(true), json!(true)));

    let unused = item(&r, "textStyle", "UnusedStyle");
    assert_eq!((unused["used"].clone(), unused["empty"].clone(), unused["unused"].clone()), (json!(false), json!(true), json!(true)));
    assert_eq!(names(&r, "textStyles"), ["UnusedStyle"]);

    // An entity's explicit linetype.
    let dashed = item(&r, "linetype", "DASHED");
    assert_eq!((dashed["entities"].clone(), dashed["blockEntities"].clone()), (json!(1), json!(0)));
    assert_eq!((dashed["used"].clone(), dashed["unused"].clone()), (json!(true), json!(false)));

    // ByLayer and ByBlock are values, not uses of the linetype table's entries.
    for special in ["ByLayer", "ByBlock"] {
        let it = item(&r, "linetype", special);
        assert_eq!((it["entities"].clone(), it["blockEntities"].clone()), (json!(0), json!(0)), "{special}");
        assert_eq!(it["unused"], false, "{special}");
    }

    let spare = item(&r, "linetype", "SPARE");
    assert_eq!((spare["used"].clone(), spare["unused"].clone()), (json!(false), json!(true)));
    assert_eq!(names(&r, "linetypes"), ["SPARE"]);

    // A layer's linetype is a reference to it; every layer here is Continuous.
    let continuous = item(&r, "linetype", "Continuous");
    assert_eq!((continuous["entities"].clone(), continuous["references"].clone()), (json!(0), json!(5)));
    assert_eq!((continuous["used"].clone(), continuous["unused"].clone()), (json!(true), json!(false)));
}

/// Guards the promise that USAGE is read-only: the drawing is not copied and its uid, revision,
/// dirty flag, undo stack and tables are exactly as they were.
#[test]
fn nothing_is_modified() {
    let mut s = drawing();
    let before = s.state().unwrap().doc.clone();
    let (uid, revision) = (s.state().unwrap().uid, s.state().unwrap().revision);
    let dirty = s.state().unwrap().is_dirty();
    let tables = |d: &Drawing| {
        (
            d.layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
            d.linetypes.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
            d.text_styles.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
            d.blocks.keys().cloned().collect::<Vec<_>>(),
            d.entity_count(),
        )
    };
    let before_tables = tables(&before);

    let _ = usage(&mut s, json!({}));
    let st = s.state().unwrap();
    assert!(Arc::ptr_eq(&before, &st.doc), "USAGE copied the drawing");
    assert_eq!((st.uid, st.revision), (uid, revision));
    assert_eq!(tables(&st.doc), before_tables);
    assert!(st.undo.is_empty(), "USAGE pushed an undo step");
    assert_eq!(st.is_dirty(), dirty);
}

/// Guards the paging envelope (`count`, `returned`, `offset`, `truncated`), the `type` filter and
/// that a bad `type`, `limit` or `offset` is reported as an error rather than panicking.
#[test]
fn paging_and_filters() {
    let mut s = drawing();
    let all = usage(&mut s, json!({}));
    let total = all["count"].as_u64().unwrap();
    assert_eq!(total as usize, all["items"].as_array().unwrap().len());
    assert_eq!((all["returned"].clone(), all["truncated"].clone()), (json!(total), json!(false)));
    assert_eq!(all["offset"], 0);

    let page = usage(&mut s, json!({ "limit": 2 }));
    assert_eq!((page["count"].clone(), page["returned"].clone(), page["truncated"].clone()), (json!(total), json!(2), json!(true)));
    let offset = usage(&mut s, json!({ "offset": 2, "limit": 2 }));
    assert_eq!((offset["returned"].clone(), offset["truncated"].clone()), (json!(2), json!(total > 4)));

    let layers = usage(&mut s, json!({ "type": "layers" }));
    assert_eq!(layers["count"], 5);
    assert!(layers["items"].as_array().unwrap().iter().all(|i| i["type"] == "layer"));
    let styles = usage(&mut s, json!({ "type": "textstyles" }));
    assert_eq!(styles["count"], 3);
    let none = usage(&mut s, json!({ "type": "blocks", "offset": 100 }));
    assert_eq!((none["returned"].clone(), none["truncated"].clone()), (json!(0), json!(false)));

    assert!(s.execute("usage", &json!({ "type": "nope" })).is_err());
    assert!(s.execute("usage", &json!({ "limit": "x" })).is_err());
    assert!(s.execute("usage", &json!({ "offset": -1 })).is_err());
}

/// Guards that a drawing within the scan limit reports itself complete: `incomplete` is a real
/// signal, false here and with no reason, so callers can tell complete counts from partial ones.
#[test]
fn a_complete_scan_is_marked_complete() {
    let mut s = drawing();
    let r = usage(&mut s, json!({}));
    assert_eq!(r["incomplete"], false);
    assert!(r.get("incompleteReason").is_none(), "a complete scan has no reason: {r}");
    assert!(!r["message"].as_str().unwrap().contains("incomplete"), "{}", r["message"]);
}
