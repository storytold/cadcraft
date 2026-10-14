//! New text follows its style as AutoCAD's does: a fixed-height style isn't asked for a height,
//! an annotative style is asked for the paper height, TEXT takes the style's width factor and
//! obliquing angle, the Style option picks the style, and Enter at the start point continues
//! below the last TEXT.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, HAlign, MText, Text, VAlign};
use serde_json::json;

fn lines(s: &mut Session, input: &[&str]) {
    for l in input {
        s.cmdline(l).unwrap_or_else(|e| panic!("{l:?}: {e:?}"));
    }
}

fn texts(s: &Session) -> Vec<Text> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.clone()) } else { None }).collect()
}

fn mtexts(s: &Session) -> Vec<MText> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t.clone()) } else { None }).collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn a_fixed_height_style_is_not_asked_for_a_height() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Fixed", "height": 2.5 })).unwrap();
    s.cmdline("TEXT").unwrap();
    assert!(s.log.iter().any(|l| l.contains("\"Fixed\"") && l.contains("Text height:  2.5000")), "{:?}", s.log);
    s.cmdline("0,0").unwrap();
    assert!(s.prompt_text().contains("Specify rotation angle of text"), "{}", s.prompt_text());
    lines(&mut s, &["0", "Hello", ""]);
    assert_eq!(texts(&s)[0].height, 2.5);
    // TEXTSIZE isn't changed by a fixed height; JSON text takes the fixed height by default.
    assert_eq!(s.doc().unwrap().header.f64("TEXTSIZE", 0.0), 0.2);
    s.execute("text", &json!({ "at": [0, 10], "text": "Json" })).unwrap();
    assert_eq!(texts(&s)[1].height, 2.5);
    s.execute("text", &json!({ "at": [0, 20], "text": "Own", "height": 1 })).unwrap();
    assert_eq!(texts(&s)[2].height, 1.0, "an explicit height is kept");
}

#[test]
fn new_text_takes_the_styles_width_factor_and_oblique_angle() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Narrow", "widthFactor": 0.8, "oblique": 15 })).unwrap();
    lines(&mut s, &["TEXT", "0,0", "1", "0", "Typed", ""]);
    s.execute("text", &json!({ "at": [0, 5], "text": "Json" })).unwrap();
    s.execute("style", &json!({ "name": "Plain", "current": false })).unwrap();
    s.execute("text", &json!({ "at": [0, 10], "text": "Other", "style": "plain" })).unwrap();
    let t = texts(&s);
    for x in &t[..2] {
        assert_eq!((x.style.as_str(), x.width_factor), ("Narrow", 0.8));
        assert!(close(x.oblique.to_degrees(), 15.0));
    }
    assert_eq!((t[2].style.as_str(), t[2].width_factor, t[2].oblique), ("Plain", 1.0, 0.0));
    // Changing the style afterwards leaves existing text as it is.
    s.execute("style", &json!({ "name": "Narrow", "widthFactor": 1.5 })).unwrap();
    assert_eq!(texts(&s)[0].width_factor, 0.8);
    assert!(s.execute("text", &json!({ "at": [0, 0], "text": "x", "style": "Nope" })).is_err());
}

#[test]
fn an_annotative_style_is_asked_for_the_paper_height() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Notes", "annotative": true })).unwrap();
    lines(&mut s, &["TEXT", "0,0"]);
    assert!(s.prompt_text().contains("Specify paper text height"), "{}", s.prompt_text());
    lines(&mut s, &["0.125", "0", "Note", ""]);
    let d = s.doc().unwrap();
    let e = d.model.iter().find(|e| matches!(e.kind, EntityKind::Text(_))).unwrap();
    assert!(cadcraft_engine::doc::is_annotative(e));
    assert_eq!(texts(&s)[0].height, 0.125);
}

