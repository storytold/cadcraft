//! `properties.set` for text, mtext and dimensions: the edits the Properties palette makes, keys
//! nobody took coming back as `ignored`, and style edits reaching dimensions read from files.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, HAlign, Handle, MText, Text};
use serde_json::{Value, json};

fn handle(r: &Value) -> String {
    r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string()
}

fn last(s: &Session) -> String {
    s.doc().unwrap().model.last().unwrap().handle.hex()
}

fn set(s: &mut Session, h: &str, p: Value) -> Value {
    let mut p = p;
    p["handles"] = json!([h]);
    s.execute("properties.set", &p).unwrap()
}

fn text(s: &Session, h: &str) -> Text {
    match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
        EntityKind::Text(t) => t.clone(),
        k => panic!("not text: {k:?}"),
    }
}

fn mtext(s: &Session, h: &str) -> MText {
    match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
        EntityKind::MText(t) => t.clone(),
        k => panic!("not mtext: {k:?}"),
    }
}

fn dim(s: &Session, h: &str) -> cadcraft_engine::doc::Dimension {
    match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
        EntityKind::Dimension(d) => d.clone(),
        k => panic!("not a dimension: {k:?}"),
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn text_style_justify_oblique_and_position() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Notes", "current": false })).unwrap();
    let t = handle(&s.execute("text", &json!({ "at": [5, 5], "text": "Hello", "height": 2 })).unwrap());
    let r = set(&mut s, &t, json!({ "style": "notes", "oblique": 15, "bogus": 1 }));
    assert_eq!(r["ignored"], json!(["bogus"]));
    assert_eq!(text(&s, &t).style, "Notes");
    assert!(near(text(&s, &t).oblique.to_degrees(), 15.0));
    // A style that doesn't exist is refused; an oblique past 85 degrees is ignored.
    assert!(s.execute("properties.set", &json!({ "handles": [t], "style": "Nope" })).is_err());
    assert_eq!(set(&mut s, &t, json!({ "oblique": 90 }))["ignored"], json!(["oblique"]));

    // Justify keeps the point the text is placed by: the text moves around it.
    set(&mut s, &t, json!({ "justify": "MC" }));
    let j = text(&s, &t);
    assert_eq!((j.halign, j.justify_point().x, j.justify_point().y), (HAlign::Center, 5.0, 5.0));
    // Position X moves the text along X only.
    set(&mut s, &t, json!({ "positionX": 20 }));
    let j = text(&s, &t);
    assert_eq!((j.justify_point().x, j.justify_point().y), (20.0, 5.0));
    // Fit puts the second point at the end of the text, along its rotation.
    set(&mut s, &t, json!({ "justify": "Fit" }));
    let f = text(&s, &t);
    let end = f.align_pt.unwrap();
    assert!(f.halign == HAlign::Fit && near(f.insert.x, 20.0) && end.x > 20.0 && near(end.y, 5.0), "{f:?}");
    // The second point alone (the text refits between them).
    set(&mut s, &t, json!({ "alignmentX": 40 }));
    assert!(near(text(&s, &t).align_pt.unwrap().x, 40.0) && near(text(&s, &t).insert.x, 20.0));
    // Back to left: one point, where the fit text started.
    set(&mut s, &t, json!({ "justify": "Left" }));
    let l = text(&s, &t);
    assert!(l.align_pt.is_none() && near(l.insert.x, 20.0));
    // Alignment points belong to aligned and fit text only.
    assert_eq!(set(&mut s, &t, json!({ "alignmentX": 1 }))["ignored"], json!(["alignmentX"]));
}

