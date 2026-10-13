//! -STYLE at the command line (AutoCAD's prompt order, so scripts written for AutoCAD run) and
//! `style.fonts`, the fonts the Text Style dialog offers.

use cadcraft_engine::Session;
use cadcraft_engine::doc::TextStyle;
use serde_json::json;

fn style(s: &Session, name: &str) -> TextStyle {
    s.doc().unwrap().text_style(name).cloned().unwrap()
}

fn current(s: &Session) -> String {
    s.doc().unwrap().header.str("TEXTSTYLE", "")
}

fn lines(s: &mut Session, input: &[&str]) {
    for l in input {
        s.cmdline(l).unwrap_or_else(|e| panic!("{l:?}: {e:?}"));
    }
}

/// A font file with only a naming table (Windows English names), built here: no font file is
/// committed.
fn font_with_names(family: &str, style: &str) -> Vec<u8> {
    let mut strings = Vec::new();
    let mut recs = Vec::new();
    for (id, s) in [(1u16, family), (2, style)] {
        let bytes: Vec<u8> = s.encode_utf16().flat_map(u16::to_be_bytes).collect();
        for v in [3u16, 1, 0x0409, id, bytes.len() as u16, strings.len() as u16] {
            recs.extend_from_slice(&v.to_be_bytes());
        }
        strings.extend(bytes);
    }
    let mut name = Vec::new();
    for v in [0u16, 2, (6 + recs.len()) as u16] {
        name.extend_from_slice(&v.to_be_bytes());
    }
    name.extend(recs);
    name.extend(strings);
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    for v in [1u16, 16, 0, 0] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(b"name");
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&28u32.to_be_bytes());
    out.extend_from_slice(&(name.len() as u32).to_be_bytes());
    out.extend(name);
    out
}

#[test]
fn style_fonts_lists_families_and_styles() {
    cadcraft_fonts::ttf::register("cadcraft-style-test-bd.ttf", font_with_names("CADCraft Style Test", "Bold"));
    cadcraft_fonts::ttf::register("cadcraft-style-test.ttf", font_with_names("CADCraft Style Test", "Regular"));
    let mut s = Session::new();
    let r = s.execute("style.fonts", &json!({})).unwrap();
    let fonts = r["fonts"].as_array().unwrap();
    assert_eq!(fonts[0]["file"], cadcraft_fonts::BUILTIN_FONT, "the built-in font first");
    let ours: Vec<(&str, &str)> =
        fonts.iter().filter(|f| f["family"] == "CADCraft Style Test").map(|f| (f["style"].as_str().unwrap(), f["file"].as_str().unwrap())).collect();
    assert_eq!(ours, [("Regular", "cadcraft-style-test.ttf"), ("Bold", "cadcraft-style-test-bd.ttf")]);
    // A style naming the file uses that face.
    s.execute("style", &json!({ "name": "Heavy", "font": "cadcraft-style-test-bd.ttf" })).unwrap();
    assert!(cadcraft_fonts::ttf::find(&style(&s, "Heavy").font).is_some());
}

#[test]
fn dash_style_asks_in_autocads_order() {
    let mut s = Session::new();
    s.cmdline("-STYLE").unwrap();
    assert!(s.prompt_text().contains("Enter name of text style or [?] <Standard>"), "{}", s.prompt_text());
    lines(&mut s, &["Notes", "romans.shx"]);
    assert!(s.prompt_text().contains("Specify height of text or [Annotative]"), "{}", s.prompt_text());
    lines(&mut s, &["0.25", "0.8", "15", "Y", "N"]);
    // SHX fonts are asked about vertical text.
    assert!(s.prompt_text().contains("Vertical?"), "{}", s.prompt_text());
    s.cmdline("N").unwrap();
    assert!(s.running.is_none());
    let st = style(&s, "Notes");
    assert_eq!((st.font.as_str(), st.height, st.width_factor), ("romans.shx", 0.25, 0.8));
    assert!((st.oblique.to_degrees() - 15.0).abs() < 1e-9);
    assert!(st.backwards && !st.upside_down && !st.vertical);
    assert_eq!(current(&s), "Notes");
    assert!(s.log.iter().any(|l| l.contains("\"Notes\" is now the current text style.")), "{:?}", s.log);
}

