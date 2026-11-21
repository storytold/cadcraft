//! Draw order (#439): DRAWORDER's Above objects / Under objects / Front / Back, typed and as JSON,
//! TEXTTOFRONT and HATCHTOBACK.

use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::{Input, Session};
use serde_json::json;

/// A session with five lines; their handles in draw order.
fn five_lines() -> (Session, Vec<Handle>) {
    let mut s = Session::new();
    for y in 0..5 {
        s.execute("line", &json!({ "points": [[0, y], [10, y]] })).unwrap();
    }
    let hs = order(&s);
    (s, hs)
}

fn order(s: &Session) -> Vec<Handle> {
    s.doc().unwrap().model.iter().map(|e| e.handle).collect()
}

fn hex(hs: &[Handle]) -> Vec<String> {
    hs.iter().map(|h| h.hex()).collect()
}

#[test]
fn draworder_json_keeps_relative_order() {
    let (mut s, h) = five_lines();
    // Front/back keep the moved objects' order among themselves, whatever order they are given in.
    s.execute("draworder", &json!({ "handles": hex(&[h[2], h[0]]), "order": "front" })).unwrap();
    assert_eq!(order(&s), [h[1], h[3], h[4], h[0], h[2]]);
    s.execute("draworder", &json!({ "handles": hex(&[h[2], h[0]]) })).unwrap();
    assert_eq!(order(&s), [h[0], h[2], h[1], h[3], h[4]], "back is the default");
    // Above: right over the topmost reference object.
    let r = s.execute("draworder", &json!({ "handles": hex(&[h[0], h[2]]), "order": "above", "reference": hex(&[h[1], h[3]]) })).unwrap();
    assert_eq!(r["moved"], 2);
    assert_eq!(order(&s), [h[1], h[3], h[0], h[2], h[4]]);
    // Under: right under the lowest reference object; a moved object among the references is skipped.
    s.execute("draworder.under", &json!({ "handles": hex(&[h[4]]), "reference": hex(&[h[3], h[0], h[4]]) })).unwrap();
    assert_eq!(order(&s), [h[1], h[4], h[3], h[0], h[2]]);
    s.execute("draworder.above", &json!({ "handles": hex(&[h[1]]), "reference": hex(&[h[2]]) })).unwrap();
    assert_eq!(order(&s), [h[4], h[3], h[0], h[2], h[1]]);
    // Bad options, missing references and unknown handles are refused without changes.
    let before = order(&s);
    assert!(s.execute("draworder", &json!({ "handles": hex(&[h[0]]), "order": "sideways" })).is_err());
    assert!(s.execute("draworder", &json!({ "handles": hex(&[h[0]]), "order": 3 })).is_err());
    assert!(s.execute("draworder", &json!({ "handles": hex(&[h[0]]), "order": "above" })).is_err());
    assert!(s.execute("draworder.above", &json!({ "handles": hex(&[h[0]]), "reference": "1A" })).is_err());
    assert!(s.execute("draworder.under", &json!({ "handles": hex(&[h[0]]), "reference": ["FFFFF"] })).is_err());
    assert!(s.execute("draworder.under", &json!({ "handles": hex(&[h[0]]), "reference": hex(&[h[0]]) })).is_err());
    assert_eq!(order(&s), before);
    // The old menu commands still work.
    s.execute("draworder.front", &json!({ "handles": hex(&[h[4]]) })).unwrap();
    assert_eq!(order(&s).last(), Some(&h[4]));
    s.execute("draworder.back", &json!({ "handles": hex(&[h[4]]) })).unwrap();
    assert_eq!(order(&s).first(), Some(&h[4]));
    assert!(s.running.is_none(), "JSON calls never prompt");
}

