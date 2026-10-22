//! FIND: Edit ▸ Find… and typed FIND prompt for the search text, the replacement and the
//! Match case / Whole word options; the JSON form keeps working without prompting.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

fn add_text(s: &mut Session, at: [f64; 2], value: &str) {
    s.execute("text", &json!({ "at": at, "text": value, "height": 2.0 })).unwrap();
}

fn texts(s: &Session) -> Vec<String> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).collect()
}

#[test]
fn find_prompts_instead_of_erroring() {
    let mut s = Session::new();
    s.cmdline("find").unwrap();
    assert!(s.running.is_some(), "FIND should prompt, not run with no parameters");
    assert!(s.prompt_text().contains("Find what"), "{}", s.prompt_text());
    // Enter at the first prompt leaves the command.
    s.cmdline("").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
}

#[test]
fn find_replaces_with_match_case_and_whole_word_options() {
    let mut s = Session::new();
    add_text(&mut s, [0.0, 0.0], "bolt bolt");
    add_text(&mut s, [0.0, 5.0], "Bolt");
    s.cmdline("find").unwrap();
    s.cmdline("bolt").unwrap();
    assert!(s.prompt_text().contains("Replace with"), "{}", s.prompt_text());
    s.cmdline("pin").unwrap();
    assert!(s.prompt_text().contains("Search options"), "{}", s.prompt_text());
    s.cmdline("w").unwrap(); // Whole word on
    s.cmdline("m").unwrap(); // Match case on
    s.cmdline("").unwrap(); // <Done>
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(texts(&s), vec!["pin pin".to_string(), "Bolt".to_string()]);
}

#[test]
fn find_without_replacement_selects_the_matches() {
    let mut s = Session::new();
    add_text(&mut s, [0.0, 0.0], "bolt");
    s.cmdline("find").unwrap();
    s.cmdline("BOLT").unwrap(); // Match case is off by default.
    assert!(s.prompt_text().contains("Replace with"), "{}", s.prompt_text());
    s.cmdline("").unwrap(); // No replacement
    s.cmdline("").unwrap(); // <Done>
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(texts(&s), vec!["bolt".to_string()]);
    assert_eq!(s.selection().len(), 1, "the match is selected");
}

#[test]
fn find_json_form_runs_without_prompting() {
    let mut s = Session::new();
    add_text(&mut s, [0.0, 0.0], "Bolt bolt");
    let r = s.execute("find", &json!({ "find": "bolt", "replace": "pin" })).unwrap();
    assert_eq!(r["total"], json!(2));
    assert_eq!(r["message"], json!("2 replacement(s) in 1 object(s)"));
    assert_eq!(texts(&s), vec!["pin pin".to_string()]);
    assert!(s.running.is_none(), "the JSON form does not prompt");

    let r = s.execute("find", &json!({ "find": "pin" })).unwrap();
    assert_eq!(r["total"], json!(2));
    assert_eq!(r["message"], json!("2 match(es) in 1 object(s)"));
    let r = s.execute("find", &json!({ "find": "PIN", "matchCase": true })).unwrap();
    assert_eq!(r["total"], json!(0), "the JSON form still honours matchCase");
    let r = s.execute("find", &json!({ "find": "PIN" })).unwrap();
    assert_eq!(r["total"], json!(2), "matchCase is still off by default");
    assert!(s.running.is_none());
}
