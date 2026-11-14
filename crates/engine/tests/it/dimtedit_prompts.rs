//! DIMTEDIT at the command line and in scripts: "Select dimension", then a new text location or
//! Left/Right/Center/Home/Angle.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Dimension, EntityKind};
use cadcraft_engine::geom::Vec2;

fn the_dim(s: &Session) -> Dimension {
    s.doc()
        .unwrap()
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Dimension(d) => Some(d.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn dimtedit_moves_justifies_rotates_and_homes_typed_text() {
    let mut s = Session::new();
    s.script("dimlinear 0,0 10,0 5,3\n").unwrap();
    // Pick the dimension on its dimension line, then a new text location.
    s.script("dimtedit 2,3\n").unwrap();
    let p = s.prompt_text();
    assert!(p.contains("Specify new location for dimension text") && p.contains("[Left/Right/Center/Home/Angle]"), "{p}");
    s.script("7,6\n").unwrap();
    assert!(s.running.is_none());
    let d = the_dim(&s);
    assert!(d.user_text_pos && d.text_mid.xy().dist(Vec2::new(7.0, 6.0)) < 1e-9, "{:?}", d.text_mid);
    // DIMTMOVE 0 (the default): the dimension line moved with the text, now through y = 6.
    assert!((d.defpt.y - 6.0).abs() < 1e-9, "{:?}", d.defpt);

    s.script("dimtedit 2,6 h\n").unwrap();
    assert!(!the_dim(&s).user_text_pos, "Home puts the text back");

    s.script("dimtedit 2,6 l\n").unwrap();
    assert_eq!(the_dim(&s).overrides.get("textJust").and_then(|v| v.as_i64()), Some(1));

    s.script("dimtedit 2,6 a 30\n").unwrap();
    assert!(s.running.is_none());
    assert!((the_dim(&s).text_rotation - 30f64.to_radians()).abs() < 1e-9);

    // Something other than a dimension is refused and the prompt stays.
    s.script("line 20,20 30,20\n\ndimtedit 25,20\n").unwrap();
    assert!(s.prompt_text().contains("Select dimension"), "{}", s.prompt_text());
    s.script("2,6 c\n").unwrap();
    assert!(s.running.is_none());
    assert_eq!(the_dim(&s).overrides.get("textJust").and_then(|v| v.as_i64()), Some(0));
}

/// The text position the dimension is drawn with.
fn text_pos(s: &Session) -> Vec2 {
    cadcraft_engine::render::dimension_in(s.doc().unwrap(), &the_dim(s)).text_pos
}

#[test]
fn center_brings_text_that_does_not_fit_back_between_the_extension_lines() {
    let mut s = Session::new();
    // "1.0000" does not fit between extension lines 1 apart: best fit puts it outside (#3114).
    s.script("dimlinear 0,0 1,0 0.5,1\n").unwrap();
    assert!(text_pos(&s).x > 1.0, "{:?}", text_pos(&s));
    s.script("dimtedit 0.5,1 c\n").unwrap();
    assert!(s.running.is_none());
    let p = text_pos(&s);
    assert!((p.x - 0.5).abs() < 1e-9 && (p.y - 1.0).abs() < 1e-9, "Center puts the text mid-line: {p:?}");
    // Home is the default place again, outside.
    s.script("dimedit h 0.5,1\n\n").unwrap();
    assert!(text_pos(&s).x > 1.0, "{:?}", text_pos(&s));
    s.script("dimtedit 0.5,1 c\n").unwrap();
    s.script("dimtedit 0.5,1 h\n").unwrap();
    assert!(text_pos(&s).x > 1.0, "{:?}", text_pos(&s));
}

#[test]
fn align_text_angle_menu_entry_asks_for_the_angle() {
    let mut s = Session::new();
    s.script("dimlinear 0,0 10,0 5,3\n").unwrap();
    let h = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Dimension(_))).unwrap().handle;
    // With the dimension preselected, the menu entry (started with no parameters) asks for the angle.
    s.set_selection(vec![h]);
    s.start("dimtedit.angle").unwrap();
    assert!(s.prompt_text().contains("Specify angle for dimension text"), "{}", s.prompt_text());
    s.script("30\n").unwrap();
    assert!(s.running.is_none());
    assert!((the_dim(&s).text_rotation - 30f64.to_radians()).abs() < 1e-9);

    // Without a selection it asks for the dimension first.
    s.set_selection(vec![]);
    s.start("dimtedit.angle").unwrap();
    assert!(s.prompt_text().contains("Select dimension"), "{}", s.prompt_text());
}
