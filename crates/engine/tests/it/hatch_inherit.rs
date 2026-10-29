//! Gap F13: HATCH's `inherit` and Match properties keyword, the layer and transparency of new
//! hatches (HPLAYER, HPTRANSPARENCY), and HATCHPICK's Inherit and Preview (the Hatch and Gradient
//! dialog's Inherit Properties and Preview).

use cadcraft_engine::doc::{EntityKind, Handle, Hatch, Transparency};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, PickMode, Session};
use serde_json::{Value, json};

fn handle(v: &Value) -> String {
    v["handle"].as_str().unwrap().to_string()
}

fn entity(s: &Session, h: &str) -> cadcraft_engine::doc::Entity {
    (**s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap()).clone()
}

fn hatch(s: &Session, h: &str) -> Hatch {
    match entity(s, h).kind {
        EntityKind::Hatch(x) => x,
        k => panic!("not a hatch: {k:?}"),
    }
}

fn hatches(s: &Session) -> Vec<(String, Hatch)> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some((e.handle.hex(), h.clone())) } else { None }).collect()
}

/// What the running command previews.
fn kinds(s: &Session) -> Vec<EntityKind> {
    s.preview(Vec2::ZERO).into_iter().map(|e| e.kind).collect()
}

fn rect(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    handle(&s.execute("rectang", &json!({ "p1": a, "p2": b })).unwrap())
}

/// A source hatch: BRICK at scale 2, 30°, outer style, on layer "Walls" at 40% transparency,
/// with its own origin.
fn source(s: &mut Session) -> String {
    s.execute("layer.new", &json!({ "name": "Walls" })).unwrap();
    let r = rect(s, [0.0, 0.0], [10.0, 10.0]);
    let p = json!({ "handles": [r], "pattern": "BRICK", "scale": 2, "angle": 30, "style": "outer", "layer": "Walls", "transparency": 40, "origin": [1, 2] });
    handle(&s.execute("hatch", &p).unwrap())
}

#[test]
fn inherit_copies_pattern_scale_angle_style_layer_transparency_and_associativity() {
    let mut s = Session::new();
    let src = source(&mut s);
    let e = entity(&s, &src);
    assert_eq!((e.common.layer.as_str(), e.common.transparency), ("Walls", Transparency::Percent(40)));
    rect(&mut s, [20.0, 0.0], [30.0, 10.0]);
    let h = handle(&s.execute("hatch", &json!({ "points": [[25, 5]], "inherit": src })).unwrap());
    let (x, e) = (hatch(&s, &h), entity(&s, &h));
    assert_eq!((x.pattern.as_str(), x.scale, x.style, x.associative), ("BRICK", 2.0, 1, true));
    assert!((x.angle.to_degrees() - 30.0).abs() < 1e-9);
    assert_eq!((e.common.layer.as_str(), e.common.transparency), ("Walls", Transparency::Percent(40)));
    // HPINHERIT 0: the current origin, not the source's.
    assert!(x.origin.dist(Vec2::new(1.0, 2.0)) > 1e-6);
    // Given parameters win; the source's origin with inheritOrigin (or HPINHERIT 1).
    let h2 = handle(&s.execute("hatch", &json!({ "points": [[25, 5]], "inherit": src, "scale": 3, "inheritOrigin": true })).unwrap());
    let x2 = hatch(&s, &h2);
    assert_eq!((x2.scale, x2.origin), (3.0, Vec2::new(1.0, 2.0)));
    // A non-associative source gives a non-associative hatch.
    s.execute("hatchedit", &json!({ "handles": [src.clone()], "associative": false })).unwrap();
    let h3 = handle(&s.execute("hatch", &json!({ "points": [[25, 5]], "inherit": src })).unwrap());
    assert!(!hatch(&s, &h3).associative);
    // Hostile or wrong handles are refused and change nothing.
    let n = s.doc().unwrap().model.len();
    for bad in [json!("zzz"), json!(5), json!(h3.clone() + "0"), json!(handle(&json!({ "handle": rect(&mut s, [50.0, 0.0], [51.0, 1.0]) })))] {
        let n = s.doc().unwrap().model.len();
        assert!(s.execute("hatch", &json!({ "points": [[25, 5]], "inherit": bad })).is_err(), "{bad}");
        assert_eq!(s.doc().unwrap().model.len(), n);
    }
    assert!(s.doc().unwrap().model.len() > n);
}

