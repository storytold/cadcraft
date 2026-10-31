//! ATTDEF beyond tag, prompt and default: justification, text style, rotation and the modes,
//! typed (-ATTDEF) and as JSON; Align and Fit span a baseline of two points; Enter at the start
//! point puts the definition under the previous one; `properties.set` edits a definition's fields.

use cadcraft_engine::doc::{Attrib, EntityKind, HAlign, Handle, VAlign};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::json;

fn attdefs(s: &Session) -> Vec<Attrib> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::AttDef(a) => Some(a.clone()),
            _ => None,
        })
        .collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// How long a definition's tag is drawn with its stored height and width factor, left-justified
/// and unrotated: an aligned or fit definition stores the values that make it span its baseline.
fn drawn_width(s: &Session, a: &Attrib) -> f64 {
    let mut t = a.text.clone();
    (t.halign, t.valign, t.align_pt, t.rotation) = (HAlign::Left, VAlign::Baseline, None, 0.0);
    let (_, b) = cadcraft_engine::render::place_text_entity(s.doc().unwrap(), &t, &a.tag);
    b.max.x - b.min.x
}

/// Where a definition is drawn, as it is drawn.
fn drawn_bounds(s: &Session, a: &Attrib) -> [f64; 4] {
    let (_, b) = cadcraft_engine::render::place_text_entity(s.doc().unwrap(), &a.text, &a.tag);
    [b.min.x, b.min.y, b.max.x, b.max.y]
}

#[test]
fn json_attdef_takes_justification_style_rotation_and_modes() {
    let mut s = Session::new();
    s.execute("style", &json!({"name": "Big", "font": "Arial", "height": 3.0, "widthFactor": 0.8, "current": false})).unwrap();
    let r = s
        .execute(
            "attdef",
            &json!({"tag": "no", "prompt": "Number", "default": "1", "at": [10, 20], "justify": "MC", "style": "big", "rotation": 90,
                "invisible": true, "verify": true, "preset": true, "lockPosition": true}),
        )
        .unwrap();
    assert!(r["handle"].is_string());
    let a = &attdefs(&s)[0];
    assert_eq!((a.tag.as_str(), a.prompt.as_str(), a.text.value.as_str()), ("NO", "Number", "1"));
    assert_eq!((a.text.halign, a.text.valign), (HAlign::Center, VAlign::Middle));
    assert_eq!(a.text.align_pt.map(|p| (p.x, p.y)), Some((10.0, 20.0)));
    assert_eq!(a.text.style, "Big");
    assert_eq!(a.text.height, 3.0, "a fixed-height style gives the height");
    assert!(near(a.text.width_factor, 0.8), "the style's width factor");
    assert!(near(a.text.rotation, std::f64::consts::FRAC_PI_2));
    assert!(a.invisible && !a.constant && a.modes.verify && a.modes.preset && a.modes.lock_position);

    // Without a style or height: the current style and TEXTSIZE, left-justified, unrotated.
    s.execute("setvar", &json!({"name": "TEXTSIZE", "value": 2.5})).unwrap();
    s.execute("attdef", &json!({"tag": "B", "at": [0, 0]})).unwrap();
    let b = &attdefs(&s)[1];
    assert_eq!((b.text.halign, b.text.valign), (HAlign::Left, VAlign::Baseline));
    assert_eq!((b.text.height, b.text.rotation, b.text.align_pt), (2.5, 0.0, None));
    assert_eq!(b.prompt, "B", "the tag stands in for a missing prompt");

    // A constant attribute has no prompt; bad fields are refused.
    s.execute("attdef", &json!({"tag": "C", "at": [0, 0], "prompt": "ignored", "constant": true})).unwrap();
    assert!(attdefs(&s)[2].constant && attdefs(&s)[2].prompt.is_empty());
    for bad in [
        json!({"tag": "X", "at": [0, 0], "style": "Nope"}),
        json!({"tag": "X", "at": [0, 0], "justify": "XX"}),
        json!({"tag": "X", "at": [0, 0], "height": -1}),
    ] {
        assert!(s.execute("attdef", &bad).is_err(), "{bad}");
    }
    assert_eq!(attdefs(&s).len(), 3);
}

