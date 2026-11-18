//! Annotative scaling (#468): annotative objects keep their paper size at every annotation
//! scale, show only at scales they support unless ANNOALLVISIBLE is 1, and the scale list,
//! OBJECTSCALE, ANNOAUTOSCALE and ANNOUPDATE manage the scales.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{AnnoDraw, Drawing, Entity, EntityKind, Handle, Space, entity_bounds};
use serde_json::{Value, json};

fn setvar(s: &mut Session, name: &str, value: Value) {
    s.execute("setvar", &json!({ "name": name, "value": value })).unwrap();
}

/// A session whose current text style is annotative, with one text 0.25 high on paper.
fn with_text() -> (Session, Handle) {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Notes", "annotative": true, "current": true })).unwrap();
    let r = s.execute("text", &json!({ "at": [0, 0], "text": "NOTE", "height": 0.25 })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    (s, h)
}

fn entity(s: &Session, h: Handle) -> Entity {
    (**s.doc().unwrap().entity(h).unwrap()).clone()
}

fn scales(s: &Session, h: Handle) -> Vec<String> {
    entity(s, h).common.anno_scales.iter().map(|x| x.name.clone()).collect()
}

fn height(d: &Drawing, h: Handle) -> f64 {
    let e = d.entity(h).unwrap();
    entity_bounds(d, e, 0).height()
}

/// Height of what the renderer draws for `h` in model space.
fn drawn_height(d: &Drawing, h: Handle) -> Option<f64> {
    let list = cadcraft_engine::render::build(d, &Space::Model, &Default::default());
    let mut b = cadcraft_engine::geom::Bounds2::EMPTY;
    for p in list.prims.iter().filter(|p| p.handle == h) {
        for q in list.points(p) {
            b.add(*q);
        }
    }
    (!b.is_empty()).then(|| b.height())
}

#[test]
fn text_of_an_annotative_style_supports_the_current_scale() {
    let (s, h) = with_text();
    assert_eq!(scales(&s, h), ["1:1"]);
    let mut s = s;
    let p = s.execute("properties", &json!({ "handles": [h.hex()] })).unwrap();
    assert_eq!(p["objects"][0]["annotative"], json!(true), "{p}");
    assert_eq!(p["objects"][0]["annotativeScales"], json!(["1:1"]), "{p}");
    // A plain style makes plain text.
    let mut s = Session::new();
    let r = s.execute("text", &json!({ "at": [0, 0], "text": "X", "height": 0.25 })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    assert!(scales(&s, h).is_empty());
}

#[test]
fn annotative_text_draws_at_paper_height_times_the_scale() {
    let (mut s, h) = with_text();
    let at_1 = drawn_height(s.doc().unwrap(), h).unwrap();
    setvar(&mut s, "CANNOSCALE", json!("1:50"));
    s.execute("objectscale", &json!({ "handles": [h.hex()], "add": "1:50" })).unwrap();
    assert_eq!(scales(&s, h), ["1:1", "1:50"]);
    let at_50 = drawn_height(s.doc().unwrap(), h).unwrap();
    assert!((at_50 / at_1 - 50.0).abs() < 1e-6, "{at_1} → {at_50}");
    // Bounds (zoom, selection) follow the drawn size; the stored height stays the paper height.
    assert!((height(s.doc().unwrap(), h) / at_50 - 1.0).abs() < 0.5);
    match entity(&s, h).kind {
        EntityKind::Text(t) => assert_eq!(t.height, 0.25),
        k => panic!("{k:?}"),
    }
}

#[test]
fn unsupported_scale_hides_unless_annoallvisible() {
    let (mut s, h) = with_text();
    setvar(&mut s, "CANNOSCALE", json!("1:20"));
    // ANNOALLVISIBLE 1 (the default): shown, at the object's own scale.
    assert_eq!(s.doc().unwrap().anno_draw(&entity(&s, h)), AnnoDraw::Scaled(1.0));
    assert!(drawn_height(s.doc().unwrap(), h).is_some());
    s.execute("annovisible.current", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().header.i64("ANNOALLVISIBLE", 1), 0);
    assert_eq!(drawn_height(s.doc().unwrap(), h), None);
    assert!(entity_bounds(s.doc().unwrap(), &entity(&s, h), 0).is_empty());
    // Back at a supported scale it shows again.
    setvar(&mut s, "CANNOSCALE", json!("1:1"));
    assert!(drawn_height(s.doc().unwrap(), h).is_some());
    s.execute("annovisible.toggle", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().header.i64("ANNOALLVISIBLE", 0), 1);
}

#[test]
fn annoautoscale_adds_the_new_scale() {
    let (mut s, h) = with_text();
    setvar(&mut s, "ANNOAUTOSCALE", json!(4));
    setvar(&mut s, "CANNOSCALE", json!("1:10"));
    assert_eq!(scales(&s, h), ["1:1", "1:10"]);
    let v = s.execute("getvar", &json!({ "name": "CANNOSCALEVALUE" })).unwrap();
    assert!((v["value"].as_f64().unwrap() - 0.1).abs() < 1e-12, "{v}");
    assert!(s.execute("setvar", &json!({ "name": "CANNOSCALEVALUE", "value": 1 })).is_err());
    setvar(&mut s, "ANNOAUTOSCALE", json!(-4));
    setvar(&mut s, "CANNOSCALE", json!("1:20"));
    assert_eq!(scales(&s, h), ["1:1", "1:10"]);
    // A scale that isn't in the list is refused.
    assert!(s.execute("setvar", &json!({ "name": "CANNOSCALE", "value": "1:7" })).is_err());
    assert!(s.execute("setvar", &json!({ "name": "CANNOSCALE", "value": 7 })).is_err());
}

#[test]
fn add_and_delete_current_scale_keep_one_scale() {
    let (mut s, h) = with_text();
    setvar(&mut s, "CANNOSCALE", json!("1:4"));
    s.execute("aiobjectscaleadd", &json!({ "handles": [h.hex()] })).unwrap();
    assert_eq!(scales(&s, h), ["1:1", "1:4"]);
    s.execute("aiobjectscaleremove", &json!({ "handles": [h.hex()] })).unwrap();
    assert_eq!(scales(&s, h), ["1:1"]);
    // The last scale stays.
    setvar(&mut s, "CANNOSCALE", json!("1:1"));
    let r = s.execute("aiobjectscaleremove", &json!({ "handles": [h.hex()] })).unwrap();
    assert_eq!(scales(&s, h), ["1:1"]);
    assert!(r["message"].as_str().unwrap().contains("only scale"), "{r}");
    assert!(s.execute("objectscale", &json!({ "handles": [h.hex()], "add": "no such" })).is_err());
}

#[test]
fn objectscale_at_the_command_line() {
    let (mut s, h) = with_text();
    // The pickfirst selection is used.
    s.set_selection(vec![h]);
    s.script("-OBJECTSCALE\nA\n1:2\nA\n1:8\nD\n1:1\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(scales(&s, h), ["1:2", "1:8"]);
}

#[test]
fn scale_list_add_delete_reset() {
    let (mut s, h) = with_text();
    let r = s.execute("scalelistedit", &json!({ "add": { "name": "Site", "ratio": "1:250" } })).unwrap();
    assert!(r["scales"].as_array().unwrap().iter().any(|x| x["name"] == "Site" && x["drawingUnits"] == 250.0), "{r}");
    assert!(s.execute("scalelistedit", &json!({ "add": { "name": "site", "ratio": "1:3" } })).is_err());
    assert!(s.execute("scalelistedit", &json!({ "add": { "name": "Bad", "ratio": "0:3" } })).is_err());
    assert!(s.execute("scalelistedit", &json!({ "add": { "name": "Bad", "paperUnits": 1, "drawingUnits": -2 } })).is_err());
    // In use by the text, or current: can't be deleted.
    s.execute("objectscale", &json!({ "handles": [h.hex()], "add": "Site" })).unwrap();
    assert!(s.execute("scalelistedit", &json!({ "delete": "Site" })).is_err());
    assert!(s.execute("scalelistedit", &json!({ "delete": "1:1" })).is_err());
    s.execute("scalelistedit", &json!({ "delete": "1:100" })).unwrap();
    assert!(s.doc().unwrap().anno_scale("1:100").is_none());
    // Reset brings the defaults back and keeps scales in use.
    let r = s.execute("scalelistedit", &json!({ "reset": "metric" })).unwrap();
    let names: Vec<&str> = r["scales"].as_array().unwrap().iter().filter_map(|x| x["name"].as_str()).collect();
    assert!(names.contains(&"1:100") && names.contains(&"Site") && !names.iter().any(|n| n.contains('"')), "{names:?}");
    let r = s.execute("scalelistedit", &json!({ "reset": "imperial" })).unwrap();
    assert!(r["scales"].as_array().unwrap().iter().any(|x| x["name"] == "1/4\" = 1'-0\"" && x["drawingUnits"] == 12.0));
}

#[test]
fn scalelistedit_at_the_command_line() {
    let mut s = Session::new();
    s.script("-SCALELISTEDIT\nA\nDetail\n5:1\nD\n1:16\nE\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let d = s.doc().unwrap();
    let detail = d.anno_scale("detail").unwrap();
    assert_eq!((detail.paper_units, detail.drawing_units), (5.0, 1.0));
    assert!(d.anno_scale("1:16").is_none());
    // Bad input is reported, not fatal.
    s.script("-SCALELISTEDIT\nA\nOdd\nfoo\n1:3\nD\nnothing\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(s.doc().unwrap().anno_scale("Odd").map(|x| x.drawing_units), Some(3.0));
    s.script("-SCALELISTEDIT\nR\nY\nX\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(s.doc().unwrap().anno_scale("1:16").is_some(), "reset restores the defaults");
    assert!(s.doc().unwrap().anno_scale("Odd").is_none(), "an unused scale goes");
}

#[test]
fn annotative_dimension_scales_its_text_and_arrows() {
    let mut s = Session::new();
    s.execute("dimstyle", &json!({ "name": "Anno", "annotative": true, "current": true })).unwrap();
    let r = s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [100, 0], "at": [50, 20] })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap()).unwrap();
    assert_eq!(scales(&s, h), ["1:1"]);
    let at_1 = drawn_height(s.doc().unwrap(), h).unwrap();
    s.execute("scalelistedit", &json!({ "add": { "name": "1:3", "ratio": "1:3" } })).ok();
    setvar(&mut s, "ANNOAUTOSCALE", json!(1));
    setvar(&mut s, "CANNOSCALE", json!("1:4"));
    let at_4 = drawn_height(s.doc().unwrap(), h).unwrap();
    // The dimension stays 20 tall between its points and dimension line; the extension line
    // offset and overshoot (DIMEXO, DIMEXE) are 4× their paper size.
    assert!(((at_4 - 20.0) / (at_1 - 20.0) - 4.0).abs() < 1e-6, "{at_1} → {at_4}");
}

#[test]
fn annotative_multileader_and_hatch() {
    let mut s = Session::new();
    s.execute("mleaderstyle", &json!({ "name": "Anno", "annotative": true })).unwrap();
    let r = s.execute("mleader", &json!({ "points": [[0, 0], [5, 5]], "text": "A" })).unwrap();
    let ml = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    assert_eq!(scales(&s, ml), ["1:1"]);
    setvar(&mut s, "HPANNOTATIVE", json!(1));
    s.execute("rectang", &json!({ "p1": [20, 0], "p2": [30, 10] })).unwrap();
    let r = s.execute("hatch", &json!({ "points": [[25, 5]], "pattern": "ANSI31" })).unwrap();
    let hh = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).and_then(Handle::parse_hex).unwrap();
    assert_eq!(scales(&s, hh), ["1:1"]);
    // At 1:2 the hatch pattern doubles: fewer, longer lines over the same area.
    let count = |s: &Session| {
        let d = s.doc().unwrap();
        let list = cadcraft_engine::render::build(d, &Space::Model, &Default::default());
        list.prims.iter().filter(|p| p.handle == hh).count()
    };
    let n1 = count(&s);
    setvar(&mut s, "ANNOAUTOSCALE", json!(1));
    setvar(&mut s, "CANNOSCALE", json!("1:2"));
    let n2 = count(&s);
    assert!(n2 < n1, "{n1} → {n2}");
}

#[test]
fn explode_gives_plain_objects_at_the_current_scale() {
    let (mut s, h) = with_text();
    s.execute("objectscale", &json!({ "handles": [h.hex()], "add": "1:10" })).unwrap();
    setvar(&mut s, "CANNOSCALE", json!("1:10"));
    // Text doesn't explode; a block reference of an annotative block does.
    s.execute("block", &json!({ "name": "TAG", "base": [0, 0], "handles": [h.hex()], "annotative": true })).unwrap();
    let ins = s.doc().unwrap().model.last().unwrap().handle;
    assert_eq!(scales(&s, ins), ["1:10"]);
    s.execute("explode", &json!({ "handles": [ins.hex()] })).unwrap();
    let d = s.doc().unwrap();
    let t = d.model.iter().find(|e| matches!(e.kind, EntityKind::Text(_))).unwrap();
    match &t.kind {
        EntityKind::Text(t) => assert!((t.height - 2.5).abs() < 1e-9, "{}", t.height),
        _ => unreachable!(),
    }
}

#[test]
fn annoupdate_and_properties_follow_the_style() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Notes", "current": true })).unwrap();
    let r = s.execute("text", &json!({ "at": [0, 0], "text": "N", "height": 0.25 })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    assert!(scales(&s, h).is_empty());
    s.execute("style", &json!({ "name": "Notes", "annotative": true })).unwrap();
    s.execute("annoupdate", &json!({ "handles": [h.hex()] })).unwrap();
    assert_eq!(scales(&s, h), ["1:1"]);
    s.execute("properties.set", &json!({ "handles": [h.hex()], "annotative": false })).unwrap();
    assert!(scales(&s, h).is_empty());
    s.execute("properties.set", &json!({ "handles": [h.hex()], "annotative": true })).unwrap();
    assert_eq!(scales(&s, h), ["1:1"]);
    let r = s.execute("annoreset", &json!({ "handles": [h.hex()] })).unwrap();
    assert_eq!(r["objects"], 1);
    // Lines can't be annotative.
    let r = s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap();
    let l = r["handles"][0].as_str().or_else(|| r["handle"].as_str()).and_then(Handle::parse_hex).unwrap();
    s.execute("properties.set", &json!({ "handles": [l.hex()], "annotative": true })).unwrap();
    assert!(scales(&s, l).is_empty());
}

#[test]
fn hostile_parameters_are_refused() {
    let (mut s, h) = with_text();
    for p in [
        json!({ "handles": [h.hex()], "add": 5 }),
        json!({ "handles": [h.hex()], "delete": [1, 2] }),
        json!({ "handles": "x" }),
        json!({ "handles": [] }),
    ] {
        assert!(s.execute("objectscale", &p).is_err(), "{p}");
    }
    for p in [
        json!({ "add": { "name": "", "ratio": "1:2" } }),
        json!({ "add": { "name": "x", "ratio": "1:inf" } }),
        json!({ "add": { "name": "x", "paperUnits": 1e308, "drawingUnits": 1e-308 } }),
        json!({ "add": "1:2" }),
        json!({ "reset": 3 }),
        json!({ "delete": "missing" }),
    ] {
        assert!(s.execute("scalelistedit", &p).is_err(), "{p}");
    }
}

#[test]
fn annotative_mtext_columns_follow_the_scale() {
    // #708: at 1:2, two columns 10 wide with a gutter of 1 and a height of 20 on paper draw 20
    // wide, 2 apart and 40 tall: the box is 42 × 40, not 41 × 20.
    let mut s = Session::new();
    setvar(&mut s, "CANNOSCALE", json!("1:2"));
    let r = s
        .execute(
            "mtext",
            &json!({ "at": [0, 0], "height": 1, "width": 10, "text": "a\\Pb\\Pc", "columns": { "type": "static", "count": 2, "gutter": 1, "height": 20 } }),
        )
        .unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    s.execute("properties.set", &json!({ "handles": [h.hex()], "annotative": true })).unwrap();
    assert_eq!(scales(&s, h), ["1:2"]);
    let v = |s: &mut Session, name: &str| s.execute("getvar", &json!({ "name": name })).unwrap()["value"].clone();
    assert_eq!(v(&mut s, "EXTMIN"), json!([0.0, -40.0, 0.0]));
    assert_eq!(v(&mut s, "EXTMAX"), json!([42.0, 0.0, 0.0]));
    // Dynamic columns with manual heights, and the text of a multileader, scale the same way.
    let mut e = entity(&s, h);
    if let EntityKind::MText(t) = &mut e.kind
        && let Some(c) = &mut t.columns
    {
        c.heights = vec![3.0, 5.0];
    }
    let mtext = match &e.kind {
        EntityKind::MText(t) => t.clone(),
        k => panic!("{k:?}"),
    };
    let ml = Entity {
        kind: EntityKind::MLeader(cadcraft_engine::doc::MLeader {
            leaders: Vec::new(),
            landing: Default::default(),
            dogleg: 1.0,
            text: Some(mtext),
            style: "Standard".into(),
            arrow_size: 1.0,
            spline: false,
            block: None,
        }),
        ..e.clone()
    };
    for e in [e, ml] {
        let t = match cadcraft_engine::doc::anno_scaled(&e, 2.0).kind {
            EntityKind::MText(t) => t,
            EntityKind::MLeader(m) => m.text.unwrap(),
            k => panic!("{k:?}"),
        };
        let c = t.columns.unwrap();
        assert_eq!((t.height, t.width, c.count, c.gutter, c.height, c.heights), (2.0, 20.0, 2, 2.0, 40.0, vec![6.0, 10.0]));
    }
}
