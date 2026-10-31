//! #1658: `attdef` takes a justification and a text style, as `text` and `mtext` do, so a
//! title-block attribute can be built centred or right-aligned in its style without an `entmod`.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Attrib, EntityKind, HAlign, Handle, VAlign};
use cadcraft_engine::geom::Vec3;
use serde_json::{Value, json};

fn h(v: &Value) -> Handle {
    v["handle"].as_str().and_then(Handle::parse_hex).unwrap()
}

fn attdef(s: &Session, h: Handle) -> Attrib {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::AttDef(a) => a.clone(),
        k => panic!("{k:?}"),
    }
}

/// Runs `attdef` with `p` and returns the definition it made.
fn add(s: &mut Session, p: Value) -> Attrib {
    let made = h(&s.execute("attdef", &p).unwrap());
    attdef(s, made)
}

/// The issue's drawing: a text style ROMANS beside the current Standard.
fn session() -> Session {
    let mut s = Session::new();
    s.execute("style", &json!({"name": "ROMANS", "font": "romans.shx"})).unwrap();
    s.execute("style.current", &json!({"name": "Standard"})).unwrap();
    s
}

#[test]
fn justify_and_style_are_applied() {
    let mut s = session();
    let a = add(&mut s, json!({"tag": "T", "at": [10, 10], "height": 2.5, "justify": "MC", "style": "ROMANS"}));
    assert_eq!((a.text.halign, a.text.valign), (HAlign::Center, VAlign::Middle));
    assert_eq!(a.text.style, "ROMANS");
    assert_eq!(a.text.align_pt, Some(Vec3::new(10.0, 10.0, 0.0)), "justified text is placed by its alignment point");
    // Every code `text` takes, and the style name in any case (kept as the table spells it).
    for (j, want) in [
        ("BR", (HAlign::Right, VAlign::Bottom)),
        ("tl", (HAlign::Left, VAlign::Top)),
        ("C", (HAlign::Center, VAlign::Baseline)),
        ("M", (HAlign::Middle, VAlign::Baseline)),
    ] {
        let a = add(&mut s, json!({"tag": "T", "at": [0, 0], "justify": j, "style": "romans"}));
        assert_eq!((a.text.halign, a.text.valign), want, "{j}");
        assert_eq!(a.text.style, "ROMANS");
    }
}

#[test]
fn without_them_nothing_changes() {
    let mut s = session();
    let a = add(&mut s, json!({"tag": "T", "at": [10, 10]}));
    assert_eq!((a.text.halign, a.text.valign, a.text.align_pt), (HAlign::Left, VAlign::Baseline, None));
    assert_eq!(a.text.style, "Standard", "the current text style");
    // With another current style, that one: not always Standard.
    s.execute("style.current", &json!({"name": "ROMANS"})).unwrap();
    let a = add(&mut s, json!({"tag": "T", "at": [10, 10]}));
    assert_eq!(a.text.style, "ROMANS", "the current text style");
    let a = add(&mut s, json!({"tag": "T", "at": [10, 10], "justify": "L"}));
    assert_eq!((a.text.halign, a.text.valign, a.text.align_pt), (HAlign::Left, VAlign::Baseline, None));
    assert_eq!(a.text.style, "ROMANS", "the current text style");
}

#[test]
fn a_bad_justification_or_style_is_refused() {
    let mut s = session();
    let n = s.doc().unwrap().model.len();
    assert!(s.execute("attdef", &json!({"tag": "T", "at": [0, 0], "justify": "XX"})).is_err());
    assert!(s.execute("attdef", &json!({"tag": "T", "at": [0, 0], "style": "NOPE"})).is_err());
    // Align and Fit need a second baseline point, which attdef has no way to take.
    for j in ["A", "ALIGN", "F", "fit"] {
        assert!(s.execute("attdef", &json!({"tag": "T", "at": [0, 0], "justify": j})).is_err(), "{j}");
    }
    assert_eq!(s.doc().unwrap().model.len(), n, "nothing added");
}

