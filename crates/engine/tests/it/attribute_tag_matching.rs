//! Attributes are matched to their block's definitions, and definitions to a reference's
//! attributes, by tag in one lookup each: BURST and ATTSYNC on a reference with many of both.

use std::sync::Arc;

use cadcraft_doc::{Attrib, EntityKind, Handle};
use cadcraft_engine::Session;
use serde_json::json;

/// Block B with definition T and a reference to it carrying T = "v"; with `n` > 0 the block gets
/// `n` more definitions and the reference's attribute is replaced by `n`, the tags `n` long ones
/// alike up to their last characters, no attribute's a definition's.
fn reference(n: usize) -> (Session, Handle) {
    let mut s = Session::new();
    s.execute("attdef", &json!({"tag": "T", "default": "d", "at": [0, 1], "height": 1.0})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": "B", "base": [0, 0], "keep": "delete", "handles": hs})).unwrap();
    let r = s.execute("insert", &json!({"name": "B", "at": [0, 0], "attribs": {"T": "v"}})).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    if n > 0 {
        let tag = |kind: char, i: usize| format!("{}{kind}{i:05}", "T".repeat(1000));
        let d = s.doc_mut().unwrap();
        let handles: Vec<Handle> = (0..n).map(|_| d.new_handle()).collect();
        let blk = Arc::make_mut(d.blocks.get_mut("B").unwrap());
        let def = blk.entities.iter().find(|e| matches!(e.kind, EntityKind::AttDef(_))).unwrap().as_ref().clone();
        for (i, h) in handles.into_iter().enumerate() {
            let mut e = def.clone();
            e.handle = h;
            if let EntityKind::AttDef(a) = &mut e.kind {
                a.tag = tag('D', i);
            }
            blk.entities.push(e);
        }
        d.modify_entity(h, |e| {
            if let EntityKind::Insert(ins) = &mut e.kind {
                let one = ins.attribs[0].clone();
                ins.attribs = (0..n).map(|i| Attrib { tag: tag('A', i), ..one.clone() }).collect();
            }
        })
        .unwrap();
    }
    (s, h)
}

fn attribs(s: &Session, h: Handle) -> Vec<Attrib> {
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("not an insert") };
    i.attribs.clone()
}

/// #1923: comparing each attribute with each definition (BURST: for its properties; ATTSYNC: for
/// the value each definition keeps) cost N x N comparisons of whole tags (many seconds here); one
/// lookup each costs milliseconds.
const N: usize = 4000;

#[test]
fn burst_matches_many_attributes_without_comparing_every_pair() {
    let (mut s, h) = reference(N);
    let t = std::time::Instant::now();
    s.execute("burst", &json!({"handles": [h.hex()]})).unwrap();
    assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
    assert_eq!(s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Text(_))).count(), N);
}

#[test]
fn attsync_matches_many_definitions_without_comparing_every_pair() {
    let (mut s, h) = reference(N);
    let t = std::time::Instant::now();
    s.execute("attsync", &json!({"name": "B"})).unwrap();
    assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
    // An attribute for each definition (T and the N others), with its default value: the
    // reference carried none of their tags.
    let a = attribs(&s, h);
    assert_eq!(a.len(), N + 1);
    assert!(a.iter().all(|a| a.text.value == "d"));
}

#[test]
fn attsync_keeps_the_value_of_the_first_attribute_of_a_tag() {
    // A reference carrying two attributes of one tag (in different case, as a file can hold
    // them) keeps the first one's value for the definition of that tag.
    let (mut s, h) = reference(0);
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::Insert(ins) = &mut e.kind {
                let mut second = ins.attribs[0].clone();
                second.tag = "t".into();
                second.text.value = "second".into();
                ins.attribs[0].text.value = "first".into();
                ins.attribs.push(second);
            }
        })
        .unwrap();
    s.execute("attsync", &json!({"name": "B"})).unwrap();
    let a = attribs(&s, h);
    assert_eq!(a.iter().map(|a| (a.tag.as_str(), a.text.value.as_str())).collect::<Vec<_>>(), [("T", "first")]);
}
