//! CALIBRATE: two points and a true length scale a drawing (#1181), the discoverable front end
//! for SCALE ▸ Reference with an apply target (everything, the selection, an underlay).

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use serde_json::json;

/// The first handle of a LINE the command just drew.
fn handle(v: &serde_json::Value) -> Handle {
    Handle::parse_hex(v["handles"][0].as_str().unwrap()).unwrap()
}

/// A line's two ends.
fn ends(s: &Session, h: Handle) -> (Vec2, Vec2) {
    let d = s.doc().unwrap();
    let e = d.entity(h).expect("the object is in the drawing");
    let EntityKind::Line(l) = &e.kind else { panic!("expected a line") };
    (l.a.xy(), l.b.xy())
}

/// Compare two points at the tolerance the CALIBRATE tests accept, so valid scaling arithmetic is
/// compared without exact-float brittleness.
fn near(a: Vec2, b: Vec2) -> bool {
    a.near(b, 1e-9)
}

/// The JSON form with no `apply` scales every object about `p1`, so the measured segment becomes
/// exactly `length` and objects elsewhere move by the same factor.
#[test]
fn json_scales_everything_by_the_true_length() {
    let mut s = Session::new();
    let a = handle(&s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap());
    let b = handle(&s.execute("line", &json!({"points": [[10, 0], [10, 5]]})).unwrap());

    // No `apply`: everything is scaled about the first calibration point.
    let r = s.execute("calibrate", &json!({"p1": [0, 0], "p2": [5, 0], "length": 1250})).unwrap();
    assert_eq!(r["apply"], "all", "{r}");
    assert_eq!(r["scaled"], 2, "{r}");
    assert!((r["factor"].as_f64().unwrap() - 250.0).abs() < 1e-9, "{r}");
    assert!((r["measured"].as_f64().unwrap() - 5.0).abs() < 1e-9, "{r}");
    assert!((r["length"].as_f64().unwrap() - 1250.0).abs() < 1e-9, "{r}");

    // The measured segment is now 1250 long; the other object scaled with it.
    let (a1, a2) = ends(&s, a);
    assert!(near(a1, Vec2::ZERO) && near(a2, Vec2::new(1250.0, 0.0)), "{a1:?} {a2:?}");
    let (b1, b2) = ends(&s, b);
    assert!(near(b1, Vec2::new(2500.0, 0.0)) && near(b2, Vec2::new(2500.0, 1250.0)), "{b1:?} {b2:?}");
}

/// `apply: "selection"` scales only the named handles, and an explicit empty selection is an error
/// that leaves the drawing untouched instead of silently falling back to the whole drawing.
#[test]
fn json_selection_leaves_other_objects_untouched() {
    let mut s = Session::new();
    let a = handle(&s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap());
    let b = handle(&s.execute("line", &json!({"points": [[10, 0], [10, 5]]})).unwrap());

    let r = s.execute("calibrate", &json!({"handles": [a.hex()], "p1": [0, 0], "p2": [5, 0], "length": 1250, "apply": "selection"})).unwrap();
    assert_eq!(r["apply"], "selection", "{r}");
    assert_eq!(r["scaled"], 1, "{r}");
    let (a1, a2) = ends(&s, a);
    assert!(near(a1, Vec2::ZERO) && near(a2, Vec2::new(1250.0, 0.0)), "{a1:?} {a2:?}");
    let (b1, b2) = ends(&s, b);
    assert!(near(b1, Vec2::new(10.0, 0.0)) && near(b2, Vec2::new(10.0, 5.0)), "untouched: {b1:?} {b2:?}");

    // An empty selection is a clear error, not a panic, and changes nothing.
    let model = s.doc().unwrap().model.len();
    let e =
        s.execute("calibrate", &json!({"handles": [], "p1": [0, 0], "p2": [5, 0], "length": 1250, "apply": "selection"})).unwrap_err().to_string();
    assert!(e.to_lowercase().contains("selected"), "{e}");
    assert_eq!(s.doc().unwrap().model.len(), model);
}

/// Naming handles without an `apply` target means the selection: the rest of the drawing is never
/// scaled by accident.
#[test]
fn json_handles_default_to_the_selection() {
    let mut s = Session::new();
    let a = handle(&s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap());
    let b = handle(&s.execute("line", &json!({"points": [[10, 0], [10, 5]]})).unwrap());

    // Objects named but no `apply`: only they scale, never the whole drawing.
    let r = s.execute("calibrate", &json!({"handles": [a.hex()], "p1": [0, 0], "p2": [5, 0], "length": 1250})).unwrap();
    assert_eq!(r["apply"], "selection", "{r}");
    assert_eq!(r["scaled"], 1, "{r}");
    let (_, a2) = ends(&s, a);
    assert!(near(a2, Vec2::new(1250.0, 0.0)), "{a2:?}");
    let (b1, b2) = ends(&s, b);
    assert!(near(b1, Vec2::new(10.0, 0.0)) && near(b2, Vec2::new(10.0, 5.0)), "untouched: {b1:?} {b2:?}");
}

