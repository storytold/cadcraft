//! TEXT's Align and Fit take the two ends of the baseline, as AutoCAD's do: Align works out
//! the height so the text spans it, Fit keeps the height and works out the width factor. The
//! line being typed can be drawn in place (`Session::typing_preview`).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, HAlign, Text};
use cadcraft_geom::Vec2;
use serde_json::json;

fn lines(s: &mut Session, input: &[&str]) {
    for l in input {
        s.cmdline(l).unwrap_or_else(|e| panic!("{l:?}: {e:?}"));
    }
}

fn texts(s: &Session) -> Vec<Text> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.clone()) } else { None }).collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// How wide `t` is drawn, measured as the renderer measures it.
fn width(s: &Session, t: &Text) -> f64 {
    let st = s.doc().unwrap().text_style(&t.style).cloned().unwrap_or_default();
    cadcraft_fonts::text_width(&cadcraft_fonts::TextFont::resolve(&st.font), &t.value, t.height, t.width_factor)
}

#[test]
fn align_works_out_the_height_from_the_baseline() {
    let mut s = Session::new();
    lines(&mut s, &["TEXT", "J", "A"]);
    assert!(s.prompt_text().contains("Specify first endpoint of text baseline"), "{}", s.prompt_text());
    s.cmdline("0,0").unwrap();
    assert!(s.prompt_text().contains("Specify second endpoint of text baseline"), "{}", s.prompt_text());
    s.cmdline("10,0").unwrap();
    assert!(s.prompt_text().contains("Enter text"), "no height or rotation: {}", s.prompt_text());
    lines(&mut s, &["ABC", "ABCDEF", ""]);
    let t = texts(&s);
    assert_eq!(t.len(), 2);
    for x in &t {
        assert_eq!(x.halign, HAlign::Aligned);
        assert!(close(width(&s, x), 10.0), "{} spans the baseline: {}", x.value, width(&s, x));
        assert_eq!(x.width_factor, 1.0, "Align keeps the width factor");
        assert_eq!(x.rotation, 0.0);
    }
    assert!(t[1].height < t[0].height, "longer text is smaller");
    // The second line is one line spacing of the first below it, as long.
    assert_eq!(t[0].insert.xy(), Vec2::new(0.0, 0.0));
    assert_eq!(t[0].align_pt.unwrap().xy(), Vec2::new(10.0, 0.0));
    let down = t[0].height * 5.0 / 3.0;
    assert!(close(t[1].insert.y, -down) && close(t[1].align_pt.unwrap().y, -down) && close(t[1].align_pt.unwrap().x, 10.0));
}

#[test]
fn fit_keeps_the_height_and_works_out_the_width_factor() {
    let mut s = Session::new();
    lines(&mut s, &["TEXT", "J", "F", "0,0", "10,5"]);
    assert!(s.prompt_text().contains("Specify height"), "{}", s.prompt_text());
    s.cmdline("2").unwrap();
    assert!(s.prompt_text().contains("Enter text"), "no rotation: {}", s.prompt_text());
    lines(&mut s, &["Hi", ""]);
    let t = &texts(&s)[0];
    assert_eq!((t.halign, t.height), (HAlign::Fit, 2.0));
    assert!(close(width(&s, t), 125f64.sqrt()), "{}", width(&s, t));
    assert!(t.width_factor > 1.0);
    assert!(close(t.rotation, 5f64.atan2(10.0)));
    // A fixed-height style isn't asked for a height.
    s.execute("style", &json!({ "name": "Fixed", "height": 3 })).unwrap();
    lines(&mut s, &["TEXT", "J", "F", "0,20", "10,20"]);
    assert!(s.prompt_text().contains("Enter text"), "{}", s.prompt_text());
    lines(&mut s, &["Fixed", ""]);
    assert_eq!(texts(&s)[1].height, 3.0);
}

#[test]
fn the_two_ends_must_differ() {
    let mut s = Session::new();
    lines(&mut s, &["TEXT", "J", "A", "5,5"]);
    let _ = s.cmdline("5,5");
    assert!(s.prompt_text().contains("Specify second endpoint of text baseline"), "asked again: {}", s.prompt_text());
    lines(&mut s, &["15,5", "Ok", ""]);
    assert_eq!(texts(&s).len(), 1);
}

