//! The object structure around a drawing survives saving and reopening: a dimension's extension dictionary and DIMASSOC keep their handles, the geometry the
//! dimension follows names the DIMASSOC as a persistent reactor, and the named object dictionary
//! and the layout dictionary list their entries in key order.

use cadcraft_doc::*;
use cadcraft_dxf::{Tag, parse, records, sections};
use cadcraft_geom::Vec3;
use cadcraft_io::{read_dxf, write_dxf};

fn drawing() -> (Drawing, Handle, Handle) {
    let mut d = Drawing::new_imperial();
    let c = Common::default;
    let l1 = d.add(&Space::Model, c(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(0.0, 5.0, 0.0) })).unwrap();
    let l2 = d.add(&Space::Model, c(), EntityKind::Line(Line { a: Vec3::new(10.0, 0.0, 0.0), b: Vec3::new(10.0, 5.0, 0.0) })).unwrap();
    let dim = Dimension {
        flag_73: Default::default(),
        flip_arrows: Default::default(),
        attachment: Default::default(),
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(0.0, -2.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(10.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: vec![
            DimAssoc { read: Default::default(), point: "p13".into(), handle: l1, snap: AssocSnap::Start },
            DimAssoc { read: Default::default(), point: "p14".into(), handle: l2, snap: AssocSnap::Start },
        ],
        marks: Default::default(),
    };
    d.add(&Space::Model, c(), EntityKind::Dimension(dim)).unwrap();
    (d, l1, l2)
}

/// Every record of a written file: (type, groups).
fn all(text: &str) -> Vec<(String, Vec<Tag>)> {
    let tags = parse(text.as_bytes()).unwrap();
    sections(&tags).into_iter().flat_map(|s| records(&s.tags)).collect()
}

fn handle(g: &[Tag]) -> String {
    g.iter().find(|t| t.code == 5).map(|t| t.str()).unwrap_or_default()
}

/// The handles of the DIMASSOC and of the dictionary that owns it.
fn assoc(text: &str) -> (String, String) {
    let recs = all(text);
    let a = recs.iter().find(|(k, _)| k == "DIMASSOC").expect("a DIMASSOC");
    let owner = a.1.iter().filter(|t| t.code == 330).find(|t| t.str() != "0").map(|t| t.str()).unwrap_or_default();
    (handle(&a.1), owner)
}

/// The reactors of the record with handle `h`.
fn reactors(text: &str, h: Handle) -> Vec<String> {
    let recs = all(text);
    let Some((_, g)) = recs.iter().find(|(_, g)| handle(g) == h.hex()) else { return Vec::new() };
    let Some(start) = g.iter().position(|t| t.code == 102 && t.str() == "{ACAD_REACTORS") else { return Vec::new() };
    g.iter().skip(start + 1).take_while(|t| t.code != 102).map(|t| t.str()).collect()
}

/// The entry keys of the dictionary with handle `h`.
fn keys(text: &str, h: &str) -> Vec<String> {
    let recs = all(text);
    recs.iter()
        .find(|(k, g)| k == "DICTIONARY" && handle(g) == h)
        .map(|(_, g)| g.iter().filter(|t| t.code == 3).map(|t| t.str()).collect())
        .unwrap_or_default()
}

#[test]
fn dimension_structure_keeps_its_handles_and_reactors() {
    let (d, l1, l2) = drawing();
    let first = write_dxf(&d);
    let (a1, x1) = assoc(&first);
    let again = write_dxf(&read_dxf(first.as_bytes()).unwrap());
    let (a2, x2) = assoc(&again);
    assert_eq!((a1.as_str(), x1.as_str()), (a2.as_str(), x2.as_str()), "the DIMASSOC and its dictionary keep their handles");
    for text in [&first, &again] {
        for l in [l1, l2] {
            assert!(reactors(text, l).contains(&a1), "line {} names the DIMASSOC {a1} as a reactor: {:?}", l.hex(), reactors(text, l));
        }
    }
    // And a third time: nothing drifts.
    assert_eq!(write_dxf(&read_dxf(again.as_bytes()).unwrap()), again);
}

#[test]
fn dictionaries_list_entries_in_key_order() {
    let (mut d, _, _) = drawing();
    d.layouts.push(Layout::new("A-Plan", 2));
    let text = write_dxf(&read_dxf(write_dxf(&d).as_bytes()).unwrap());
    let recs = all(&text);
    let root = recs.iter().find(|(k, _)| k == "DICTIONARY").map(|(_, g)| handle(g)).unwrap();
    let sorted = |v: Vec<String>| {
        let mut s = v.clone();
        s.sort();
        (v, s)
    };
    let (got, want) = sorted(keys(&text, &root));
    assert!(got.len() > 3);
    assert_eq!(got, want, "the named object dictionary");
    let layouts = recs
        .iter()
        .find(|(k, g)| k == "DICTIONARY" && g.iter().any(|t| t.code == 3 && t.str() == "Model"))
        .map(|(_, g)| g.iter().filter(|t| t.code == 3).map(|t| t.str()).collect::<Vec<_>>())
        .unwrap();
    let (got, want) = sorted(layouts);
    assert_eq!(got, want, "the layout dictionary");
}

/// A line that a dimension follows and that a kept object of another application reacts to names
/// both: the DIMASSOC the writer adds doesn't replace the kept reactor.
#[test]
fn a_followed_line_keeps_its_foreign_reactor_beside_the_dimassoc() {
    use cadcraft_doc::foreign::{ForeignExtra, ForeignObject};
    let (mut d, l1, l2) = drawing();
    // The line's extension dictionary holds an object of another application, which the line
    // reacts to (a kept object is written when its owners lead to a written record).
    let (xdict, acme) = (Handle(0xACD), Handle(0xACE));
    let tag = |code: i32, value: String| RawTag { code, value };
    let mut f = (*d.foreign).clone();
    f.objects.push(ForeignObject {
        handle: xdict,
        kind: "DICTIONARY".into(),
        tags: vec![tag(330, l1.hex()), tag(100, "AcDbDictionary".into()), tag(281, "1".into()), tag(3, "ACME".into()), tag(350, acme.hex())],
    });
    f.objects.push(ForeignObject {
        handle: acme,
        kind: "XRECORD".into(),
        tags: vec![tag(330, xdict.hex()), tag(100, "AcDbXrecord".into()), tag(280, "1".into())],
    });
    f.entities.insert(l1, ForeignExtra { xdict: Some(xdict), reactors: vec![acme], ..ForeignExtra::default() });
    d.foreign = std::sync::Arc::new(f);
    let text = write_dxf(&d);
    let (assoc_handle, _) = assoc(&text);
    let got = reactors(&text, l1);
    assert!(got.contains(&assoc_handle) && got.contains(&acme.hex()), "reactors of the followed line: {got:?}");
    // A line with no kept reactor names only the DIMASSOC.
    assert_eq!(reactors(&text, l2), vec![assoc_handle]);
}