/// Hostile inputs — coincident points, a distance that overflows from finite coordinates, zero,
/// negative or textual lengths, a malformed point and an unknown target — each return a message and
/// change nothing, guarding against division by zero and panics.
#[test]
fn json_refuses_degenerate_distances_and_bad_lengths() {
    let mut s = Session::new();
    let a = handle(&s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap());
    let before = ends(&s, a);

    for p in [
        // The two points coincide.
        json!({"p1": [2, 2], "p2": [2, 2], "length": 1250}),
        // ... and the distance may overflow with finite inputs.
        json!({"p1": [1e308, 0], "p2": [-1e308, 0], "length": 1250}),
        json!({"p1": [0, 0], "p2": [5, 0], "length": 0}),
        json!({"p1": [0, 0], "p2": [5, 0], "length": -5}),
        json!({"p1": [0, 0], "p2": [5, 0], "length": "1250"}),
        json!({"p1": "x", "p2": [5, 0], "length": 1250}),
        json!({"p1": [0, 0], "p2": [5, 0], "length": 1250, "apply": "somewhere"}),
    ] {
        let e = s.execute("calibrate", &p).unwrap_err().to_string();
        assert!(!e.is_empty(), "no message for {p}");
    }
    assert_eq!(ends(&s, a), before, "a refused calibration changes nothing");
}

/// The interactive sequence asks for the two points, rejects the same point twice with a clear
/// message and asks again, then takes the true length and Enter at the target prompt to scale
/// everything — the default a user gets without choosing.
#[test]
fn command_line_picks_two_points_the_length_and_everything() {
    let mut s = Session::new();
    let a = handle(&s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap());
    let b = handle(&s.execute("line", &json!({"points": [[10, 0], [10, 5]]})).unwrap());

    s.cmdline("calibrate").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Specify first point of the distance to calibrate");
    s.cmdline("0,0").unwrap();
    // The same point twice is reported clearly and the second point is asked for again.
    s.cmdline("0,0").unwrap();
    assert!(s.running.is_some());
    assert!(s.log.iter().any(|l| l.contains("different")), "{:?}", s.log);
    assert_eq!(s.current_prompt().unwrap().message, "Specify second point of the distance to calibrate");
    s.cmdline("5,0").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Enter the true length of that distance");
    s.cmdline("1250").unwrap();
    let p = s.current_prompt().unwrap();
    assert!(p.message.starts_with("Apply the scale to"), "{}", p.display());
    assert_eq!(p.default.as_deref(), Some("Everything"));
    assert_eq!(p.keywords, ["Everything", "Selection", "Underlay"]);
    // Enter takes the default: everything.
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    let (a1, a2) = ends(&s, a);
    assert!(near(a1, Vec2::ZERO) && near(a2, Vec2::new(1250.0, 0.0)), "{a1:?} {a2:?}");
    let (b1, b2) = ends(&s, b);
    assert!(near(b1, Vec2::new(2500.0, 0.0)) && near(b2, Vec2::new(2500.0, 1250.0)), "{b1:?} {b2:?}");
}

/// Choosing Selection at the last prompt applies the calibration to the objects already selected
/// and leaves every other object exactly where it was.
#[test]
fn command_line_selection_scales_only_what_was_selected() {
    let mut s = Session::new();
    let a = handle(&s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap());
    let b = handle(&s.execute("line", &json!({"points": [[10, 0], [10, 5]]})).unwrap());
    s.set_selection(vec![b]);

    for t in ["calibrate", "0,0", "5,0", "1250", "s"] {
        s.cmdline(t).unwrap();
    }
    assert!(s.running.is_none());
    let (a1, a2) = ends(&s, a);
    assert!(near(a1, Vec2::ZERO) && near(a2, Vec2::new(5.0, 0.0)), "untouched: {a1:?} {a2:?}");
    let (b1, b2) = ends(&s, b);
    assert!(near(b1, Vec2::new(2500.0, 0.0)) && near(b2, Vec2::new(2500.0, 1250.0)), "{b1:?} {b2:?}");
}

/// Choosing Selection with nothing preselected asks for objects, and an empty pick reports
/// "Nothing selected" and ends without scaling anything.
#[test]
fn command_line_selection_with_nothing_selected_scales_nothing() {
    let mut s = Session::new();
    let a = handle(&s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap());
    let before = ends(&s, a);

    for t in ["calibrate", "0,0", "5,0", "1250", "s"] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(s.current_prompt().unwrap().message, "Select objects");
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert!(s.log.iter().any(|l| l.contains("Nothing selected")), "{:?}", s.log);
    assert_eq!(ends(&s, a), before);
}

/// Choosing Underlay while underlays are unsupported reports so clearly, keeps the command at the
/// target prompt so another choice is possible, and changes nothing; Choosing Everything afterwards
/// still completes the calibration.
#[test]
fn command_line_underlay_without_one_is_a_clear_refusal() {
    let mut s = Session::new();
    let a = handle(&s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap());
    let before = ends(&s, a);

    for t in ["calibrate", "0,0", "5,0", "1250", "u"] {
        s.cmdline(t).unwrap();
    }
    // The command stays at the apply prompt, so another target can be chosen.
    assert!(s.running.is_some());
    assert!(s.log.iter().any(|l| l.contains("underlay")), "{:?}", s.log);
    assert_eq!(s.current_prompt().unwrap().message, "Apply the scale to");
    assert_eq!(ends(&s, a), before);

    // Everything still ends it.
    s.cmdline("e").unwrap();
    assert!(s.running.is_none());
    let (_, a2) = ends(&s, a);
    assert!(near(a2, Vec2::new(1250.0, 0.0)), "{a2:?}");
}
