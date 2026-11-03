//! A command or keyword typed with the Greek keyboard layout active (ΛΙΝΕ for LINE, ΘΝΔΟ for
//! Undo) works as its Latin name: the layout types each Greek letter on the Latin key it sounds
//! like.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

fn lines(s: &Session) -> usize {
    s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count()
}

#[test]
fn greek_layout_commands_start_like_the_latin_ones() {
    let mut s = Session::new();
    s.script("ΛΙΝΕ 0,0 10,0\n\n").unwrap();
    assert_eq!(lines(&s), 1, "{}", s.prompt_text());
    // Accents the layout's dead key adds are ignored; lower case works too.
    s.script("λίνε 0,5 10,5\n\n").unwrap();
    assert_eq!(lines(&s), 2);
    // The Q key types a semicolon: QSAVE is ";ΣΑΩΕ", which only starts the command (no file needed
    // here: it reports, it never fails as an unknown command).
    let err = s.start(";ΣΑΩΕ").map(|_| String::new()).unwrap_or_else(|e| e.to_string());
    assert!(!err.contains("unknown command"), "{err}");
}

#[test]
fn a_greek_lisp_command_runs_as_itself_not_as_the_latin_keys_command() {
    let mut s = Session::new();
    // ΛΙΝΕ types LINE, but the routine defines a command of that Greek name: it is the command typed.
    s.cmdline("(defun c:ΛΙΝΕ () (command \"_.CIRCLE\" '(0 0) 5) (princ))").unwrap();
    s.cmdline("ΛΙΝΕ").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let doc = s.doc().unwrap();
    let circles = doc.model.iter().filter(|e| matches!(e.kind, EntityKind::Circle(_))).count();
    assert_eq!((circles, lines(&s)), (1, 0), "the routine drew the circle, LINE was not started");
}

#[test]
fn greek_layout_selection_words_select_like_the_latin_ones() {
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 5 })).unwrap();
    s.execute("circle", &json!({ "center": [20, 0], "radius": 5 })).unwrap();
    // ΑΛΛ is the keys A-L-L: ALL at the "Select objects" prompt.
    s.script(
        "ERASE ΑΛΛ

",
    )
    .unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(s.doc().unwrap().model.is_empty(), "ΑΛΛ selected nothing");
}

#[test]
fn greek_layout_keywords_match_prompt_options() {
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 5 })).unwrap();
    s.script("ΜΙΡΡΟΡ ALL\n\n0,0 10,0\n").unwrap();
    // ... the last prompt asks whether to delete the sources: N (Greek layout: Ν) keeps them.
    s.script("Ν\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let circles = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Circle(_))).count();
    assert_eq!(circles, 2, "the circle and its mirror image");

    // Υ (the Y key) answers Yes, which erases the source: unmatched Greek text would count as No, so
    // only a recognised keyword gives this result.
    let mut yes = Session::new();
    yes.execute("circle", &json!({ "center": [0, 0], "radius": 5 })).unwrap();
    yes.script(
        "ΜΙΡΡΟΡ ALL

0,0 10,0
Υ
",
    )
    .unwrap();
    assert!(yes.running.is_none(), "{}", yes.prompt_text());
    let circles = yes.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Circle(_))).count();
    assert_eq!(circles, 1, "Υ answered Yes and the source was erased");
}
