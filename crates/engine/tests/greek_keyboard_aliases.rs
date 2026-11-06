//! A word typed with the Greek keyboard layout that is an alias of its own (ALIASEDIT) is that
//! alias, not the Latin name its keys type. The aliases are process-wide, so this file keeps to
//! one test at a time.

use std::sync::Mutex;

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

#[test]
fn a_greek_alias_starts_its_command_not_the_latin_keys_command() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = Session::new();
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
    // ΛΙΝΕ types LINE, but the user made it an alias of CIRCLE: aliases are stored as typed (only
    // ASCII is lower-cased), and the alias wins.
    s.execute("aliasedit", &json!({ "set": { "ΛΙΝΕ": "circle" } })).unwrap();
    let ran = s.script(
        "ΛΙΝΕ 0,0 5
",
    );
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
    ran.unwrap();
    let doc = s.doc().unwrap();
    let circles = doc.model.iter().filter(|e| matches!(e.kind, EntityKind::Circle(_))).count();
    let lines = doc.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
    assert_eq!((circles, lines), (1, 0), "the alias drew the circle, LINE was not started");
}

#[test]
fn a_greek_alias_starts_its_command_in_any_case_or_accent() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = Session::new();
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
    s.execute("aliasedit", &json!({ "set": { "ΛΙΝΕ": "circle" } })).unwrap();
    let ran: Vec<_> = ["λινε", "λίνε"].iter().map(|typed| s.script(&format!("{typed} 0,0 5\n"))).collect();
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
    assert!(ran.iter().all(Result::is_ok), "{ran:?}");
    let doc = s.doc().unwrap();
    let circles = doc.model.iter().filter(|e| matches!(e.kind, EntityKind::Circle(_))).count();
    let lines = doc.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
    assert_eq!((circles, lines), (2, 0), "the alias drew both circles, LINE was not started");
}