#[test]
fn inheriting_a_gradient_makes_a_gradient() {
    let mut s = Session::new();
    rect(&mut s, [0.0, 0.0], [10.0, 10.0]);
    let g = handle(&s.execute("gradient", &json!({ "points": [[5, 5]], "name": "CYLINDER", "color1": "1", "color2": "3", "angle": 45 })).unwrap());
    rect(&mut s, [20.0, 0.0], [30.0, 10.0]);
    let h = handle(&s.execute("hatch", &json!({ "points": [[25, 5]], "inherit": g })).unwrap());
    let gr = hatch(&s, &h).gradient.expect("a gradient");
    assert_eq!(gr.name, "CYLINDER");
    assert!((gr.angle.to_degrees() - 45.0).abs() < 1e-9);
    // With a pattern given it is a pattern hatch.
    let h2 = handle(&s.execute("hatch", &json!({ "points": [[25, 5]], "inherit": g, "pattern": "NET" })).unwrap());
    assert!(hatch(&s, &h2).gradient.is_none());
}

#[test]
fn hatch_match_properties_takes_the_source_for_the_next_hatch() {
    let mut s = Session::new();
    let src = source(&mut s);
    rect(&mut s, [20.0, 0.0], [30.0, 10.0]);
    s.start("hatch").unwrap();
    assert!(s.current_prompt().unwrap().keywords.iter().any(|k| k == "Match properties"));
    s.input(Input::Keyword("Match properties".into())).unwrap();
    assert_eq!(s.prompt_text(), "HATCH Select hatch object or [Use current origin/use Source hatch origin]:");
    s.input(Input::Keyword("use Source hatch origin".into())).unwrap();
    s.input(Input::Pick(vec![Handle::parse_hex(&src).unwrap()])).unwrap();
    assert!(s.prompt_text().starts_with("HATCH Pick internal point"), "{}", s.prompt_text());
    s.input(Input::Point(Vec2::new(25.0, 5.0))).unwrap();
    s.input(Input::Enter).unwrap();
    let (h, x) = hatches(&s).into_iter().find(|(h, _)| *h != src).unwrap();
    assert_eq!((x.pattern.as_str(), x.scale, x.origin), ("BRICK", 2.0, Vec2::new(1.0, 2.0)));
    assert_eq!(entity(&s, &h).common.layer, "Walls");
    // The header now holds the source's settings (the dialog and next HATCH start from them).
    let d = s.doc().unwrap();
    assert_eq!((d.header.str("HPNAME", ""), d.header.str("HPLAYER", ""), d.header.i64("HPINHERIT", 0)), ("BRICK".into(), "Walls".into(), 1));
}

#[test]
fn hatchpick_preview_shows_the_hatch_and_changes_nothing() {
    let mut s = Session::new();
    let r = rect(&mut s, [0.0, 0.0], [10.0, 10.0]);
    let before = s.doc().unwrap().clone();
    let out = s.execute("hatchpick", &json!({ "preview": { "command": "hatch", "params": { "points": [[5, 5]], "pattern": "SQUARE" } } })).unwrap();
    assert_eq!(out["preview"], 1);
    assert_eq!(*s.doc().unwrap(), before, "nothing added");
    // The interactive preview shows it; Enter accepts, a pick goes back.
    s.start("hatchpick").unwrap();
    s.input(Input::Keyword("preView".into())).unwrap();
    assert!(s.prompt_text().contains("Right-click to accept hatch"));
    assert!(matches!(kinds(&s).first(), Some(EntityKind::Hatch(h)) if h.pattern == "SQUARE"));
    s.input(Input::Enter).unwrap();
    let p = s.hatch_picks.clone().unwrap();
    assert_eq!((p.mode, p.accepted), (PickMode::Preview, true));
    assert_eq!(*s.doc().unwrap(), before);
    // Edit mode previews the edited hatch; a bad call is refused and changes nothing.
    let h = handle(&s.execute("hatch", &json!({ "handles": [r] })).unwrap());
    let before = s.doc().unwrap().clone();
    let out = s.execute("hatchpick", &json!({ "preview": { "command": "hatchedit", "params": { "handles": [h], "pattern": "NET" } } })).unwrap();
    assert_eq!(out["preview"], 1);
    assert!(matches!(&s.hatch_picks.as_ref().unwrap().preview[0], EntityKind::Hatch(x) if x.pattern == "NET"));
    for bad in [json!({ "command": "erase", "params": {} }), json!({ "command": "hatch", "params": { "points": [[500, 500]] } }), json!(7)] {
        assert!(s.execute("hatchpick", &json!({ "preview": bad })).is_err(), "{bad}");
        assert_eq!(*s.doc().unwrap(), before);
    }
}