#[test]
fn typed_attdef_asks_modes_texts_point_height_and_rotation() {
    let mut s = Session::new();
    s.start("-attdef").unwrap();
    assert!(s.prompt_text().contains("attribute modes"), "{}", s.prompt_text());
    s.input(Input::Keyword("Invisible".into())).unwrap();
    s.input(Input::Keyword("Lock position".into())).unwrap();
    // Something else is reported and asked again (the session echoes and keeps the prompt).
    s.input(Input::Text("Nonsense".into())).unwrap();
    assert!(s.log.last().is_some_and(|l| l.contains("Invalid option")), "{:?}", s.log.last());
    assert!(s.prompt_text().contains("attribute modes"), "{}", s.prompt_text());
    s.input(Input::Enter).unwrap();
    assert!(s.prompt_text().contains("tag name"), "{}", s.prompt_text());
    s.input(Input::Text("Room No".into())).unwrap();
    assert!(s.prompt_text().contains("tag name"), "a tag has no spaces: {}", s.prompt_text());
    assert!(s.log.last().is_some_and(|l| l.contains("Invalid attribute tag")), "{:?}", s.log.last());
    s.input(Input::Text("room".into())).unwrap();
    assert!(s.prompt_text().contains("attribute prompt"), "{}", s.prompt_text());
    s.input(Input::Text("Room?".into())).unwrap();
    assert!(s.prompt_text().contains("default attribute value"), "{}", s.prompt_text());
    s.input(Input::Text("101".into())).unwrap();
    assert!(s.prompt_text().contains("start point"), "{}", s.prompt_text());
    s.input(Input::Keyword("Justify".into())).unwrap();
    assert!(s.prompt_text().contains("option"), "{}", s.prompt_text());
    s.input(Input::Keyword("TR".into())).unwrap();
    assert!(s.prompt_text().contains("start point"), "{}", s.prompt_text());
    s.input(Input::Point(Vec2::new(5.0, 6.0))).unwrap();
    assert!(s.prompt_text().contains("height"), "{}", s.prompt_text());
    s.input(Input::Text("2.5".into())).unwrap();
    assert!(s.prompt_text().contains("rotation"), "{}", s.prompt_text());
    s.input(Input::Text("30".into())).unwrap();
    assert!(s.running.is_none());
    let a = &attdefs(&s)[0];
    assert_eq!((a.tag.as_str(), a.prompt.as_str(), a.text.value.as_str()), ("ROOM", "Room?", "101"));
    assert!(a.invisible && a.modes.lock_position && !a.modes.verify && !a.modes.preset && !a.constant);
    assert_eq!((a.text.halign, a.text.valign), (HAlign::Right, VAlign::Top));
    assert_eq!(a.text.align_pt.map(|p| (p.x, p.y)), Some((5.0, 6.0)));
    assert_eq!(a.text.height, 2.5);
    assert!(near(a.text.rotation, 30f64.to_radians()));
    assert_eq!(s.doc().unwrap().header.f64("TEXTSIZE", 0.0), 2.5, "a typed height becomes TEXTSIZE, as TEXT's does");
}

#[test]
fn typed_attdef_style_option_and_constant_mode() {
    let mut s = Session::new();
    s.execute("style", &json!({"name": "Fixed", "font": "Arial", "height": 4.0, "current": false})).unwrap();
    s.start("-attdef").unwrap();
    s.input(Input::Keyword("Constant".into())).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Text("K".into())).unwrap();
    assert!(s.prompt_text().contains("attribute value") && !s.prompt_text().contains("default"), "{}", s.prompt_text());
    s.input(Input::Text("fixed value".into())).unwrap();
    s.input(Input::Keyword("Style".into())).unwrap();
    s.input(Input::Text("Nope".into())).unwrap();
    assert!(s.log.last().is_some_and(|l| l.contains("Cannot find text style")), "{:?}", s.log.last());
    assert!(s.prompt_text().contains("style name"), "{}", s.prompt_text());
    s.input(Input::Text("fixed".into())).unwrap();
    assert!(s.prompt_text().contains("start point"), "{}", s.prompt_text());
    s.input(Input::Point(Vec2::new(1.0, 1.0))).unwrap();
    // A fixed-height style: no height prompt.
    assert!(s.prompt_text().contains("rotation"), "{}", s.prompt_text());
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    let a = &attdefs(&s)[0];
    assert!(a.constant && a.prompt.is_empty());
    assert_eq!((a.text.value.as_str(), a.text.style.as_str(), a.text.height), ("fixed value", "Fixed", 4.0));
    assert_eq!(s.doc().unwrap().header.str("TEXTSTYLE", ""), "Fixed", "the Style option makes the style current");
}

