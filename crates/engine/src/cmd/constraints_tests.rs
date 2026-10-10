use cadcraft_doc::{ConstraintKind, EntityKind, Handle};
use cadcraft_geom::Vec2;
use serde_json::json;

use crate::{Input, Session, command_specs};

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    s.execute("line", &json!({ "points": [a, b] })).unwrap();
    s.doc().unwrap().model.iter().last().unwrap().handle.hex()
}
fn ends(s: &Session, h: &str) -> (Vec2, Vec2) {
    let h = Handle::parse_hex(h).unwrap();
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Line(l) => (l.a.xy(), l.b.xy()),
        _ => panic!(),
    }
}

#[test]
fn json_geometric_and_dimensional_constraints() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.5]);
    let b = line(&mut s, [10.2, 0.3], [10.5, 6.0]);
    s.execute("gchorizontal", &json!({ "h1": a })).unwrap();
    s.execute("gccoincident", &json!({ "h1": a, "h2": b })).unwrap(); // closest end points
    s.execute("geomconstraint", &json!({ "type": "perpendicular", "h1": a, "h2": b })).unwrap();
    let r = s.execute("dcaligned", &json!({ "h1": a, "expr": "8" })).unwrap();
    assert_eq!(r["name"], "d1");
    s.execute("dimconstraint", &json!({ "type": "aligned", "h1": b, "expr": "d1/2" })).unwrap();
    let ((a0, a1), (b0, b1)) = (ends(&s, &a), ends(&s, &b));
    assert!((a0.y - a1.y).abs() < 1e-6);
    assert!(a1.near(b0, 1e-6));
    assert!((a0.dist(a1) - 8.0).abs() < 1e-6 && (b0.dist(b1) - 4.0).abs() < 1e-6);
    // PARAMETERS: change d1, geometry follows; undo restores.
    let r = s.execute("parameters", &json!({ "name": "d1", "expr": "12" })).unwrap();
    assert!(r["message"].as_str().unwrap().contains("d1 = 12"));
    let (b0, b1) = ends(&s, &b);
    assert!((b0.dist(b1) - 6.0).abs() < 1e-6);
    s.undo().unwrap();
    let (b0, b1) = ends(&s, &b);
    assert!((b0.dist(b1) - 4.0).abs() < 1e-6);
    // User parameter.
    s.execute("parameters", &json!({ "name": "len", "expr": "20" })).unwrap();
    s.execute("parameters", &json!({ "name": "d1", "expr": "len/2" })).unwrap();
    assert!((ends(&s, &a).0.dist(ends(&s, &a).1) - 10.0).abs() < 1e-6);
    assert!(s.execute("parameters", &json!({ "delete": "len" })).is_err(), "still used");
    assert!(s.execute("parameters", &json!({ "delete": "d1" })).is_err(), "dimensional");
    // Conflict is reported and changes nothing.
    let before = s.doc().unwrap().clone();
    let e = s.execute("gcvertical", &json!({ "h1": a })).unwrap_err().to_string();
    assert!(e.contains("conflict") || e.contains("over-constrain"), "{e}");
    assert_eq!(*s.doc().unwrap(), before);
    let inspect = s.execute("constraints.inspect", &json!({})).unwrap();
    assert_eq!(inspect["constraints"].as_array().unwrap().len(), 5);
    assert_eq!(inspect["satisfied"], true);
    assert_eq!(inspect["glyphs"][0]["kind"], "Horizontal");
}

#[test]
fn edits_are_resolved_and_erase_purges() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [10.0, 0.0], [10.0, 5.0]);
    s.execute("gccoincident", &json!({ "h1": a, "p1": "end", "h2": b, "p2": "start" })).unwrap();
    s.execute("gcfix", &json!({ "h1": a })).unwrap();
    // Move line b: the coincidence pulls its start back onto a's (fixed) end.
    s.execute("move", &json!({ "handles": [b], "from": [0, 0], "to": [3, 2] })).unwrap();
    let ((_, a1), (b0, _)) = (ends(&s, &a), ends(&s, &b));
    assert!(a1.near(Vec2::new(10.0, 0.0), 1e-9), "{a1:?}");
    assert!(b0.near(a1, 1e-6), "{b0:?}");
    // Erasing a constrained object removes its constraints.
    s.execute("erase", &json!({ "handles": [b] })).unwrap();
    let kinds: Vec<ConstraintKind> = s.doc().unwrap().constraints.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, vec![ConstraintKind::Fix]);
    s.execute("delconstraint", &json!({ "handles": [a] })).unwrap();
    assert!(s.doc().unwrap().constraints.is_empty());
}

