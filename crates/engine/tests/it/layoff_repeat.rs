//! LAYOFF stays active until Enter or Esc (#1418): every pick turns off its object's layer and the
//! prompt comes straight back, the way AutoCAD's Layer Off works.

use cadcraft_engine::doc::Handle;
use cadcraft_engine::{Input, Session};
use serde_json::json;

/// The handles of the objects on `layer`.
fn on_layer(s: &Session, layer: &str) -> Vec<Handle> {
    s.doc().unwrap().model.iter().filter(|e| e.common.layer == layer).map(|e| e.handle).collect()
}

/// Three lines, one on layer "A", one on "B" and one on "0" (the current layer).
fn session_with_three_layers() -> Session {
    let mut s = Session::new();
    s.execute("layer.new", &json!({ "name": "A", "current": true })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.execute("layer.new", &json!({ "name": "B", "current": true })).unwrap();
    s.execute("line", &json!({ "points": [[0, 5], [10, 5]] })).unwrap();
    s.execute("layer.current", &json!({ "name": "0" })).unwrap();
    s.execute("line", &json!({ "points": [[0, 10], [10, 10]] })).unwrap();
    s
}

fn is_on(s: &Session, layer: &str) -> bool {
    s.doc().unwrap().layer(layer).unwrap().on
}

#[test]
fn layoff_keeps_asking_until_enter() {
    let mut s = session_with_three_layers();
    let undo_before = s.state().unwrap().undo.len();

    s.start("layoff").unwrap();
    assert!(s.prompt_text().contains("layer to be turned off"), "{}", s.prompt_text());
    // The first pick turns off its layer's object and the command stays active (#1418).
    s.input(Input::Pick(on_layer(&s, "A"))).unwrap();
    assert!(s.running.is_some(), "LAYOFF is still running: {}", s.prompt_text());
    assert!(s.prompt_text().contains("layer to be turned off"), "it asks again: {}", s.prompt_text());
    assert!(!is_on(&s, "A"), "the picked object's layer is off");
    assert!(is_on(&s, "B"), "no other layer was touched");
    // The second pick turns off the second layer as well, without the command ending.
    s.input(Input::Pick(on_layer(&s, "B"))).unwrap();
    assert!(s.running.is_some(), "LAYOFF is still running: {}", s.prompt_text());
    assert!(!is_on(&s, "B"), "the second object's layer is off");
    assert!(is_on(&s, "0"), "the layer nothing was picked on is untouched");
    // Enter ends it, and the whole run is one undo step.
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none(), "Enter ends LAYOFF: {}", s.prompt_text());
    assert_eq!(s.state().unwrap().undo.len(), undo_before + 1, "one undo step for the whole run");
    s.undo().unwrap();
    assert!(is_on(&s, "A") && is_on(&s, "B"), "one undo takes both layers back");
}

#[test]
fn esc_ends_layoff_and_keeps_what_it_turned_off() {
    let mut s = session_with_three_layers();
    let undo_before = s.state().unwrap().undo.len();
    s.start("layoff").unwrap();
    s.input(Input::Pick(on_layer(&s, "A"))).unwrap();
    s.input(Input::Cancel).unwrap();
    assert!(s.running.is_none(), "Esc ends LAYOFF: {}", s.prompt_text());
    assert!(!is_on(&s, "A"), "the layer turned off before Esc stays off");
    assert_eq!(s.state().unwrap().undo.len(), undo_before + 1, "one undo step, the LAYOFF command");
}

#[test]
fn an_empty_pick_keeps_layoff_running() {
    let mut s = session_with_three_layers();
    s.start("layoff").unwrap();
    // A window that finds nothing is not a pick: the prompt stays and nothing is turned off.
    s.input(Input::Pick(Vec::new())).unwrap();
    assert!(s.running.is_some(), "{}", s.prompt_text());
    assert!(s.log.iter().any(|l| l.contains("0 found")), "{:?}", s.log);
    assert!(is_on(&s, "A") && is_on(&s, "B") && is_on(&s, "0"));
    s.input(Input::Pick(on_layer(&s, "A"))).unwrap();
    assert!(s.running.is_some() && !is_on(&s, "A"));
}

/// A pickfirst selection still acts at once and ends the command, and the JSON form never prompts.
#[test]
fn pickfirst_and_json_calls_are_unchanged() {
    let mut s = session_with_three_layers();
    let a = on_layer(&s, "A");
    s.set_selection(a);
    s.start("layoff").unwrap();
    assert!(s.running.is_none(), "pickfirst: no prompt");
    assert!(!is_on(&s, "A"), "{:?}", s.log);

    let mut s = session_with_three_layers();
    s.execute("layoff", &json!({ "handles": on_layer(&s, "B").iter().map(|h| h.hex()).collect::<Vec<_>>() })).unwrap();
    assert!(s.running.is_none(), "the JSON form must not prompt");
    assert!(!is_on(&s, "B"));
}