#[test]
fn enter_continues_aligned_text_along_a_baseline_as_long() {
    let mut s = Session::new();
    lines(&mut s, &["TEXT", "J", "A", "0,0", "8,0", "First", ""]);
    lines(&mut s, &["TEXT", ""]);
    assert!(s.prompt_text().contains("Enter text"), "{}", s.prompt_text());
    lines(&mut s, &["Second line", ""]);
    let t = texts(&s);
    assert_eq!(t[1].halign, HAlign::Aligned);
    assert!(close(width(&s, &t[1]), 8.0));
    assert!(close(t[1].insert.y, -t[0].height * 5.0 / 3.0));
}

#[test]
fn json_align_and_fit_take_the_baselines_end() {
    let mut s = Session::new();
    s.execute("text", &json!({ "at": [0, 0], "end": [20, 0], "justify": "A", "text": "Json" })).unwrap();
    s.execute("text", &json!({ "at": [0, 10], "end": [20, 10], "justify": "F", "text": "Json", "height": 1 })).unwrap();
    let t = texts(&s);
    assert!(close(width(&s, &t[0]), 20.0) && close(width(&s, &t[1]), 20.0));
    assert_eq!((t[0].halign, t[1].halign, t[1].height), (HAlign::Aligned, HAlign::Fit, 1.0));
    // Without `end`, the baseline runs along the rotation as long as the text is.
    s.execute("text", &json!({ "at": [0, 20], "justify": "F", "text": "Plain", "height": 2, "rotation": 90 })).unwrap();
    let t = texts(&s);
    assert_eq!((t[2].halign, t[2].height), (HAlign::Fit, 2.0));
    assert!(close(t[2].width_factor, 1.0), "{}", t[2].width_factor);
    assert!(close(t[2].align_pt.unwrap().y - 20.0, width(&s, &t[2])) && close(t[2].align_pt.unwrap().x, 0.0));
    assert!(s.execute("text", &json!({ "at": [1, 1], "end": [1, 1], "justify": "F", "text": "x" })).is_err(), "no length");
}

#[test]
fn the_line_being_typed_is_previewed_where_it_goes() {
    let mut s = Session::new();
    lines(&mut s, &["TEXT", "0,0"]);
    assert!(s.typing_preview("2").is_empty(), "the height prompt places nothing");
    lines(&mut s, &["2", "0"]);
    assert!(s.typing_preview("").is_empty());
    let first = s.typing_preview("Hel");
    let [e] = first.as_slice() else { panic!("{first:?}") };
    let EntityKind::Text(t) = &e.kind else { panic!() };
    assert_eq!((t.value.as_str(), t.height, t.insert.xy()), ("Hel", 2.0, Vec2::new(0.0, 0.0)));
    assert!(texts(&s).is_empty(), "a preview isn't added");
    // After a line the preview is one line lower.
    s.cmdline("Hello").unwrap();
    let EntityKind::Text(t) = &s.typing_preview("W")[0].kind else { panic!() };
    assert!(close(t.insert.y, -2.0 * 5.0 / 3.0));
    s.cancel();
    assert!(s.typing_preview("x").is_empty(), "no command");
    // Aligned text previews at the height it would get.
    lines(&mut s, &["TEXT", "J", "A", "0,0", "6,0"]);
    let EntityKind::Text(t) = &s.typing_preview("Wide")[0].kind else { panic!() };
    assert!(close(width(&s, t), 6.0));
    s.cancel();
}

#[test]
fn hostile_two_point_input_never_panics() {
    let mut s = Session::new();
    for script in [
        vec!["TEXT", "J", "A", "0,0", "1e308,0", "x", ""],
        vec!["TEXT", "J", "F", "0,0", "1e-300,0", "1e-300", "x", ""],
        vec!["TEXT", "J", "A", "0,0", "1,0", "", ""],
        vec!["TEXT", "J", "F", "0,0", "1,0", "-5", "nan", "inf", "1", &"W".repeat(100_000), ""],
    ] {
        for l in script {
            let _ = s.cmdline(l);
            let _ = s.typing_preview(&"y".repeat(10_000));
        }
        s.cancel();
    }
    for p in [
        json!({ "at": [0, 0], "end": "x", "justify": "A", "text": "x" }),
        json!({ "at": [0, 0], "end": [1e308, 1e308], "justify": "F", "text": "x" }),
    ] {
        let _ = s.execute("text", &p);
    }
}
