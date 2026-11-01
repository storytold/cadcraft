//! Carets in TEXT, ATTDEF, ATTRIB and DIMENSION strings are written as `^ ` and read back: AutoCAD
//! reads a bare `^2` in `10^2` as a control character. A tab is `^I`, as AutoCAD writes it.

use cadcraft_doc::{Attrib, Common, DimKind, Dimension, Drawing, EntityKind, HAlign, Space, Text, VAlign};
use cadcraft_geom::Vec3;
use cadcraft_io::{read_dxf, write_dxf};

fn text(value: &str) -> Text {
    Text {
        insert: Vec3::new(1.0, 1.0, 0.0),
        align_pt: None,
        height: 2.5,
        value: value.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
    }
}

fn dimension(label: &str) -> Dimension {
    Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(0.0, 5.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(10.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: label.into(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    }
}

fn drawing() -> Drawing {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Text(text("10^2 m, A^JB"))).unwrap();
    let attdef = Attrib {
        tag: "AREA".into(),
        text: text("1^3"),
        invisible: false,
        constant: false,
        prompt: "Area in m^2".into(),
        props: Default::default(),
        multiline: None,
        modes: Default::default(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::AttDef(attdef)).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dimension("<> x10^3"))).unwrap();
    d
}

fn strings(k: &EntityKind) -> Vec<String> {
    match k {
        EntityKind::Text(t) => vec![t.value.clone()],
        EntityKind::AttDef(a) => vec![a.text.value.clone(), a.prompt.clone()],
        EntityKind::Dimension(dm) => vec![dm.text.clone()],
        _ => Vec::new(),
    }
}

fn all_strings(d: &Drawing) -> Vec<String> {
    d.model.iter().flat_map(|e| strings(&e.kind)).collect()
}

#[test]
fn carets_are_written_escaped_and_read_back() {
    let dxf = write_dxf(&drawing());
    // `^J` stays two characters: a single-line string has no line feeds.
    for escaped in ["10^ 2 m, A^ JB", "1^ 3", "Area in m^ 2", "<> x10^ 3"] {
        assert!(dxf.contains(escaped), "{escaped}");
    }
    let back = read_dxf(dxf.as_bytes()).unwrap();
    assert_eq!(all_strings(&back), ["10^2 m, A^JB", "1^3", "Area in m^2", "<> x10^3"]);
    // Older files have bare carets, `^J` too: kept as they are (a single-line string has no line feed).
    let old = read_dxf(dxf.replace("10^ 2 m, A^ JB", "10^2 m, A^JB").as_bytes()).unwrap();
    assert_eq!(all_strings(&old)[0], "10^2 m, A^JB");
    assert!(write_dxf(&old).contains("10^ 2 m, A^ JB"));
    // AutoCAD writes a tab as `^I`: it is read as a tab and written back as `^I`.
    let tab = read_dxf(dxf.replace("10^ 2 m, A^ JB", "A^IB").as_bytes()).unwrap();
    assert_eq!(all_strings(&tab)[0], "A\tB");
    assert!(write_dxf(&tab).contains("A^IB"));
    // Through a DWG.
    let dwg = cadcraft_io::write(&drawing(), "t.dwg").unwrap();
    assert_eq!(all_strings(&cadcraft_io::read(&dwg, "t.dwg").unwrap()), all_strings(&drawing()));
}

#[test]
fn a_caret_before_a_space_or_at_the_end_round_trips() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Text(text("x^ y^"))).unwrap();
    let dxf = write_dxf(&d);
    assert!(dxf.contains("x^  y^ "), "{dxf}");
    assert_eq!(all_strings(&read_dxf(dxf.as_bytes()).unwrap()), ["x^ y^"]);
}

#[test]
fn entget_shows_plain_carets_and_entmake_takes_them() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Text(text("x^ y 10^2"))).unwrap();
    let e = d.model.iter().next().unwrap();
    let (kind, groups) = cadcraft_io::entity_groups(&d, e);
    assert!(groups.iter().any(|t| t.code == 1 && t.str() == "x^ y 10^2"), "{groups:?}");
    // entmod of entget changes nothing; entmake takes a plain string.
    let (_, back) = cadcraft_io::entity_from_groups(&kind, &groups).unwrap();
    assert_eq!(strings(&back), ["x^ y 10^2"]);
    let made = [cadcraft_dxf::Tag::s(1, "p^ q")];
    assert_eq!(strings(&cadcraft_io::entity_from_groups("TEXT", &made).unwrap().1), ["p^ q"]);
    // Every kind, and a DIMENSION's style name (also group 3) as it is.
    let d = drawing();
    for e in d.model.iter() {
        let (kind, groups) = cadcraft_io::entity_groups(&d, e);
        let (_, back) = cadcraft_io::entity_from_groups(&kind, &groups).unwrap();
        assert_eq!(strings(&back), strings(&e.kind), "{kind}");
    }
}
