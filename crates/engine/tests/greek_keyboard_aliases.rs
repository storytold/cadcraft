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

/// 5 000 Greek aliases and a pasted word of a million letters that is none of them: looking it up
/// costs the word's length once, not once for every alias.
#[test]
fn a_long_greek_word_is_folded_once_however_many_greek_aliases_there_are() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = Session::new();
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
    let set: serde_json::Map<String, serde_json::Value> = (0..5_000).map(|i| (format!("ΞΞ{i}"), json!("line"))).collect();
    s.execute("aliasedit", &json!({ "set": set })).unwrap();
    let word = "Ω".repeat(1_000_000);
    let t = std::time::Instant::now();
    let started = s.start(&word);
    let took = t.elapsed();
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
    assert!(started.is_err(), "no command has those keys");
    assert!(took.as_secs() < 5, "{took:?}");
}