#[test]
fn enter_keeps_an_existing_styles_settings() {
    let mut s = Session::new();
    s.execute(
        "style",
        &json!({ "name": "Plan", "font": "CADCraft Stroke", "height": 2.5, "widthFactor": 0.9, "oblique": 10, "upsideDown": true, "current": false }),
    )
    .unwrap();
    s.cmdline("-STYLE").unwrap();
    s.cmdline("plan").unwrap();
    assert!(s.log.iter().any(|l| l == "Existing style."), "{:?}", s.log);
    // Font, height, width factor, obliquing angle, backwards, upside down: all Enter. A TrueType or
    // stroke font isn't asked about vertical text.
    lines(&mut s, &["", "", "", "", "", ""]);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let st = style(&s, "Plan");
    assert_eq!((st.height, st.width_factor, st.upside_down), (2.5, 0.9, true));
    assert!((st.oblique.to_degrees() - 10.0).abs() < 1e-9);
    assert_eq!(current(&s), "Plan");
}

#[test]
fn annotative_styles_ask_for_the_paper_height() {
    let mut s = Session::new();
    lines(&mut s, &["-STYLE", "Anno", "", "A", "Y"]);
    assert!(s.prompt_text().contains("Match text orientation to layout?"), "{}", s.prompt_text());
    s.cmdline("N").unwrap();
    assert!(s.prompt_text().contains("Specify paper text height"), "{}", s.prompt_text());
    lines(&mut s, &["0.125", "", "", "", ""]);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let st = style(&s, "Anno");
    assert!(st.annotative);
    assert_eq!(st.height, 0.125);
}

#[test]
fn bad_answers_ask_again() {
    let mut s = Session::new();
    let still = |s: &mut Session, answer: &str, prompt: &str| {
        let _ = s.cmdline(answer);
        assert!(s.prompt_text().contains(prompt), "{answer:?} is refused: {}", s.prompt_text());
    };
    lines(&mut s, &["-STYLE", "Wide", ""]);
    still(&mut s, "-1", "Specify height of text");
    still(&mut s, "0", "Specify width factor");
    still(&mut s, "0", "Specify width factor");
    still(&mut s, "2", "Specify obliquing angle");
    still(&mut s, "86", "Specify obliquing angle");
    lines(&mut s, &["-20", "", ""]);
    assert!(s.running.is_none());
    let st = style(&s, "Wide");
    assert_eq!(st.width_factor, 2.0);
    assert!((st.oblique.to_degrees() + 20.0).abs() < 1e-9);
}

#[test]
fn question_mark_lists_styles() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Title", "height": 5, "current": false })).unwrap();
    lines(&mut s, &["-STYLE", "?"]);
    let before = s.log.len();
    s.cmdline("T*").unwrap();
    let listed: Vec<&String> = s.log[before..].iter().collect();
    assert!(listed.iter().any(|l| l.contains("Style name: \"Title\"") && l.contains("Height: 5.0000")), "{listed:?}");
    assert!(!listed.iter().any(|l| l.contains("\"Standard\"")), "{listed:?}");
    assert!(s.prompt_text().contains("Enter name of text style"), "back to the name");
    s.cancel();
}

#[test]
fn json_form_is_the_style_command() {
    let mut s = Session::new();
    s.execute("-style", &json!({ "name": "Json", "widthFactor": 0.75, "current": false })).unwrap();
    assert_eq!(style(&s, "Json").width_factor, 0.75);
    assert_eq!(current(&s), "Standard");
    assert!(s.execute("-style", &json!({})).is_err());
}

#[test]
fn hostile_input_never_panics() {
    let mut s = Session::new();
    let long = "x".repeat(100_000);
    for script in [
        vec!["-STYLE", "?", &long],
        vec!["-STYLE", "a/b"],
        vec!["-STYLE", "Ok", &long, "1e308", "NaN", "inf", "-0", "1e-300", "85.0000001", "-85", "maybe", "y", "y"],
        vec!["-STYLE", "Shx", "x.shx", "", "", "", "", "", "Yes"],
    ] {
        for l in script {
            let _ = s.cmdline(l);
        }
        s.cancel();
    }
    for p in [json!({ "name": 5 }), json!({ "name": "n", "height": -1, "widthFactor": 1e300, "oblique": f64::MAX }), json!({ "name": long })] {
        let _ = s.execute("-style", &p);
    }
}