#[test]
fn enter_at_the_start_point_aligns_below_the_previous_definition() {
    let mut s = Session::new();
    s.execute("attdef", &json!({"tag": "A", "at": [10, 20], "justify": "MC", "height": 3.0})).unwrap();
    s.start("-attdef").unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Text("B".into())).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let b = &attdefs(&s)[1];
    assert_eq!((b.text.halign, b.text.valign), (HAlign::Center, VAlign::Middle));
    assert_eq!(b.text.height, 3.0);
    let at = b.text.justify_point();
    assert!(near(at.x, 10.0) && near(at.y, 20.0 - 5.0), "{at:?}: one line (5/3 of the height) below");

    // The JSON form: `below`.
    s.execute("attdef", &json!({"tag": "C", "below": true})).unwrap();
    let c = attdefs(&s)[2].text.justify_point();
    assert!(near(c.x, 10.0) && near(c.y, 10.0), "{c:?}");

    // Nothing to align below: refused.
    let mut empty = Session::new();
    assert!(empty.execute("attdef", &json!({"tag": "C", "below": true})).is_err());
    empty.start("-attdef").unwrap();
    empty.input(Input::Enter).unwrap();
    empty.input(Input::Text("C".into())).unwrap();
    empty.input(Input::Enter).unwrap();
    empty.input(Input::Enter).unwrap();
    empty.input(Input::Enter).unwrap();
    assert!(empty.log.last().is_some_and(|l| l.contains("No previous attribute definition")), "{:?}", empty.log.last());
    assert!(empty.prompt_text().contains("start point"), "still asking for the start point: {}", empty.prompt_text());
}