#[test]
fn mtext_style_justify_and_line_spacing() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Notes", "current": false })).unwrap();
    let m = handle(&s.execute("mtext", &json!({ "at": [0, 10], "text": "one\\Ptwo", "height": 3, "width": 40 })).unwrap());
    set(&mut s, &m, json!({ "style": "Notes", "justify": "MC", "lineSpacing": 2, "lineSpacingStyle": "Exactly" }));
    let t = mtext(&s, &m);
    assert_eq!((t.style.as_str(), t.attach, t.line_spacing, t.line_spacing_exact), ("Notes", 5, 2.0, true));
    // The attachment point stays put.
    assert!(near(t.insert.x, 0.0) && near(t.insert.y, 10.0));
    // MTEXT's factors run from 0.25 to 4.
    assert_eq!(set(&mut s, &m, json!({ "lineSpacing": 10 }))["ignored"], json!(["lineSpacing"]));
    // A distance between lines sets the factor: factor × height × 5/3.
    set(&mut s, &m, json!({ "lineSpacingDistance": 7.5 }));
    assert!(near(mtext(&s, &m).line_spacing, 1.5));
    set(&mut s, &m, json!({ "lineSpacingStyle": "atLeast", "positionX": 4, "positionY": 6 }));
    let t = mtext(&s, &m);
    assert!(!t.line_spacing_exact && near(t.insert.x, 4.0) && near(t.insert.y, 6.0));
}

#[test]
fn dimension_variables_become_overrides_of_each_dimension() {
    let mut s = Session::new();
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    let dm = last(&s);
    set(&mut s, &dm, json!({ "textHeight": 0.3 }));
    assert_eq!(dim(&s, &dm).overrides.get("textHeight"), Some(&json!(0.3)));
    let geometry = |s: &Session| cadcraft_engine::render::dimension_in(s.doc().unwrap(), &dim(s, &dm));
    assert!(near(geometry(&s).text_height, 0.3));
    // A DIM* name replaces the override under its field name.
    set(&mut s, &dm, json!({ "DIMTXT": 0.25, "DIMGAP": 0.05 }));
    let o = dim(&s, &dm).overrides;
    assert_eq!((o.len(), o.get("textHeight"), o.get("textGap")), (2, Some(&json!(0.25)), Some(&json!(0.05))));
    // Clear one, then all.
    set(&mut s, &dm, json!({ "clear": ["DIMGAP"] }));
    assert_eq!(dim(&s, &dm).overrides.keys().collect::<Vec<_>>(), ["textHeight"]);
    set(&mut s, &dm, json!({ "clearOverrides": true }));
    assert!(dim(&s, &dm).overrides.is_empty());
    assert!(near(geometry(&s).text_height, 0.18));
    // Bad values and unknown text styles don't become overrides.
    // An unusable text height is refused (as Properties' dimension rows refuse it, #1473).
    assert!(s.execute("properties.set", &json!({ "handles": [dm], "textHeight": -1 })).is_err());
    assert!(s.execute("properties.set", &json!({ "handles": [dm], "textStyle": "Nope" })).is_err());
    assert!(dim(&s, &dm).overrides.is_empty());
    // A refused call makes no layer either.
    assert!(s.execute("properties.set", &json!({ "handles": [dm], "layer": "Notes", "dimStyle": "Nope" })).is_err());
    assert!(s.doc().unwrap().layer("Notes").is_none());
}

