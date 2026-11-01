//! PURGE removes multiline styles no multiline uses (#626): not STANDARD, not the current
//! style, not one used by a multiline in model space or in a block that is still inserted.

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Attrib, Block, Common, Entity, EntityKind, Handle, Insert, MLine, MLineElement, MLineJust, MLineStyle, Space};
use cadcraft_engine::geom::{Vec2, Vec3};
use serde_json::json;

fn mline(style: &MLineStyle) -> EntityKind {
    EntityKind::MLine(MLine::new(style, &[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)], false, MLineJust::Zero, 1.0))
}

fn block(name: &str, style: &MLineStyle, handle: u64) -> Arc<Block> {
    let mut b = Block::new(name);
    b.entities.push(Entity::new(Handle(handle), mline(style)));
    Arc::new(b)
}

/// Styles: Model (used in model space), InBlock (used in the inserted block WALLS), Gone (used
/// only in the unreferenced block OLD), Spare (unused), Current (unused but current).
fn session() -> Session {
    let mut s = Session::new();
    let d = s.doc_mut().unwrap();
    let style = |n: &str| MLineStyle { name: n.into(), ..MLineStyle::default() };
    for n in ["Model", "InBlock", "Gone", "Spare", "Current"] {
        d.mline_styles.push(style(n));
    }
    d.header.set_str("CMLSTYLE", "Current");
    d.add(&Space::Model, Common::default(), mline(&style("Model"))).unwrap();
    d.blocks.insert("WALLS".into(), block("WALLS", &style("InBlock"), 0x9000));
    d.blocks.insert("OLD".into(), block("OLD", &style("Gone"), 0x9001));
    let ins = Insert {
        block: "WALLS".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
    s
}

fn styles(s: &Session) -> Vec<String> {
    s.doc().unwrap().mline_styles.iter().map(|m| m.name.clone()).collect()
}

#[test]
fn purge_mlinestyles_keeps_used_standard_and_current() {
    let mut s = session();
    let r = s.execute("purge", &json!({ "type": "mlinestyles" })).unwrap();
    assert_eq!(styles(&s), ["STANDARD", "Model", "InBlock", "Gone", "Current"], "{r}");
    assert_eq!(r["purged"]["mlinestyles"], json!(["Spare"]));
}

#[test]
fn purge_all_frees_styles_of_purged_blocks() {
    let mut s = session();
    s.script("-PURGE A * N\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(s.doc().unwrap().block("OLD").is_none());
    assert_eq!(styles(&s), ["STANDARD", "Model", "InBlock", "Current"], "{:?}", s.log);
}

#[test]
fn purge_mlinestyles_at_the_command_line() {
    let mut s = session();
    s.script("-PURGE M Sp* N\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(styles(&s), ["STANDARD", "Model", "InBlock", "Gone", "Current"]);
}

/// A linetype is in use when only a dimension style, a dimension override, a multiline style, a
/// multiline's copy of its style or an attribute override names it.
#[test]
fn linetypes_named_by_styles_and_attributes_are_kept() {
    let mut s = session();
    let d = s.doc_mut().unwrap();
    for name in ["InDimStyle", "InOverride", "InMline", "InMlineCopy", "InAttribute", "Unused"] {
        let mut lt = d.linetypes[0].clone();
        lt.name = name.into();
        d.linetypes.push(lt);
    }
    d.dim_styles[0].ext_linetype2 = "InDimStyle".into();
    let element = MLineElement { linetype: "InMline".into(), ..MLineElement::default() };
    d.mline_styles.iter_mut().for_each(|t| t.elements.push(element.clone()));
    let at = serde_json::to_value(Vec3::ZERO).unwrap();
    let mut attrib: Attrib = serde_json::from_value(json!({ "tag": "A", "text": { "insert": at, "height": 1.0, "value": "1" } })).unwrap();
    attrib.props.linetype = Some("InAttribute".into());
    let Some(EntityKind::Insert(mut ins)) = d.model.iter().find_map(|e| matches!(e.kind, EntityKind::Insert(_)).then(|| e.kind.clone())) else {
        return;
    };
    ins.attribs.push(attrib);
    d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
    let copy = MLineElement { linetype: "InMlineCopy".into(), ..MLineElement::default() };
    let drawn = MLineStyle { name: "Model".into(), elements: vec![copy], ..MLineStyle::default() };
    d.add(&Space::Model, Common::default(), mline(&drawn)).unwrap();
    let dim = s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [4, 0], "at": [2, 2] })).unwrap();
    let handle = dim["handle"].clone();
    s.execute("dimoverride", &json!({ "handles": [handle], "DIMLTYPE": "InOverride" })).unwrap();
    s.execute("purge", &json!({ "type": "linetypes" })).unwrap();
    let names: Vec<String> = s.doc().unwrap().linetypes.iter().map(|l| l.name.clone()).collect();
    for kept in ["InDimStyle", "InOverride", "InMline", "InMlineCopy", "InAttribute"] {
        assert!(names.iter().any(|n| n == kept), "{kept}: {names:?}");
    }
    assert!(!names.iter().any(|n| n == "Unused"), "{names:?}");
}
