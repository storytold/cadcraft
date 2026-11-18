//! MTEXT's options before the text: Height, Justify, Line spacing, Rotation, Style, Width and
//! Columns, from the command line and scripts (#408).

use cadcraft_doc::{EntityKind, MText, TextStyle};
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use serde_json::json;

fn last_mtext(s: &Session) -> MText {
    match s.doc().unwrap().model.iter().last().map(|e| e.kind.clone()) {
        Some(EntityKind::MText(t)) => t,
        k => panic!("no mtext: {k:?}"),
    }
}

fn logged(s: &Session, text: &str) -> bool {
    s.log.iter().any(|l| l.contains(text))
}

fn at(t: &MText) -> Vec2 {
    t.insert.xy()
}

#[test]
fn height_justify_and_corners() {
    let mut s = Session::new();
    s.script("MTEXT 0,10 h 0.5 j mc 20,0\nHello\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let t = last_mtext(&s);
    assert_eq!(t.height, 0.5);
    assert_eq!(t.attach, 5);
    assert_eq!(t.width, 20.0);
    assert!(at(&t).near(Vec2::new(10.0, 5.0), 1e-9), "middle centre of the box: {:?}", at(&t));
    assert_eq!(t.contents, "Hello");
    // The height becomes the default for the next text.
    s.cmdline("mtext 0,0 h").unwrap();
    assert!(s.prompt_text().contains("0.5000"), "{}", s.prompt_text());
    // A picked height is the distance from the first corner.
    s.cmdline("0,2").unwrap();
    s.cmdline("j br 30,-10").unwrap();
    s.cmdline("x").unwrap();
    s.cmdline("").unwrap();
    let t = last_mtext(&s);
    assert_eq!(t.height, 2.0);
    assert_eq!(t.attach, 9);
    assert!(at(&t).near(Vec2::new(30.0, -10.0), 1e-9));
    // Bad answers are reported and asked again.
    s.cmdline("mtext 0,0 j zz").unwrap();
    assert!(logged(&s, "Invalid option keyword."));
    s.cmdline("tl h -1").unwrap();
    assert!(logged(&s, "Requires a positive height."));
    s.cmdline("*cancel*").ok();
}

#[test]
fn line_spacing_type_and_factor_or_distance() {
    let mut s = Session::new();
    s.script("MTEXT 0,10 l e 1.5x 20,0\nA\nB\n\n").unwrap();
    let t = last_mtext(&s);
    assert!(t.line_spacing_exact);
    assert_eq!(t.line_spacing, 1.5);
    // A distance: 1x is 5/3 of the height (0.2 here), so 2/3 is 2x.
    s.script("MTEXT 0,10 l a 0.6667 20,0\nA\n\n").unwrap();
    let t = last_mtext(&s);
    assert!(!t.line_spacing_exact);
    assert!((t.line_spacing - 2.0).abs() < 1e-3, "{}", t.line_spacing);
    // Out of range.
    s.cmdline("mtext 0,0 l e 9x").unwrap();
    assert!(logged(&s, "Requires a factor between 0.25x and 4x"));
    s.cmdline("*cancel*").ok();
}

#[test]
fn rotation_turns_the_box() {
    let mut s = Session::new();
    // Rotated 90 degrees: the box's width runs up from the first corner, its top edge through it.
    s.script("MTEXT 0,0 r 90 5,20\nUp\n\n").unwrap();
    let t = last_mtext(&s);
    assert!((t.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    assert!((t.width - 20.0).abs() < 1e-9, "{}", t.width);
    assert!(at(&t).near(Vec2::new(0.0, 0.0), 1e-9), "top left stays at the first corner: {:?}", at(&t));
    // A picked rotation is the angle from the first corner.
    s.script("MTEXT 1,1 r 2,2 10,0\nDiag\n\n").unwrap();
    assert!((last_mtext(&s).rotation - std::f64::consts::FRAC_PI_4).abs() < 1e-9);
}

#[test]
fn width_replaces_the_opposite_corner() {
    let mut s = Session::new();
    s.script("MTEXT 2,3 j tc w 12\nCentred\nTwo lines\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let t = last_mtext(&s);
    assert_eq!(t.width, 12.0);
    assert_eq!(t.attach, 2);
    assert!(at(&t).near(Vec2::new(2.0, 3.0), 1e-9), "the first point is the insertion point");
    assert_eq!(t.contents, "Centred\\PTwo lines");
    // Picked: the distance along the text direction.
    s.script("MTEXT 0,0 w 7,4\nX\n\n").unwrap();
    assert_eq!(last_mtext(&s).width, 7.0);
    // Zero: no wrapping.
    s.script("MTEXT 0,0 w 0\nY\n\n").unwrap();
    assert_eq!(last_mtext(&s).width, 0.0);
}

#[test]
fn style_and_columns() {
    let mut s = Session::new();
    s.doc_mut().unwrap().text_styles.push(TextStyle { name: "Notes".into(), ..TextStyle::default() });
    s.script("MTEXT 0,10 s notes 20,0\nStyled\n\n").unwrap();
    assert_eq!(last_mtext(&s).style, "Notes");
    s.cmdline("mtext 0,0 s nosuch").unwrap();
    assert!(logged(&s, "Cannot find text style \"nosuch\"."));
    s.cmdline("?").unwrap();
    assert!(logged(&s, "Text styles: Standard, Notes"));
    s.cmdline("").unwrap();
    // Columns: No keeps one column (Dynamic and Static are in mtext_columns.rs).
    s.cmdline("c n 20,-10").unwrap();
    s.cmdline("One column").unwrap();
    s.cmdline("").unwrap();
    let t = last_mtext(&s);
    assert_eq!(t.contents, "One column");
    assert_eq!(t.style, "Standard");
    assert!(t.columns.is_none());
}

#[test]
fn json_form_takes_justify_style_and_spacing() {
    let mut s = Session::new();
    s.execute("mtext", &json!({"at": [1, 2], "text": "a\nb", "justify": "BR", "lineSpacing": 1.25, "lineSpacingExact": true})).unwrap();
    let t = last_mtext(&s);
    assert_eq!(t.attach, 9);
    assert!(t.line_spacing_exact && t.line_spacing == 1.25);
    assert!(s.execute("mtext", &json!({"at": [1, 2], "text": "a", "justify": "XX"})).is_err());
    assert!(s.execute("mtext", &json!({"at": [1, 2], "text": "a", "style": "missing"})).is_err());
}