/// The issue's use: a block built from the definition gives its attributes the same placement,
/// and a save and reopen keeps them.
#[test]
fn a_block_and_a_saved_file_keep_them() {
    let mut s = session();
    let ad =
        h(&s.execute("attdef", &json!({"tag": "TITLE", "default": "x", "at": [50, 10], "height": 2.5, "justify": "MC", "style": "ROMANS"})).unwrap());
    let l = s.execute("line", &json!({"points": [[0, 0], [100, 0]]})).unwrap()["handles"][0].as_str().and_then(Handle::parse_hex).unwrap();
    s.execute("block", &json!({"name": "TB", "base": [0, 0], "handles": [ad.hex(), l.hex()], "keep": "delete"})).unwrap();
    let ins = h(&s.execute("insert", &json!({"name": "TB", "at": [0, 0], "attribs": {"TITLE": "PLAN"}})).unwrap());
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(ins).unwrap().kind else { panic!() };
    let att = i.attribs.first().unwrap();
    assert_eq!(
        (att.text.halign, att.text.valign, att.text.style.as_str(), att.text.value.as_str()),
        (HAlign::Center, VAlign::Middle, "ROMANS", "PLAN")
    );
    let read = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    let def =
        read.block("TB").unwrap().entities.iter().find_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some(a.clone()) } else { None }).unwrap();
    assert_eq!((def.text.halign, def.text.valign, def.text.style.as_str()), (HAlign::Center, VAlign::Middle, "ROMANS"), "reopened definition");
    let att = read.model.iter().find_map(|e| if let EntityKind::Insert(i) = &e.kind { i.attribs.first().cloned() } else { None }).unwrap();
    assert_eq!((att.text.halign, att.text.valign, att.text.style.as_str()), (HAlign::Center, VAlign::Middle, "ROMANS"), "reopened attribute");
}

/// A `justify` or `style` that is present but not a string is an error, not a silent default (#1720).
#[test]
fn a_non_string_justify_or_style_is_refused() {
    let mut s = session();
    let n = s.doc().unwrap().model.len();
    for (key, bad) in [("justify", json!(1)), ("justify", json!(true)), ("style", json!(2)), ("style", json!(["ROMANS"]))] {
        let mut p = json!({"tag": "T", "at": [0, 0]});
        p[key] = bad.clone();
        let err = s.execute("attdef", &p).unwrap_err().to_string();
        assert!(err.contains(key) && err.contains("string"), "{key} = {bad}: {err}");
    }
    // Null means absent.
    assert!(s.execute("attdef", &json!({"tag": "T", "at": [0, 0], "justify": null, "style": null})).is_ok());
    assert_eq!(s.doc().unwrap().model.len(), n + 1, "only the null one was added");
}

/// The definition takes its text style's width factor and oblique angle (#1720).
#[test]
fn the_definition_takes_the_styles_width_factor_and_oblique() {
    let mut s = session();
    s.execute("style", &json!({"name": "SLANT", "font": "romans.shx", "widthFactor": 0.7, "oblique": 15, "current": false})).unwrap();
    let a = add(&mut s, json!({"tag": "T", "at": [0, 0], "style": "SLANT"}));
    assert!((a.text.width_factor - 0.7).abs() < 1e-12, "width factor {}", a.text.width_factor);
    assert!((a.text.oblique - 15f64.to_radians()).abs() < 1e-12, "oblique {}", a.text.oblique);
    // The current style, when none is named.
    s.execute("style.current", &json!({"name": "SLANT"})).unwrap();
    let a = add(&mut s, json!({"tag": "T", "at": [0, 0]}));
    assert!((a.text.width_factor - 0.7).abs() < 1e-12 && (a.text.oblique - 15f64.to_radians()).abs() < 1e-12);
    // A plain style keeps 1 and 0.
    let a = add(&mut s, json!({"tag": "T", "at": [0, 0], "style": "Standard"}));
    assert_eq!((a.text.width_factor, a.text.oblique), (1.0, 0.0));
}

/// `-ATTDEF` typed at the command line: tag, prompt and default, then the start-point prompt.
fn typed_to_start_point(s: &mut Session) {
    s.cmdline("-attdef").unwrap();
    for line in ["TAG", "Prompt", "Default"] {
        s.cmdline(line).unwrap();
    }
}

fn last_attdef(s: &Session) -> Attrib {
    let e = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::AttDef(_))).last().unwrap();
    attdef(s, e.handle)
}

#[test]
fn the_start_point_prompt_offers_justify_and_style() {
    let mut s = session();
    typed_to_start_point(&mut s);
    assert!(s.prompt_text().contains("Specify start point of text or [Justify/Style]"), "{}", s.prompt_text());
}