#[test]
fn properties_set_edits_a_definition() {
    let mut s = Session::new();
    s.execute("style", &json!({"name": "Big", "font": "Arial"})).unwrap();
    let r = s.execute("attdef", &json!({"tag": "A", "prompt": "a?", "default": "a", "at": [10, 20], "height": 1.0})).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let set = |s: &mut Session, p: serde_json::Value| {
        let mut p = p;
        p["handles"] = json!([h.hex()]);
        s.execute("properties.set", &p)
    };
    set(
        &mut s,
        json!({"tag": "b", "prompt": "b?", "text": "bee", "justify": "BR", "style": "big", "height": 4.0, "rotation": 45, "widthFactor": 1.5,
            "invisible": true, "constant": true, "verify": true, "preset": true, "lockPosition": true}),
    )
    .unwrap();
    let a = &attdefs(&s)[0];
    assert_eq!((a.tag.as_str(), a.prompt.as_str(), a.text.value.as_str(), a.text.style.as_str()), ("B", "b?", "bee", "Big"));
    assert_eq!((a.text.halign, a.text.valign), (HAlign::Right, VAlign::Bottom));
    assert_eq!(a.text.align_pt.map(|p| (p.x, p.y)), Some((10.0, 20.0)), "re-justified about the same point");
    assert_eq!((a.text.height, a.text.width_factor), (4.0, 1.5));
    assert!(near(a.text.rotation, 45f64.to_radians()));
    assert!(a.invisible && a.constant && a.modes.verify && a.modes.preset && a.modes.lock_position);
    set(&mut s, json!({"justify": "L", "invisible": false, "position": [1, 2]})).unwrap();
    let a = &attdefs(&s)[0];
    assert_eq!((a.text.halign, a.text.valign, a.text.align_pt), (HAlign::Left, VAlign::Baseline, None));
    assert_eq!((a.text.insert.x, a.text.insert.y), (1.0, 2.0));
    assert!(!a.invisible && a.constant);
    for bad in [json!({"tag": "two words"}), json!({"tag": ""}), json!({"justify": "XX"}), json!({"style": "Nope"})] {
        assert!(set(&mut s, bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(attdefs(&s)[0].tag, "B", "nothing changed by a refused call");
}

#[test]
fn typed_align_and_fit_ask_the_baselines_two_endpoints() {
    let mut s = Session::new();
    s.execute("setvar", &json!({"name": "TEXTSIZE", "value": 2.0})).unwrap();
    s.start("-attdef").unwrap();
    for i in [Input::Enter, Input::Text("A".into()), Input::Text("a?".into()), Input::Text("x".into()), Input::Keyword("Justify".into())] {
        s.input(i).unwrap();
    }
    s.input(Input::Keyword("Align".into())).unwrap();
    assert!(s.prompt_text().contains("first endpoint of text baseline"), "{}", s.prompt_text());
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    assert!(s.prompt_text().contains("second endpoint of text baseline"), "{}", s.prompt_text());
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    assert!(s.log.last().is_some_and(|l| l.contains("must differ")), "{:?}", s.log.last());
    assert!(s.prompt_text().contains("second endpoint"), "{}", s.prompt_text());
    // Align asks no height and no rotation: both come from the baseline.
    s.input(Input::Point(Vec2::new(20.0, 0.0))).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let a = &attdefs(&s)[0];
    assert_eq!(a.text.halign, HAlign::Aligned);
    assert_eq!(((a.text.insert.x, a.text.insert.y), a.text.align_pt.map(|p| (p.x, p.y))), ((0.0, 0.0), Some((20.0, 0.0))));
    assert!(near(a.text.rotation, 0.0));
    assert!((drawn_width(&s, a) - 20.0).abs() < 1e-6, "the height spans the tag over the baseline: {}", drawn_width(&s, a));

    // Fit: the two endpoints, then the height; the width factor spans the baseline.
    s.start("-attdef").unwrap();
    for i in [Input::Enter, Input::Text("F".into()), Input::Text("f?".into()), Input::Text("y".into()), Input::Keyword("Justify".into())] {
        s.input(i).unwrap();
    }
    s.input(Input::Keyword("Fit".into())).unwrap();
    s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    s.input(Input::Point(Vec2::new(5.0, 15.0))).unwrap();
    assert!(s.prompt_text().contains("height"), "{}", s.prompt_text());
    s.input(Input::Text("3".into())).unwrap();
    assert!(s.running.is_none(), "no rotation is asked: {}", s.prompt_text());
    let f = &attdefs(&s)[1];
    assert_eq!((f.text.halign, f.text.height), (HAlign::Fit, 3.0));
    assert_eq!(f.text.align_pt.map(|p| (p.x, p.y)), Some((5.0, 15.0)));
    assert!(near(f.text.rotation, std::f64::consts::FRAC_PI_2), "the baseline's angle");
    assert!((drawn_width(&s, f) - 10.0).abs() < 1e-6, "{}", drawn_width(&s, f));
}

#[test]
fn json_align_and_fit_span_their_end_or_keep_their_size() {
    let mut s = Session::new();
    s.execute("attdef", &json!({"tag": "A", "at": [0, 0], "justify": "A", "end": [0, 30]})).unwrap();
    let a = &attdefs(&s)[0];
    assert_eq!(a.text.align_pt.map(|p| (p.x, p.y)), Some((0.0, 30.0)));
    assert!(near(a.text.rotation, std::f64::consts::FRAC_PI_2));
    assert!((drawn_width(&s, a) - 30.0).abs() < 1e-6, "{}", drawn_width(&s, a));

    // Without `end`, the tag at its own size along the rotation: never both points on one spot,
    // which would draw it unrotated.
    s.execute("attdef", &json!({"tag": "Bee", "at": [10, 0], "justify": "F", "height": 2.0, "rotation": 30})).unwrap();
    let b = &attdefs(&s)[1];
    let (start, end) = (b.text.insert.xy(), b.text.align_pt.unwrap().xy());
    assert!(near(b.text.rotation, 30f64.to_radians()) && near(start.angle_to(end), 30f64.to_radians()), "{start:?} → {end:?}");
    assert!((start.dist(end) - drawn_width(&s, b)).abs() < 1e-6, "its own width");
    assert_eq!((b.text.height, b.text.width_factor), (2.0, 1.0));

    for bad in [json!({"tag": "X", "at": [0, 0], "justify": "MC", "end": [5, 0]}), json!({"tag": "X", "at": [0, 0], "justify": "A", "end": [0, 0]})] {
        assert!(s.execute("attdef", &bad).is_err(), "{bad}");
    }
    assert_eq!(attdefs(&s).len(), 2);
}

#[test]
fn enter_below_takes_the_previous_definitions_width_factor_and_obliquing() {
    let mut s = Session::new();
    s.execute("style", &json!({"name": "Slant", "font": "Arial", "oblique": 15, "current": false})).unwrap();
    let r = s.execute("attdef", &json!({"tag": "A", "at": [0, 10], "style": "slant", "height": 2.0})).unwrap();
    s.execute("properties.set", &json!({"handles": [r["handle"]], "widthFactor": 0.5})).unwrap();
    // The style changes afterwards; the definition keeps what it was made with.
    s.execute("style", &json!({"name": "Slant", "oblique": 0, "current": false})).unwrap();
    s.start("-attdef").unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Text("B".into())).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    s.execute("attdef", &json!({"tag": "C", "below": true})).unwrap();
    for (i, d) in attdefs(&s).iter().enumerate().skip(1) {
        assert_eq!((d.text.style.as_str(), d.text.width_factor), ("Slant", 0.5), "definition {i}");
        assert!(near(d.text.oblique, 15f64.to_radians()), "definition {i}: {}", d.text.oblique.to_degrees());
    }
}

#[test]
fn properties_set_align_and_fit_keep_a_rotated_definitions_size_and_angle() {
    let mut s = Session::new();
    let r = s.execute("attdef", &json!({"tag": "AB", "at": [10, 20], "height": 2.0, "rotation": 30})).unwrap();
    let set = |s: &mut Session, j: &str| s.execute("properties.set", &json!({"handles": [r["handle"]], "justify": j})).unwrap();
    let before = drawn_bounds(&s, &attdefs(&s)[0]);
    let same = |a: [f64; 4], b: [f64; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
    set(&mut s, "A");
    let a = &attdefs(&s)[0];
    assert_eq!(a.text.halign, HAlign::Aligned);
    let end = a.text.align_pt.unwrap();
    assert!(a.text.insert.xy().dist(end.xy()) > 1e-6, "a baseline, not one point");
    assert!(same(drawn_bounds(&s, a), before), "drawn as before: {:?} vs {before:?}", drawn_bounds(&s, a));
    set(&mut s, "F");
    assert_eq!(attdefs(&s)[0].text.align_pt, Some(end), "Align to Fit keeps the baseline");
    set(&mut s, "L");
    let l = &attdefs(&s)[0];
    assert!(near(l.text.rotation, 30f64.to_radians()), "the angle stays: {}", l.text.rotation.to_degrees());
    assert!(same(drawn_bounds(&s, l), before), "{:?} vs {before:?}", drawn_bounds(&s, l));
}

#[test]
fn the_dialog_on_screen_asks_align_and_fit_for_both_endpoints() {
    let mut s = Session::new();
    s.attdef_answers = Some(json!({"tag": "D", "prompt": "d?", "default": "1", "justify": "A", "height": 2.0}));
    s.start("-attdef").unwrap();
    assert!(s.prompt_text().contains("first endpoint of text baseline"), "{}", s.prompt_text());
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    assert!(s.prompt_text().contains("second endpoint of text baseline"), "{}", s.prompt_text());
    s.input(Input::Point(Vec2::new(10.0, 0.0))).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let d = &attdefs(&s)[0];
    assert_eq!((d.text.halign, d.text.align_pt.map(|p| (p.x, p.y))), (HAlign::Aligned, Some((10.0, 0.0))));
    assert!((drawn_width(&s, d) - 10.0).abs() < 1e-6, "{}", drawn_width(&s, d));

    // Fit takes the dialog's height: nothing is asked after the second endpoint.
    s.attdef_answers = Some(json!({"tag": "E", "justify": "F", "height": 2.0}));
    s.start("-attdef").unwrap();
    s.input(Input::Point(Vec2::new(0.0, 5.0))).unwrap();
    s.input(Input::Point(Vec2::new(0.0, 25.0))).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let e = &attdefs(&s)[1];
    assert_eq!((e.text.halign, e.text.height), (HAlign::Fit, 2.0));
    assert!(near(e.text.rotation, std::f64::consts::FRAC_PI_2));
    assert!((drawn_width(&s, e) - 20.0).abs() < 1e-6, "{}", drawn_width(&s, e));
}

#[test]
fn a_blank_prompt_is_the_tag_typed_and_from_the_dialog() {
    let mut s = Session::new();
    s.start("-attdef").unwrap();
    for i in [Input::Enter, Input::Text("room".into()), Input::Enter, Input::Text("101".into())] {
        s.input(i).unwrap();
    }
    for i in [Input::Point(Vec2::new(0.0, 0.0)), Input::Enter, Input::Enter] {
        s.input(i).unwrap();
    }
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(attdefs(&s)[0].prompt, "ROOM", "Enter at the prompt question: INSERT asks for the tag, not a bare value");
    // The dialog's empty (or blank) prompt field the same; a constant attribute keeps none.
    for (answers, expected) in
        [(json!({"tag": "floor", "prompt": " ", "default": "1"}), "FLOOR"), (json!({"tag": "k", "constant": true, "default": "x"}), "")]
    {
        s.attdef_answers = Some(answers);
        s.start("-attdef").unwrap();
        s.input(Input::Point(Vec2::new(0.0, 10.0))).unwrap();
        assert!(s.running.is_none(), "{}", s.prompt_text());
        assert_eq!(attdefs(&s).last().unwrap().prompt, expected);
    }
}
