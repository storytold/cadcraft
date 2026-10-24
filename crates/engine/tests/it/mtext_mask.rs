//! MTEXT's background mask and drawing direction: the Properties palette turns the mask on and
//! off and sets its fill, color and border offset; DXF keeps both (groups 90, 63, 421, 45, 72).

use cadcraft_engine::Session;
use cadcraft_engine::doc::color::{Color, Rgb};
use cadcraft_engine::doc::{EntityKind, MText, MTextDirection, MTextMask};
use cadcraft_engine::properties::describe;
use serde_json::{Value, json};

fn mtexts(s: &Session) -> Vec<MText> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t.clone()) } else { None }).collect()
}

fn session() -> (Session, String) {
    let mut s = Session::new();
    s.execute("mtext", &json!({ "at": [0, 0], "text": "Note", "height": 1, "width": 10 })).unwrap();
    let h = s.doc().unwrap().model.last().unwrap().handle.hex();
    (s, h)
}

fn rows(s: &Session) -> Vec<Value> {
    let d = s.doc().unwrap();
    let hs: Vec<_> = d.model.iter().map(|e| e.handle).collect();
    let v = serde_json::to_value(describe(d, &hs, None)).unwrap();
    v["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap().clone()).collect()
}

fn row(s: &Session, key: &str) -> Option<Value> {
    rows(s).into_iter().find(|r| r["key"] == key)
}

#[test]
fn the_palette_turns_the_mask_on_and_sets_it() {
    let (mut s, h) = session();
    assert_eq!(row(&s, "backgroundMask").unwrap()["value"], false);
    assert!(row(&s, "maskOffset").is_none(), "its settings show with a mask");
    s.execute("properties.set", &json!({ "handles": [h], "backgroundMask": true })).unwrap();
    assert_eq!(mtexts(&s)[0].background, Some(MTextMask::default()), "the drawing's background, 1.5");
    assert_eq!(row(&s, "maskFill").unwrap()["value"], "background");
    s.execute("properties.set", &json!({ "handles": [h], "maskColor": 1, "maskOffset": 2 })).unwrap();
    let m = mtexts(&s)[0].background.unwrap();
    assert_eq!((m.use_background, m.color, m.scale), (false, Color::Index(1), 2.0), "a color picked fills with it");
    // Out of range is refused; off removes it.
    let r = s.execute("properties.set", &json!({ "handles": [h], "maskOffset": 6 })).unwrap();
    assert_eq!(r["ignored"], json!(["maskOffset"]));
    s.execute("properties.set", &json!({ "handles": [h], "maskFill": "background" })).unwrap();
    assert!(mtexts(&s)[0].background.unwrap().use_background);
    s.execute("properties.set", &json!({ "handles": [h], "backgroundMask": false })).unwrap();
    assert_eq!(mtexts(&s)[0].background, None);
}

#[test]
fn the_direction_is_shown_read_only() {
    let (s, _) = session();
    let r = row(&s, "direction").unwrap();
    assert_eq!((r["value"].as_str(), r["editable"].as_bool()), (Some("leftToRight"), Some(false)));
    assert!(r["note"].as_str().is_some_and(|n| n.contains("left to right")));
}

#[test]
fn dxf_keeps_the_mask_and_direction() {
    let (mut s, h) = session();
    s.execute("properties.set", &json!({ "handles": [h], "backgroundMask": true, "maskColor": "Red", "maskOffset": 3 })).unwrap();
    let mut d = s.doc().unwrap().clone();
    let hh = d.model.iter().next().unwrap().handle;
    d.modify_entity(hh, |e| {
        if let EntityKind::MText(t) = &mut e.kind {
            t.direction = MTextDirection::TopToBottom;
        }
    })
    .unwrap();
    let dxf = cadcraft_io::write_dxf(&d);
    let back = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let t = back.model.iter().find_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t.clone()) } else { None }).unwrap();
    assert_eq!(t.direction, MTextDirection::TopToBottom);
    assert_eq!(t.background, Some(MTextMask { use_background: false, color: Color::Index(1), scale: 3.0 }));
    // A true color keeps its RGB through 421.
    let mut d2 = back.clone();
    d2.modify_entity(hh, |e| {
        if let EntityKind::MText(t) = &mut e.kind
            && let Some(m) = &mut t.background
        {
            m.color = Color::True(Rgb(10, 200, 30));
        }
    })
    .unwrap();
    let again = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d2).as_bytes()).unwrap();
    let t = again.model.iter().find_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t.clone()) } else { None }).unwrap();
    assert_eq!(t.background.unwrap().color, Color::True(Rgb(10, 200, 30)));
}

/// An MTEXT as another program writes it, with the mask groups the DXF Reference lists.
fn foreign_dxf(groups: &str) -> String {
    format!(
        "0\nSECTION\n2\nENTITIES\n0\nMTEXT\n5\n2A\n8\n0\n100\nAcDbEntity\n100\nAcDbMText\n10\n0\n20\n0\n30\n0\n40\n2.5\n41\n20\n71\n1\n1\nHello\n{groups}0\nENDSEC\n0\nEOF\n"
    )
}

fn read(groups: &str) -> MText {
    let d = cadcraft_io::read_dxf(foreign_dxf(groups).as_bytes()).unwrap();
    d.model.iter().find_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t.clone()) } else { None }).unwrap()
}

#[test]
fn masks_from_other_programs_are_read() {
    let t = read("72\n5\n90\n2\n63\n256\n45\n1.25\n");
    assert_eq!(t.direction, MTextDirection::ByStyle);
    assert_eq!(t.background, Some(MTextMask { use_background: true, color: Color::ByLayer, scale: 1.25 }));
    let t = read("90\n1\n63\n3\n421\n255\n45\n1.5\n");
    assert_eq!(t.background.unwrap().color, Color::True(Rgb(0, 0, 255)));
    assert_eq!(read("90\n0\n63\n1\n").background, None, "90 = 0 is off");
    assert_eq!(read("").background, None);
    // Unusable values fall back rather than fail.
    let t = read("72\n99\n90\n1\n63\n99999\n45\n1e300\n");
    assert_eq!(t.direction, MTextDirection::LeftToRight);
    assert_eq!(t.background.unwrap().scale, cadcraft_engine::doc::MASK_SCALE);
}

#[test]
fn hostile_mask_values_never_panic() {
    let (mut s, h) = session();
    for p in [
        json!({ "handles": [h], "backgroundMask": "yes" }),
        json!({ "handles": [h], "backgroundMask": true, "maskColor": { "x": 1 }, "maskOffset": f64::MAX }),
        json!({ "handles": [h], "maskFill": 5, "maskColor": -40000 }),
    ] {
        let _ = s.execute("properties.set", &p);
    }
}