#[test]
fn the_style_option_makes_a_style_current() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Title", "height": 5, "current": false })).unwrap();
    lines(&mut s, &["TEXT", "S"]);
    assert!(s.prompt_text().contains("Enter style name or [?] <Standard>"), "{}", s.prompt_text());
    // ? lists the styles, then asks for the name again.
    lines(&mut s, &["?", ""]);
    assert!(s.log.iter().any(|l| l.contains("Style name: \"Title\"")), "{:?}", s.log);
    let _ = s.cmdline("Nope");
    assert!(s.log.iter().any(|l| l.contains("Cannot find text style \"Nope\".")), "{:?}", s.log);
    assert!(s.prompt_text().contains("Enter style name"), "asked again: {}", s.prompt_text());
    s.cmdline("title").unwrap();
    assert_eq!(s.doc().unwrap().header.str("TEXTSTYLE", ""), "Title");
    assert!(s.log.iter().any(|l| l.contains("Current text style:  \"Title\"  Text height:  5.0000")), "{:?}", s.log);
    assert!(s.prompt_text().contains("Specify start point of text"), "{}", s.prompt_text());
    // Title fixes the height: the next question is the rotation.
    s.cmdline("0,0").unwrap();
    assert!(s.prompt_text().contains("Specify rotation angle of text"), "{}", s.prompt_text());
    lines(&mut s, &["0", "Big", ""]);
    assert_eq!(texts(&s)[0].style, "Title");
    assert_eq!(texts(&s)[0].height, 5.0);
}

#[test]
fn enter_at_the_start_point_continues_below_the_last_text() {
    let mut s = Session::new();
    lines(&mut s, &["TEXT", "J", "MC", "10,10", "2", "90", "First", ""]);
    s.execute("style", &json!({ "name": "Other" })).unwrap();
    lines(&mut s, &["TEXT", ""]);
    assert!(s.prompt_text().contains("Enter text"), "no height or rotation asked: {}", s.prompt_text());
    lines(&mut s, &["Second", "Third", ""]);
    let t = texts(&s);
    assert_eq!(t.len(), 3);
    for (i, x) in t.iter().enumerate().skip(1) {
        assert_eq!((x.height, x.halign, x.valign), (2.0, HAlign::Center, VAlign::Middle));
        assert!(close(x.rotation.to_degrees(), 90.0));
        assert_eq!(x.style, "Standard", "the continued text's style, not the current one");
        // Rotated 90°: lines go to +X, one line spacing (5/3 of the height) apart.
        let p = x.justify_point();
        assert!(close(p.x, 10.0 + 2.0 * 5.0 / 3.0 * i as f64) && close(p.y, 10.0), "line {i}: {p:?}");
    }
    // With no text in the drawing, Enter ends the command.
    let mut empty = Session::new();
    lines(&mut empty, &["TEXT", ""]);
    assert!(empty.running.is_none());
    assert!(texts(&empty).is_empty());
}

#[test]
fn mtext_starts_with_a_fixed_style_height() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Fixed", "height": 3 })).unwrap();
    lines(&mut s, &["MTEXT", "0,0", "20,-10", "Para", ""]);
    s.execute("mtext", &json!({ "at": [0, 20], "text": "Json" })).unwrap();
    // The Style option takes a fixed style's height too.
    s.execute("style", &json!({ "name": "Free" })).unwrap();
    s.execute("style", &json!({ "name": "Tall", "height": 7, "current": false })).unwrap();
    lines(&mut s, &["MTEXT", "0,40", "S", "Tall", "20,30", "Tall text", ""]);
    let heights: Vec<f64> = mtexts(&s).iter().map(|m| m.height).collect();
    assert_eq!(heights, [3.0, 3.0, 7.0]);
}

#[test]
fn hostile_text_input_never_panics() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Weird", "height": 1e308, "widthFactor": 1e-300 })).unwrap();
    for script in [
        vec!["TEXT", "S", "?", &"*".repeat(50_000)],
        vec!["TEXT", "S", &"x".repeat(100_000)],
        vec!["TEXT", "0,0", "0", "x", ""],
        vec!["TEXT", "", "", "", ""],
    ] {
        for l in script {
            let _ = s.cmdline(l);
        }
        s.cancel();
    }
    for p in [
        json!({ "at": [0, 0], "text": "x", "style": 5 }),
        json!({ "at": [0, 0], "text": "x", "height": f64::MAX }),
        json!({ "at": [0, 0], "text": "x", "height": -1 }),
    ] {
        let _ = s.execute("text", &p);
        let _ = s.execute("mtext", &p);
    }
}