#[test]
fn typed_justify_then_a_point_justifies_the_definition() {
    let mut s = session();
    typed_to_start_point(&mut s);
    s.cmdline("J").unwrap();
    assert!(s.prompt_text().contains("Enter an option"), "{}", s.prompt_text());
    s.cmdline("MC").unwrap();
    assert!(s.prompt_text().contains("Specify start point of text"), "{}", s.prompt_text());
    s.cmdline("10,10").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let a = last_attdef(&s);
    assert_eq!((a.text.halign, a.text.valign), (HAlign::Center, VAlign::Middle));
    assert_eq!(a.text.align_pt, Some(Vec3::new(10.0, 10.0, 0.0)), "stored as the JSON form does");
}

#[test]
fn typed_justify_refuses_align_fit_and_unknown_codes_and_asks_again() {
    let mut s = session();
    typed_to_start_point(&mut s);
    s.cmdline("J").unwrap();
    for bad in ["A", "Fit", "XX"] {
        s.cmdline(bad).unwrap();
        assert!(s.log.iter().any(|l| l.contains("justification") || l.contains("Align and Fit")), "{bad}: {:?}", s.log);
        s.log.clear();
        assert!(s.prompt_text().contains("Enter an option"), "{bad}: {}", s.prompt_text());
    }
    s.cmdline("TR").unwrap();
    s.cmdline("0,0").unwrap();
    let a = last_attdef(&s);
    assert_eq!((a.text.halign, a.text.valign), (HAlign::Right, VAlign::Top));
}

#[test]
fn typed_style_then_a_point_takes_the_styles_values() {
    let mut s = session();
    s.execute("style", &json!({"name": "SLANT", "font": "romans.shx", "widthFactor": 0.7, "oblique": 15, "current": false})).unwrap();
    typed_to_start_point(&mut s);
    s.cmdline("S").unwrap();
    assert!(s.prompt_text().contains("Enter style name"), "{}", s.prompt_text());
    s.cmdline("slant").unwrap();
    s.cmdline("0,0").unwrap();
    let a = last_attdef(&s);
    assert_eq!(a.text.style, "SLANT");
    assert!((a.text.width_factor - 0.7).abs() < 1e-12 && (a.text.oblique - 15f64.to_radians()).abs() < 1e-12);
}

#[test]
fn typed_style_that_does_not_exist_asks_again() {
    let mut s = session();
    typed_to_start_point(&mut s);
    s.cmdline("S").unwrap();
    s.cmdline("NOPE").unwrap();
    assert!(s.log.iter().any(|l| l.contains("no text style \"NOPE\"")), "{:?}", s.log);
    assert!(s.prompt_text().contains("Enter style name"), "{}", s.prompt_text());
    s.cmdline("ROMANS").unwrap();
    s.cmdline("0,0").unwrap();
    assert_eq!(last_attdef(&s).text.style, "ROMANS");
}

/// The keywords a command's Justify prompt offers, from `[A/B/...]` in its prompt text.
fn justify_options(s: &Session) -> Vec<String> {
    let p = s.prompt_text();
    let inner = p.split_once('[').and_then(|(_, r)| r.split_once(']')).map(|(i, _)| i).unwrap_or("");
    inner.split('/').map(str::to_string).collect()
}

/// `-ATTDEF`'s Justify prompt doesn't list Align and Fit, which it would only refuse (they run
/// between two points), while TEXT's still offers every option (#1720).
#[test]
fn attdef_justify_prompt_leaves_out_align_and_fit_but_text_keeps_them() {
    let mut s = session();
    typed_to_start_point(&mut s);
    s.cmdline("J").unwrap();
    let opts = justify_options(&s);
    assert!(opts.iter().any(|o| o == "Left") && opts.iter().any(|o| o == "MC"), "{opts:?}");
    assert!(!opts.iter().any(|o| o == "Align" || o == "Fit"), "{opts:?}");
    // A typed A or F is still refused.
    s.cmdline("A").unwrap();
    assert!(s.log.iter().any(|l| l.contains("Align and Fit")), "{:?}", s.log);
    s.cmdline("*Cancel*").ok();

    let mut t = session();
    t.cmdline("text").unwrap();
    t.cmdline("J").unwrap();
    let opts = justify_options(&t);
    assert!(opts.iter().any(|o| o == "Align") && opts.iter().any(|o| o == "Fit"), "{opts:?}");
}