#[test]
fn interactive_prompts() {
    let mut s = Session::new();
    s.viewport_px = (1000.0, 1000.0);
    let a = line(&mut s, [0.0, 0.0], [8.0, 0.4]);
    let b = line(&mut s, [0.0, 3.0], [8.0, 3.9]);
    s.cmdline("gchorizontal").unwrap();
    assert!(s.prompt_text().contains("Select an object"));
    s.input(Input::Point(Vec2::new(4.0, 0.2))).unwrap();
    assert!(s.running.is_none());
    s.cmdline("geomconstraint pa").unwrap();
    assert!(s.prompt_text().contains("Select first object"), "{}", s.prompt_text());
    s.input(Input::Point(Vec2::new(4.0, 0.2))).unwrap();
    s.input(Input::Point(Vec2::new(4.0, 3.45))).unwrap();
    assert!(s.running.is_none());
    let ((a0, a1), (b0, b1)) = (ends(&s, &a), ends(&s, &b));
    assert!((a0.y - a1.y).abs() < 1e-6 && (b0.y - b1.y).abs() < 1e-6);
    // DIMCONSTRAINT → aligned → pick → value with a name.
    s.cmdline("dimconstraint al").unwrap();
    s.input(Input::Point(Vec2::new(4.0, a0.y))).unwrap();
    assert!(s.prompt_text().contains("dimension value"), "{}", s.prompt_text());
    s.cmdline("width=5").unwrap();
    assert!(s.running.is_none());
    let (a0, a1) = ends(&s, &a);
    assert!((a0.dist(a1) - 5.0).abs() < 1e-6);
    assert_eq!(s.doc().unwrap().constraints.last().unwrap().name, "width");
    // One undo step per interactive constraint.
    let n = s.doc().unwrap().constraints.len();
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap().constraints.len(), n - 1);
    // Coincident by picking near end points.
    let c = line(&mut s, [20.0, 0.0], [30.0, 0.0]);
    let e = line(&mut s, [30.3, 0.2], [30.0, 9.0]);
    s.cmdline("gccoincident").unwrap();
    s.input(Input::Point(Vec2::new(29.98, 0.0))).unwrap();
    s.input(Input::Point(Vec2::new(30.28, 0.3))).unwrap();
    assert!(ends(&s, &c).1.near(ends(&s, &e).0, 1e-6));
    // AUTOCONSTRAIN with a pickfirst selection.
    let f = line(&mut s, [40.0, 0.0], [50.0, 0.01]);
    s.set_selection(vec![Handle::parse_hex(&f).unwrap()]);
    s.cmdline("autoconstrain").unwrap();
    assert!(s.running.is_none());
    let (f0, f1) = ends(&s, &f);
    assert!((f0.y - f1.y).abs() < 1e-9);
}

#[test]
fn bars_settings_and_autoconstrain_json() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("constraintbar.hideall", &json!({})).unwrap();
    let r = s.execute("constraintbar", &json!({ "mode": "show", "handles": [a] })).unwrap();
    assert_eq!(r["visible"], false);
    assert_eq!(r["exceptions"][0], a);
    s.execute("dcdisplay.showall", &json!({})).unwrap();
    assert!(s.execute("constraintbar", &json!({ "mode": "bogus", "handles": [a] })).is_err());
    let r = s.execute("constraintsettings", &json!({ "angleTolerance": 2.5, "autoTypes": ["Horizontal", "nope"], "infer": true })).unwrap();
    assert_eq!(r["angleTolerance"], 2.5);
    assert_eq!(r["autoTypes"], json!(["Horizontal"]));
    let b = line(&mut s, [0.0, 5.0], [0.01, 9.0]);
    let r = s.execute("autoconstrain", &json!({ "handles": [a, b] })).unwrap();
    // Only Horizontal is enabled: a is horizontal; b (near vertical) is left alone.
    assert_eq!(r["added"], 1);
    assert_eq!(s.doc().unwrap().constraints[0].kind, ConstraintKind::Horizontal);
}

