//! EXPLODE turns a dimension's text into MTEXT that looks like the text that was drawn: the
//! dimension's overrides and DIMSCALE (0 = the drawing's) set its height, and aligned text keeps
//! its angle (issue #367).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle, MText};
use serde_json::{Value, json};

/// Draw a vertical dimension after `setup`, explode it, and return (drawn text height, drawn
/// text angle, the exploded MTEXT).
fn explode_vertical(setup: &[(&str, Value)]) -> (f64, f64, MText) {
    let mut s = Session::new();
    for (cmd, p) in setup {
        s.execute(cmd, p).unwrap();
    }
    let r = s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [0, 30], "at": [5, 15]})).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let drawn = match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Dimension(dm) => cadcraft_engine::render::dimension_in(s.doc().unwrap(), dm),
        k => panic!("not a dimension: {k:?}"),
    };
    s.execute("explode", &json!({"handles": [h.hex()]})).unwrap();
    let mut texts = s.doc().unwrap().model.iter().filter_map(|e| match &e.kind {
        EntityKind::MText(m) => Some(m.clone()),
        _ => None,
    });
    let m = texts.next().expect("the exploded dimension has its text");
    assert!(texts.next().is_none());
    (drawn.text_height, drawn.text_angle, m)
}

#[test]
fn exploded_dimension_text_matches_the_drawn_text() {
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    // SETVAR DIMSCALE is an override on the dimension: 10 × DIMTXT 0.18.
    let (h, _, m) = explode_vertical(&[("setvar", json!({"name": "DIMSCALE", "value": 10}))]);
    assert!(near(h, 1.8) && near(m.height, h), "drawn {h}, exploded {}", m.height);
    // A style DIMSCALE of 0 draws at scale 1 in model space; the text must not shrink to ~0.
    let (h, _, m) = explode_vertical(&[("dimstyle", json!({"name": "Standard", "DIMSCALE": 0}))]);
    assert!(near(h, 0.18) && near(m.height, h), "drawn {h}, exploded {}", m.height);
    // Aligned text (DIMTIH off) on a vertical dimension is drawn at 90° and stays there.
    let (_, a, m) = explode_vertical(&[("dimstyle", json!({"name": "Standard", "DIMTIH": 0}))]);
    assert!(near(a, std::f64::consts::FRAC_PI_2) && near(m.rotation, a), "drawn {a}, exploded {}", m.rotation);
    assert_eq!(m.contents, "30.0000");
}