#[test]
fn closed_filled_for_one_end_is_drawn_whatever_dimblk_says() {
    let mut s = Session::new();
    s.execute("dimstyle", &json!({ "name": "Ticks", "arrowBlock": "_ARCHTICK" })).unwrap();
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    let dm = last(&s);
    // Closed filled is "" among the arrowheads; for one end it's named, as "" there is DIMBLK's.
    set(&mut s, &dm, json!({ "arrowBlock1": "" }));
    assert_eq!(dim(&s, &dm).overrides.get("arrowBlock1"), Some(&json!("_CLOSEDFILLED")));
    let rows = s.execute("properties.describe", &json!({ "handles": [dm] })).unwrap();
    let arrow = |key: &str| {
        rows["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap()).find(|r| r["key"] == key).unwrap()["value"].clone()
    };
    assert_eq!((arrow("arrowBlock1"), arrow("arrowBlock2")), (json!(""), json!("_ARCHTICK")));
}

#[test]
fn dimension_text_rows() {
    let mut s = Session::new();
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    let dm = last(&s);
    set(&mut s, &dm, json!({ "prefix": "~", "suffix": " mm", "textRotation": 30, "textPositionX": 2 }));
    let d = dim(&s, &dm);
    assert_eq!(d.overrides.get("post"), Some(&json!("~<> mm")));
    assert!(near(d.text_rotation.to_degrees(), 30.0));
    assert!(d.user_text_pos && near(d.text_mid.x, 2.0));
    let g = cadcraft_engine::render::dimension_in(s.doc().unwrap(), &d);
    assert!(g.value.starts_with('~') && g.value.ends_with(" mm"), "{}", g.value);
    // A suffix alone keeps the prefix.
    set(&mut s, &dm, json!({ "suffix": "" }));
    assert_eq!(dim(&s, &dm).overrides.get("post"), Some(&json!("~<>")));
    // Together with other edits, the prefix goes into what those leave: here no suffix override.
    set(&mut s, &dm, json!({ "suffix": " mm" }));
    set(&mut s, &dm, json!({ "clearOverrides": true, "prefix": "=" }));
    assert_eq!(dim(&s, &dm).overrides.get("post"), Some(&json!("=<>")));
    // The prefix and suffix rows are DIMPOST's halves: overridden with it, reset with it.
    let rows = s.execute("properties.describe", &json!({ "handles": [dm] })).unwrap();
    let prefix =
        rows["groups"].as_array().unwrap().iter().flat_map(|g| g["rows"].as_array().unwrap()).find(|r| r["key"] == "prefix").unwrap().clone();
    assert_eq!(prefix["overridden"], true, "{prefix}");
    set(&mut s, &dm, json!({ "clear": ["prefix"] }));
    assert!(dim(&s, &dm).overrides.get("post").is_none());
}

#[test]
fn rotating_aligned_text_turns_its_baseline() {
    let mut s = Session::new();
    let t = handle(&s.execute("text", &json!({ "at": [0, 0], "text": "Wide", "height": 1 })).unwrap());
    set(&mut s, &t, json!({ "justify": "Align" }));
    let long = text(&s, &t).align_pt.unwrap().x;
    assert!(long > 0.0);
    set(&mut s, &t, json!({ "rotation": 90 }));
    let end = text(&s, &t).align_pt.unwrap();
    assert!(near(end.x, 0.0) && near(end.y, long), "{end:?}");
}

#[test]
fn hatch_pattern_edits_go_as_hatchedits_do() {
    let mut s = Session::new();
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    s.execute("gradient", &json!({ "points": [[5, 5]], "color1": "1", "color2": "3" })).unwrap();
    let h = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Hatch(_))).unwrap().handle.hex();
    let hatch = |s: &Session| match &s.doc().unwrap().entity(Handle::parse_hex(&h).unwrap()).unwrap().kind {
        EntityKind::Hatch(x) => x.clone(),
        k => panic!("not a hatch: {k:?}"),
    };
    assert!(hatch(&s).gradient.is_some());
    // SOLID is a plain fill, not the gradient.
    set(&mut s, &h, json!({ "pattern": "SOLID" }));
    assert!(hatch(&s).solid && hatch(&s).gradient.is_none());
    // A blank name changes nothing.
    assert_eq!(set(&mut s, &h, json!({ "pattern": "  " }))["ignored"], json!(["pattern"]));
    assert_eq!(hatch(&s).pattern, "SOLID");
    // A name no pattern has is refused, as HATCHEDIT refuses it; a defined one is taken.
    assert!(s.execute("properties.set", &json!({ "handles": [h], "pattern": "ANSI3" })).is_err());
    assert_eq!(hatch(&s).pattern, "SOLID");
    set(&mut s, &h, json!({ "pattern": "ansi31" }));
    assert_eq!(hatch(&s).pattern, "ANSI31");
}

#[test]
fn an_object_that_is_not_there_refuses_the_whole_call() {
    let mut s = Session::new();
    let t = handle(&s.execute("text", &json!({ "at": [0, 0], "text": "A", "height": 1 })).unwrap());
    assert!(s.execute("properties.set", &json!({ "handles": [t, "FFFFF"], "height": 3, "layer": "Notes" })).is_err());
    assert_eq!(text(&s, &t).height, 1.0);
    assert!(s.doc().unwrap().layer("Notes").is_none());
}