#[test]
fn auto_types_spellings_are_stored_canonically() {
    for spelling in ["gcParallel", " Parallel ", "parallel"] {
        let mut s = Session::new();
        let a = line(&mut s, [0.0, 0.0], [10.0, 10.0]);
        let b = line(&mut s, [0.0, 3.0], [10.0, 13.01]);
        let r = s.execute("constraintsettings", &json!({ "autoTypes": [spelling], "angleTolerance": 1 })).unwrap();
        assert_eq!(r["autoTypes"], json!(["Parallel"]));
        let r = s.execute("autoconstrain", &json!({ "handles": [a, b] })).unwrap();
        assert_eq!(r["added"], 1, "{spelling}");
        assert_eq!(s.doc().unwrap().constraints[0].kind, ConstraintKind::Parallel);
        // The per-command `types` override accepts the same spellings.
        let mut s = Session::new();
        let a = line(&mut s, [0.0, 0.0], [10.0, 10.0]);
        let b = line(&mut s, [0.0, 3.0], [10.0, 13.01]);
        let r = s.execute("autoconstrain", &json!({ "handles": [a, b], "types": [spelling], "angleTolerance": 1 })).unwrap();
        assert_eq!(r["added"], 1, "{spelling}");
        assert_eq!(s.doc().unwrap().constraints[0].kind, ConstraintKind::Parallel);
    }
}

#[test]
fn hostile_constraint_params_never_panic() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let c = {
        s.execute("circle", &json!({ "center": [0, 5], "radius": 2 })).unwrap();
        s.doc().unwrap().model.iter().last().unwrap().handle.hex()
    };
    let hostile = [
        json!({ "h1": a, "h2": a }),
        json!({ "h1": a, "p1": "v999999", "h2": c, "p2": [1e308, -1e308] }),
        json!({ "h1": a, "p1": "zzz" }),
        json!({ "h1": c, "h2": c, "h3": c }),
        json!({ "h1": "ffffffffffffffff", "h2": 1 }),
        json!({ "h1": a, "expr": "((((" }),
        json!({ "h1": a, "expr": "1/0" }),
        json!({ "h1": a, "value": -5 }),
        json!({ "h1": c, "expr": "1e309" }),
        json!({ "h1": a, "name": "bad name", "expr": "3" }),
        json!({ "type": "nope", "h1": a }),
        json!({ "name": "x", "expr": "x+1" }),
        json!({ "name": "", "expr": "1" }),
        json!({ "delete": "nothing" }),
        json!({ "ids": [1, -1, "a", 1e20] }),
        json!({ "mode": "show" }),
        json!({ "tolerance": -1, "angleTolerance": 1e308, "handles": [a, c] }),
    ];
    let ids: Vec<&str> = command_specs()
        .iter()
        .map(|c| c.id)
        .filter(|id| id.starts_with("gc") || id.starts_with("dc") || id.contains("constrain") || *id == "parameters" || id.starts_with("constraint"))
        .collect();
    assert!(ids.len() >= 25, "{ids:?}");
    for id in ids {
        for p in &hostile {
            let _ = s.execute(id, p);
        }
        // Interactive: start, junk inputs, cancel.
        s.start(id).unwrap();
        for i in
            [Input::Point(Vec2::new(5.0, 0.0)), Input::Text("x".into()), Input::Point(Vec2::new(0.0, 7.0)), Input::Enter, Input::Text("1/0".into())]
        {
            let _ = s.input(i);
        }
        s.cancel();
        assert!(s.running.is_none());
    }
}

#[test]
fn delconstraint_rejects_referenced_dimension() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [0.0, 5.0], [4.0, 5.0]);
    s.execute("dimconstraint", &json!({ "type": "horizontal", "h1": a, "expr": "8" })).unwrap();
    s.execute("dimconstraint", &json!({ "type": "horizontal", "h1": b, "expr": "d1/2" })).unwrap();
    let before = s.doc().unwrap().clone();
    let first = s.doc().unwrap().constraints[0].id;
    let e = s.execute("delconstraint", &json!({ "ids": [first] })).unwrap_err().to_string();
    assert!(e.contains("`d1` is used by `d2`"), "{e}");
    assert_eq!(*s.doc().unwrap(), before);
    assert!(s.execute("delconstraint", &json!({ "handles": [a] })).is_err());
    assert_eq!(*s.doc().unwrap(), before);
    // Selecting both dimensions removes both; the dependent one may go with its source.
    let ids: Vec<u32> = s.doc().unwrap().constraints.iter().map(|c| c.id).collect();
    s.execute("delconstraint", &json!({ "ids": ids })).unwrap();
    assert!(s.doc().unwrap().constraints.is_empty());
}
