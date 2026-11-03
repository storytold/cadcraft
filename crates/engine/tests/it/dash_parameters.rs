//! -PARAMETERS: the Parameters Manager at the command line, one option per run (#2579).

use cadcraft_engine::Session;
use serde_json::{Value, json};

fn typed(s: &mut Session, inputs: &[&str]) {
    for t in inputs {
        s.cmdline(t).unwrap();
    }
}

/// The parameter named `name` as PARAMETERS lists it.
fn parameter(s: &mut Session, name: &str) -> Option<Value> {
    let r = s.execute("parameters", &json!({})).unwrap();
    r["parameters"].as_array()?.iter().find(|p| p["name"] == name).cloned()
}

#[test]
fn new_edit_rename_list_and_delete() {
    let mut s = Session::new();
    // New: Enter takes the offered name.
    typed(&mut s, &["-parameters", "new", "", "4*2"]);
    assert!(!s.prompting(), "one option per run");
    let p = parameter(&mut s, "user1").expect("user1 made");
    assert_eq!((p["expr"].as_str(), p["value"].as_f64()), (Some("4*2"), Some(8.0)));
    assert_eq!(s.log.last().map(String::as_str), Some("user1 = 4*2 (8)"));

    // Edit: a new expression; Enter at the expression leaves it as it is.
    typed(&mut s, &["-parameters", "e", "user1", "10"]);
    assert_eq!(parameter(&mut s, "user1").unwrap()["value"].as_f64(), Some(10.0));
    typed(&mut s, &["-parameters", "e", "user1", ""]);
    assert_eq!(parameter(&mut s, "user1").unwrap()["expr"], "10");

    // Rename, and a parameter that uses the new name.
    typed(&mut s, &["-parameters", "r", "user1", "width"]);
    assert!(parameter(&mut s, "user1").is_none());
    typed(&mut s, &["-parameters", "n", "half", "width/2"]);
    assert_eq!(parameter(&mut s, "half").unwrap()["value"].as_f64(), Some(5.0));

    // ?: names matching the wildcards.
    let before = s.log.len();
    typed(&mut s, &["-parameters", "?", "h*"]);
    assert_eq!(s.log.get(before..).unwrap_or_default().last().map(String::as_str), Some("half = width/2 (5)"));
    assert!(!s.log.get(before..).unwrap_or_default().iter().any(|l| l.starts_with("width =")), "{:?}", s.log);

    // Delete: a parameter another expression uses stays, and the prompt asks again.
    typed(&mut s, &["-parameters", "d", "width"]);
    assert!(s.prompting(), "still asking which parameter");
    assert!(s.log.iter().any(|l| l.contains("`width` is used by `half`")), "{:?}", s.log);
    typed(&mut s, &["half"]);
    assert!(!s.prompting());
    assert!(parameter(&mut s, "half").is_none());
    assert!(parameter(&mut s, "width").is_some());

    // Each run is one step to undo.
    s.undo().unwrap();
    assert!(parameter(&mut s, "half").is_some(), "UNDO brings the deleted parameter back");
}

#[test]
fn names_are_checked_where_they_are_typed() {
    let mut s = Session::new();
    s.execute("parameters", &json!({ "name": "len", "expr": "3" })).unwrap();
    typed(&mut s, &["-parameters", "new"]);
    // A name in use, or one that isn't a name, is refused and asked for again.
    typed(&mut s, &["len"]);
    assert_eq!(s.log.last().map(String::as_str), Some("`len` is already in use."));
    typed(&mut s, &["2x"]);
    assert!(s.log.last().is_some_and(|l| l.starts_with("`2x` is not a valid parameter name")), "{:?}", s.log);
    // The offered name skips the names in use.
    typed(&mut s, &["", "1"]);
    assert!(parameter(&mut s, "user1").is_some());
    typed(&mut s, &["-parameters", "new", ""]);
    assert!(s.prompt_text().contains("Enter an expression for the new parameter"), "{}", s.prompt_text());
    // A new parameter needs an expression; a bad one is refused.
    typed(&mut s, &[""]);
    assert_eq!(s.log.last().map(String::as_str), Some("An expression is required."));
    typed(&mut s, &["len*"]);
    assert!(s.prompting(), "a bad expression is asked for again");
    typed(&mut s, &["len*2"]);
    assert_eq!(parameter(&mut s, "user2").unwrap()["value"].as_f64(), Some(6.0));

    // Edit, Rename and Delete name an existing parameter.
    typed(&mut s, &["-parameters", "e", "nope"]);
    assert_eq!(s.log.last().map(String::as_str), Some("No parameter named `nope`."));
    typed(&mut s, &[""]);
    assert!(!s.prompting(), "Enter ends the command");
}

/// Only an expression that names a parameter keeps it from being deleted, even one that doesn't
/// parse; an expression broken for another reason doesn't block deleting something else.
#[test]
fn delete_is_blocked_only_by_expressions_that_name_the_parameter() {
    let mut s = Session::new();
    s.execute("parameters", &json!({ "name": "w", "expr": "3" })).unwrap();
    s.execute("parameters", &json!({ "name": "spare", "expr": "1" })).unwrap();
    // As a drawing from a file can hold them: one expression names a parameter that's gone, and
    // one doesn't parse but names w.
    let params = &mut s.doc_mut().unwrap().parametric.parameters;
    params.push(cadcraft_doc::Parameter { name: "stale".into(), expr: "missing*2".into(), description: String::new() });
    params.push(cadcraft_doc::Parameter { name: "torn".into(), expr: "w*(".into(), description: String::new() });

    s.execute("parameters", &json!({ "delete": "spare" })).expect("the stale expression doesn't name spare");
    assert!(parameter(&mut s, "spare").is_none());
    let e = s.execute("parameters", &json!({ "delete": "w" })).unwrap_err().to_string();
    assert!(e.contains("`w` is used by `torn`"), "{e}");
}

/// Renaming a parameter to its own name changes nothing, so UNDO goes back past it (CodeRabbit on
/// #2694); a name that isn't there is still reported.
#[test]
fn renaming_to_the_same_name_leaves_no_step_to_undo() {
    let mut s = Session::new();
    s.execute("parameters", &json!({ "name": "w", "expr": "3" })).unwrap();
    typed(&mut s, &["-parameters", "r", "w", "w"]);
    assert!(!s.prompting());
    s.undo().unwrap();
    assert!(parameter(&mut s, "w").is_none(), "UNDO took back making w, not an empty rename");
    assert!(s.execute("parameters", &json!({ "name": "w", "rename": "w" })).is_err(), "no parameter w");
}

#[test]
fn json_form_is_parameters() {
    let mut s = Session::new();
    let r = s.execute("-parameters", &json!({ "name": "w", "expr": "7" })).unwrap();
    assert_eq!(r["message"], "w = 7 (7)");
    assert!(!s.prompting(), "a JSON call never prompts");
}