#[test]
fn hatchpick_inherit_keeps_the_picked_hatch() {
    let mut s = Session::new();
    let r = rect(&mut s, [0.0, 0.0], [10.0, 10.0]);
    let h = handle(&s.execute("hatch", &json!({ "handles": [r.clone()] })).unwrap());
    s.start("hatchpick").unwrap();
    s.input(Input::Keyword("Inherit".into())).unwrap();
    assert_eq!(s.prompt_text(), "HATCHPICK Select hatch object:");
    s.input(Input::Pick(vec![Handle::parse_hex(&r).unwrap()])).unwrap();
    assert!(s.running.is_some(), "not a hatch: asks again");
    s.input(Input::Pick(vec![Handle::parse_hex(&h).unwrap()])).unwrap();
    let p = s.hatch_picks.clone().unwrap();
    assert_eq!((p.mode, p.handles), (PickMode::Inherit, vec![Handle::parse_hex(&h).unwrap()]));
}

#[test]
fn layer_and_transparency_of_new_hatches_and_hatchedit() {
    let mut s = Session::new();
    s.execute("layer.new", &json!({ "name": "Walls" })).unwrap();
    rect(&mut s, [0.0, 0.0], [10.0, 10.0]);
    // Given values win over HPLAYER / HPTRANSPARENCY; bad ones are refused and add nothing.
    let n = s.doc().unwrap().model.len();
    for bad in [json!({ "layer": "NoSuch" }), json!({ "transparency": 95 }), json!({ "transparency": "lots" }), json!({ "transparency": [1] })] {
        let mut p = bad.clone();
        p["points"] = json!([[5, 5]]);
        assert!(s.execute("hatch", &p).is_err(), "{bad}");
        assert_eq!(s.doc().unwrap().model.len(), n, "{bad}");
    }
    let h = handle(&s.execute("hatch", &json!({ "points": [[5, 5]], "layer": "Walls", "transparency": "50%" })).unwrap());
    let e = entity(&s, &h);
    assert_eq!((e.common.layer.as_str(), e.common.transparency), ("Walls", Transparency::Percent(50)));
    // HPLAYER and HPTRANSPARENCY set the next hatches; values that name nothing fall back to the
    // current layer and transparency.
    s.execute("setvar", &json!({ "name": "HPLAYER", "value": "Walls" })).unwrap();
    s.execute("setvar", &json!({ "name": "HPTRANSPARENCY", "value": "ByBlock" })).unwrap();
    let h = handle(&s.execute("gradient", &json!({ "points": [[5, 5]] })).unwrap());
    let e = entity(&s, &h);
    assert_eq!((e.common.layer.as_str(), e.common.transparency), ("Walls", Transparency::ByBlock));
    s.execute("setvar", &json!({ "name": "HPTRANSPARENCY", "value": "nonsense" })).unwrap();
    s.execute("setvar", &json!({ "name": "HPLAYER", "value": "NoSuchLayer" })).unwrap();
    let h = handle(&s.execute("hatch", &json!({ "points": [[5, 5]] })).unwrap());
    let e = entity(&s, &h);
    assert_eq!((e.common.layer.as_str(), e.common.transparency), ("0", Transparency::ByLayer), "unknown values fall back to the current ones");
    // "Use current" given: the current ones, whatever HPLAYER says.
    s.execute("setvar", &json!({ "name": "HPLAYER", "value": "Walls" })).unwrap();
    let h = handle(&s.execute("hatch", &json!({ "points": [[5, 5]], "layer": "." })).unwrap());
    assert_eq!(entity(&s, &h).common.layer, "0");
    // HATCHEDIT moves a hatch to a layer and sets its transparency; bad values change nothing.
    s.execute("hatchedit", &json!({ "handles": [h.clone()], "layer": "Walls", "transparency": 30 })).unwrap();
    let e = entity(&s, &h);
    assert_eq!((e.common.layer.as_str(), e.common.transparency), ("Walls", Transparency::Percent(30)));
    assert!(s.execute("hatchedit", &json!({ "handles": [h.clone()], "layer": "NoSuch" })).is_err());
    assert!(s.execute("hatchedit", &json!({ "handles": [h.clone()], "transparency": -5 })).is_err());
    assert_eq!(entity(&s, &h).common.transparency, Transparency::Percent(30));
}
