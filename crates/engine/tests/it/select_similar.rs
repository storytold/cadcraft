//! SELECTSIMILAR (with SELECTSIMILARMODE) and object isolation — ISOLATEOBJECTS, HIDEOBJECTS,
//! UNISOLATEOBJECTS (#428): JSON forms, typed forms, and hidden objects skipped by display and
//! selection.

use cadcraft_engine::doc::{Handle, Space};
use cadcraft_engine::geom::{Bounds2, Vec2};
use cadcraft_engine::{Session, render, select};
use serde_json::json;

/// Two lines and a circle on layer 0, a line and a circle on "Walls" (the last circle red).
fn drawing() -> (Session, Vec<Handle>) {
    let mut s = Session::new();
    s.execute("layer.new", &json!({ "name": "Walls" })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.execute("line", &json!({ "points": [[0, 5], [10, 5]] })).unwrap();
    s.execute("circle", &json!({ "center": [20, 0], "radius": 2 })).unwrap();
    s.execute("line", &json!({ "points": [[0, 10], [10, 10]] })).unwrap();
    s.execute("circle", &json!({ "center": [30, 0], "radius": 3 })).unwrap();
    let hs: Vec<Handle> = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
    s.execute("properties.set", &json!({ "handles": [hs[3].hex(), hs[4].hex()], "layer": "Walls" })).unwrap();
    s.execute("properties.set", &json!({ "handles": [hs[4].hex()], "color": 1 })).unwrap();
    s.set_selection(Vec::new());
    (s, hs)
}

fn sorted(mut v: Vec<Handle>) -> Vec<Handle> {
    v.sort();
    v
}

#[test]
fn selectsimilar_matches_type_and_mode_properties() {
    let (mut s, hs) = drawing();
    // Default SELECTSIMILARMODE 130: same type and layer (and name).
    let r = s.execute("selectsimilar", &json!({ "handles": [hs[0].hex()] })).unwrap();
    assert_eq!(r["selected"], 2);
    assert_eq!(sorted(s.selection()), vec![hs[0], hs[1]]);
    // Type only: every line.
    s.execute("selectsimilar", &json!({ "handles": [hs[0].hex()], "mode": 0 })).unwrap();
    assert_eq!(sorted(s.selection()), vec![hs[0], hs[1], hs[3]]);
    // Several references: objects like any of them. Colour only (1): the red circle stands alone.
    s.execute("selectsimilar", &json!({ "handles": [hs[0].hex(), hs[4].hex()], "mode": 1 })).unwrap();
    assert_eq!(sorted(s.selection()), vec![hs[0], hs[1], hs[3], hs[4]]);
    // The current selection is the default; the variable sets the default mode.
    s.execute("setvar", &json!({ "name": "SELECTSIMILARMODE", "value": 0 })).unwrap();
    s.set_selection(vec![hs[2]]);
    s.execute("selectsimilar", &json!({})).unwrap();
    assert_eq!(sorted(s.selection()), vec![hs[2], hs[4]]);
    // Hostile or empty input is an error, never a panic.
    s.set_selection(Vec::new());
    assert!(s.execute("selectsimilar", &json!({})).is_err());
    assert!(s.execute("selectsimilar", &json!({ "handles": ["FFFFFF"] })).is_err());
    for m in [json!(-1), json!(256), json!("x"), json!(1.5), json!(f64::MAX)] {
        assert!(s.execute("selectsimilar", &json!({ "handles": [hs[0].hex()], "mode": m })).is_err(), "{m}");
    }
    assert!(s.execute("setvar", &json!({ "name": "SELECTSIMILARMODE", "value": 999 })).is_err());
    assert!(s.running.is_none(), "JSON calls never prompt");
}

#[test]
fn selectsimilar_typed_with_settings_keyword() {
    let (mut s, hs) = drawing();
    s.cmdline("selectsimilar").unwrap();
    assert!(s.prompt_text().contains("Select objects or [SEttings]"), "{}", s.prompt_text());
    // SEttings asks for the bit code: 0 = type only.
    s.cmdline("se").unwrap();
    assert!(s.prompt_text().contains("SELECTSIMILARMODE"), "{}", s.prompt_text());
    s.cmdline("0").unwrap();
    assert_eq!(s.settings.selectsimilarmode, 0);
    s.cmdline("10,0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(sorted(s.selection()), vec![hs[0], hs[1], hs[3]]);
    // With a pickfirst selection it selects at once.
    s.set_selection(vec![hs[2]]);
    s.cmdline("selectsimilar").unwrap();
    assert!(s.running.is_none());
    assert_eq!(sorted(s.selection()), vec![hs[2], hs[4]]);
}

/// Handles the display list draws something for.
fn drawn(s: &Session) -> usize {
    let d = s.doc().unwrap();
    render::build(d, &Space::Model, &render::Options::default()).prims.len()
}

#[test]
fn isolate_hide_and_unisolate() {
    let (mut s, hs) = drawing();
    let all_prims = drawn(&s);
    // ISOLATE: everything else is hidden — not drawn, not selectable, not in ALL.
    let r = s.execute("isolateobjects", &json!({ "handles": [hs[0].hex()] })).unwrap();
    assert_eq!(r["hidden"], 4);
    let d = s.doc().unwrap();
    assert!(d.is_hidden_object(hs[1]) && !d.is_hidden_object(hs[0]));
    assert!(drawn(&s) < all_prims);
    let everything = Bounds2::new(Vec2::new(-100.0, -100.0), Vec2::new(100.0, 100.0));
    assert_eq!(select::select_window(s.doc().unwrap(), &Space::Model, everything, true), vec![hs[0]]);
    assert!(select::pick(s.doc().unwrap(), &Space::Model, Vec2::new(5.0, 5.0), 0.5).is_none());
    // New objects show; HIDEOBJECTS hides more.
    s.execute("line", &json!({ "points": [[0, 20], [10, 20]] })).unwrap();
    let r = s.execute("hideobjects", &json!({ "handles": [hs[0].hex()] })).unwrap();
    assert_eq!(r["hidden"], 1);
    // Undo shows the hidden object again, redo hides it.
    s.execute("u", &json!({})).unwrap();
    assert!(!s.doc().unwrap().is_hidden_object(hs[0]));
    s.execute("redo", &json!({})).unwrap();
    assert!(s.doc().unwrap().is_hidden_object(hs[0]));
    // UNISOLATE shows everything again.
    let r = s.execute("unisolateobjects", &json!({})).unwrap();
    assert_eq!(r["shown"], 5);
    assert!(s.doc().unwrap().hidden_objects.is_empty());
    assert_eq!(drawn(&s), all_prims + 1);
    assert_eq!(s.execute("unisolateobjects", &json!({})).unwrap()["shown"], 0);
    // Nothing to isolate or hide is an error; isolation is temporary only.
    assert!(s.execute("isolateobjects", &json!({})).is_err());
    assert!(s.execute("hideobjects", &json!({ "handles": "1A" })).is_err());
    assert!(s.execute("isolateobjects", &json!({ "handles": ["FFFFFF"] })).is_err());
    assert!(s.execute("setvar", &json!({ "name": "OBJECTISOLATIONMODE", "value": 1 })).is_err());
    assert_eq!(s.execute("getvar", &json!({ "name": "OBJECTISOLATIONMODE" })).unwrap()["value"], json!(0));
}

#[test]
fn isolate_typed_and_all_skips_hidden() {
    let (mut s, hs) = drawing();
    s.cmdline("hideobjects").unwrap();
    assert!(s.prompt_text().contains("Select objects"), "{}", s.prompt_text());
    s.cmdline("10,0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert!(s.doc().unwrap().is_hidden_object(hs[0]));
    assert!(s.selection().is_empty(), "hidden objects don't stay selected");
    // ALL at Select objects leaves hidden objects out.
    s.cmdline("isolateobjects").unwrap();
    s.cmdline("all").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().hidden_objects.len(), 1);
    s.cmdline("unisolateobjects").unwrap();
    assert!(s.doc().unwrap().hidden_objects.is_empty());
}