#[test]
fn draworder_typed_options_and_undo() {
    let (mut s, h) = five_lines();
    let undo = s.state().unwrap().undo.len();
    s.cmdline("draworder").unwrap();
    assert!(s.prompt_text().contains("Select objects"), "{}", s.prompt_text());
    s.input(Input::Pick(vec![h[0]])).unwrap();
    s.cmdline("").unwrap();
    assert!(s.prompt_text().contains("[Above objects/Under objects/Front/Back] <Back>"), "{}", s.prompt_text());
    s.cmdline("a").unwrap();
    assert!(s.prompt_text().contains("Select reference objects"), "{}", s.prompt_text());
    s.input(Input::Pick(vec![h[2]])).unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(order(&s), [h[1], h[2], h[0], h[3], h[4]]);
    assert_eq!(s.state().unwrap().undo.len(), undo + 1, "one undo step");
    assert!(s.selection().is_empty(), "picked objects don't stay selected");
    // Enter takes the default, Back.
    s.cmdline("dr").unwrap();
    s.input(Input::Pick(vec![h[4]])).unwrap();
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(order(&s).first(), Some(&h[4]));
    // Front by keyword; with a pickfirst selection the option is asked at once.
    s.set_selection(vec![h[4]]);
    s.cmdline("draworder").unwrap();
    assert!(s.prompt_text().contains("ordering option"), "{}", s.prompt_text());
    s.cmdline("f").unwrap();
    assert_eq!(order(&s).last(), Some(&h[4]));
    // The Send Under Objects menu item goes from the objects straight to the references.
    s.start("draworder.under").unwrap();
    s.input(Input::Pick(vec![h[4]])).unwrap();
    s.cmdline("").unwrap();
    assert!(s.prompt_text().contains("Select reference objects"), "{}", s.prompt_text());
    s.input(Input::Pick(vec![h[1]])).unwrap();
    s.cmdline("").unwrap();
    assert_eq!(order(&s).first(), Some(&h[4]));
    // An empty reference set ends without changes.
    let before = order(&s);
    s.cmdline("draworder").unwrap();
    s.input(Input::Pick(vec![h[0]])).unwrap();
    s.cmdline("").unwrap();
    s.cmdline("u").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(order(&s), before);
    s.execute("u", &json!({})).unwrap();
    s.execute("u", &json!({})).unwrap();
    s.execute("u", &json!({})).unwrap();
    s.execute("u", &json!({})).unwrap();
    assert_eq!(order(&s), h, "undo restores the order");
}

#[test]
fn texttofront_and_hatchtoback() {
    let mut s = Session::new();
    s.execute("text", &json!({ "at": [0, 0], "height": 2, "text": "A" })).unwrap();
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 5] })).unwrap();
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    s.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "SOLID" })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [10, 10]] })).unwrap();
    let kinds = |s: &Session| s.doc().unwrap().model.iter().map(|e| e.kind.type_name()).collect::<Vec<_>>();
    // HATCH already puts its hatch at the back; move it to the front first.
    let hatch = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Hatch(_))).unwrap().handle;
    s.execute("draworder.front", &json!({ "handles": [hatch.hex()] })).unwrap();
    assert_eq!(kinds(&s).last(), Some(&"Hatch"));
    let r = s.execute("hatchtoback", &json!({})).unwrap();
    assert_eq!(r["moved"], 1);
    assert_eq!(kinds(&s).first(), Some(&"Hatch"));
    // Text only, then all annotation.
    let r = s.execute("texttofront", &json!({ "objects": "text" })).unwrap();
    assert_eq!(r["moved"], 1);
    assert_eq!(kinds(&s).last(), Some(&"Text"));
    s.execute("texttofront.dimensions", &json!({})).unwrap();
    assert!(kinds(&s).last().is_some_and(|k| k.contains("Dimension")), "{:?}", kinds(&s));
    assert!(s.execute("texttofront", &json!({ "objects": "everything" })).is_err());
    // Typed: the option prompt, Enter = All.
    s.cmdline("texttofront").unwrap();
    assert!(s.prompt_text().contains("[Text/Dimensions/Leaders/All] <All>"), "{}", s.prompt_text());
    s.cmdline("t").unwrap();
    assert!(s.running.is_none());
    assert_eq!(kinds(&s).last(), Some(&"Text"));
    assert!(s.log.iter().any(|l| l.contains("1 object(s) brought to front")), "{:?}", s.log);
    s.cmdline("texttofront").unwrap();
    s.cmdline("").unwrap();
    assert!(s.log.iter().any(|l| l.contains("2 object(s) brought to front")), "{:?}", s.log);
}