#[test]
fn edits_of_a_dimension_read_from_a_file_redraw_it() {
    let mut s = Session::new();
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    // Read back from DXF, a dimension draws the block it was saved with.
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(back, "dims", None);
    let dm = last(&s);
    assert!(dim(&s, &dm).block.is_some());
    // Nothing changed: the block stays.
    set(&mut s, &dm, json!({ "clear": [], "color": "ByLayer" }));
    assert!(dim(&s, &dm).block.is_some());
    set(&mut s, &dm, json!({ "textHeight": 0.4 }));
    assert!(dim(&s, &dm).block.is_none());
}

#[test]
fn style_edits_redraw_dimensions_read_from_a_file() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "DimText", "current": false })).unwrap();
    s.execute("dimstyle", &json!({ "name": "Plan", "textStyle": "DimText" })).unwrap();
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    s.execute("dimstyle", &json!({ "name": "Other" })).unwrap();
    s.execute("dimlinear", &json!({ "p1": [0, 10], "p2": [10, 10], "at": [5, 13] })).unwrap();
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(back, "dims", None);
    let dims: Vec<(String, cadcraft_engine::doc::Dimension)> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::Dimension(d) = &e.kind { Some((e.handle.hex(), d.clone())) } else { None })
        .collect();
    let plan = dims.iter().find(|(_, d)| d.style == "Plan").unwrap().0.clone();
    let other = dims.iter().find(|(_, d)| d.style == "Other").unwrap().0.clone();
    assert!(dim(&s, &plan).block.is_some() && dim(&s, &other).block.is_some());
    // Editing a dimension style redraws the dimensions that use it, and only those.
    s.execute("dimstyle", &json!({ "name": "Plan", "DIMTXT": 0.5, "current": false })).unwrap();
    assert!(dim(&s, &plan).block.is_none() && dim(&s, &other).block.is_some());
    let g = cadcraft_engine::render::dimension_in(s.doc().unwrap(), &dim(&s, &plan));
    assert!(near(g.text_height, 0.5));
    // Editing a text style redraws the dimensions whose text uses it.
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    s.open_drawing(back, "dims", None);
    let plan = s.doc().unwrap().model.iter().find(|e| matches!(&e.kind, EntityKind::Dimension(d) if d.style == "Plan")).unwrap().handle.hex();
    assert!(dim(&s, &plan).block.is_some());
    s.execute("style", &json!({ "name": "DimText", "widthFactor": 0.8, "current": false })).unwrap();
    assert!(dim(&s, &plan).block.is_none());
}

#[test]
fn renaming_a_text_style_follows_dimension_overrides() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Old", "current": false })).unwrap();
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    let dm = last(&s);
    set(&mut s, &dm, json!({ "textStyle": "Old" }));
    s.execute("style.rename", &json!({ "from": "Old", "to": "New" })).unwrap();
    assert_eq!(dim(&s, &dm).overrides.get("textStyle"), Some(&json!("New")));
}

#[test]
fn hostile_values_never_panic() {
    let mut s = Session::new();
    let t = handle(&s.execute("text", &json!({ "at": [0, 0], "text": "A", "height": 1 })).unwrap());
    let m = handle(&s.execute("mtext", &json!({ "at": [0, 5], "text": "B", "height": 1 })).unwrap());
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    let dm = last(&s);
    for v in [json!(null), json!("x"), json!(1e308), json!(-1e308), json!([1, 2]), json!({ "a": 1 }), json!(true)] {
        let mut p = json!({ "handles": [t, m, dm] });
        for k in [
            "justify",
            "oblique",
            "positionX",
            "positionZ",
            "alignmentX",
            "lineSpacing",
            "lineSpacingDistance",
            "lineSpacingStyle",
            "textRotation",
            "textPositionX",
            "prefix",
            "suffix",
            "altPrefix",
            "clear",
            "clearOverrides",
            "DIMTXT",
            "DIMBLK1",
            "DIMCLRT",
            "DIMLWD",
            "DIMLTYPE",
        ] {
            p[k] = v.clone();
        }
        let _ = s.execute("properties.set", &p);
        let _ = s.execute("properties.describe", &json!({ "handles": [t, m, dm] }));
    }
    // Every dimension still draws.
    let g = cadcraft_engine::render::dimension_in(s.doc().unwrap(), &dim(&s, &dm));
    assert!(g.text_height.is_finite());
}
