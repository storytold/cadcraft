//! A command or keyword typed with the Greek keyboard layout active (ΛΙΝΕ for LINE, ΘΝΔΟ for
//! Undo) works as its Latin name: the layout types each Greek letter on the Latin key it sounds
//! like.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

/// How many lines the drawing's model space holds.
fn lines(s: &Session) -> usize {
    s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count()
}

/// ΛΙΝΕ draws a line like LINE, in lower case and with accents too; `;ΣΑΩΕ` starts QSAVE; a
/// Greek word that is no command's keys is reported as typed.
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
    // A Greek word that is no command's keys is reported as typed, not as the Latin letters read from it.
    let err = s.start("ΧΥΖ").unwrap_err().to_string();
    assert!(err.contains("ΧΥΖ") && !err.contains("xyz"), "{err}");
}

/// A LISP command with a Greek name runs as itself, not as the command its keys type.
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

/// ΑΛΛ at "Select objects" selects everything, as ALL does.
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

/// Ν and Υ answer MIRROR's "Erase source objects?" as N and Y do.
#[test]
fn greek_layout_keywords_match_prompt_options() {
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 5 })).unwrap();
    s.script("ΜΙΡΡΟΡ ALL\n\n0,0 10,0\n").unwrap();
    // ... the last prompt asks whether to delete the sources: N (Greek layout: Ν) keeps them. Text
    // the prompt does not know keeps them too, so what Ν is read as is checked itself.
    let no = s.current_prompt().and_then(|p| p.match_keyword("Ν"));
    assert_eq!(no.as_deref(), Some("No"), "{}", s.prompt_text());
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

/// A Greek block name typed at BEDIT's "block name or select" opens that block: it is not read
/// as a selection word.
#[test]
fn a_greek_block_name_typed_at_a_name_or_select_prompt_reaches_the_command_as_typed() {
    let mut s = Session::new();
    let l = s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap()["handles"][0].clone();
    s.execute("block", &json!({ "name": "ΠΟΡΤΑ", "base": [0, 0], "handles": [l] })).unwrap();
    // BEDIT asks for a block name or a selected reference. ΠΟΡΤΑ is also the keys P-O-R-T-A, which a
    // selection word would be read from, but no selection word is meant here: the name is typed as it is.
    s.start("BEDIT").unwrap();
    s.cmdline("ΠΟΡΤΑ").unwrap();
    let open = s.state().unwrap().block_editor().map(str::to_string);
    assert_eq!(open.as_deref(), Some("ΠΟΡΤΑ"), "{}", s.prompt_text());
}

/// Μ is the M key, Multiple at a selection prompt; once a block has that name, typing it at
/// BEDIT opens the block.
#[test]
fn a_block_named_like_a_greek_selection_word_opens_at_a_name_or_select_prompt() {
    // The Greek capital Mu (U+039C) is the M key: Multiple at a selection prompt.
    let mu = "\u{39c}";
    let mut s = Session::new();
    let l = s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap()["handles"][0].clone();
    // With no block of that name it stays the selection word, which BEDIT's prompt waits through.
    s.start("BEDIT").unwrap();
    s.cmdline(mu).unwrap();
    assert!(s.running.is_some(), "Multiple keeps the prompt: {}", s.prompt_text());
    assert!(s.state().unwrap().block_editor().is_none());
    s.cancel();
    // Once a block has that name, typing it opens that block.
    s.execute("block", &json!({ "name": mu, "base": [0, 0], "handles": [l] })).unwrap();
    s.start("BEDIT").unwrap();
    s.cmdline(mu).unwrap();
    let open = s.state().unwrap().block_editor().map(str::to_string);
    assert_eq!(open.as_deref(), Some(mu), "{}", s.prompt_text());
}

/// ΣΟΛΙΔ is the keys of Solid at HATCH's pattern prompt; once a loaded pattern has that name,
/// typing it in any case or accent chooses that pattern.
#[test]
fn a_loaded_pattern_named_like_the_greek_keys_of_solid_is_that_pattern_at_the_pattern_prompt() {
    // ΣΟΛΙΔ is the keys S-O-L-I-D: Solid at HATCH's pattern prompt, which takes no scale or angle.
    let mut s = Session::new();
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    s.start("hatch").unwrap();
    s.input(cadcraft_engine::Input::Keyword("Pattern".into())).unwrap();
    s.cmdline("ΣΟΛΙΔ").unwrap();
    assert!(!s.prompt_text().contains("scale"), "Solid: {}", s.prompt_text());
    s.cancel();
    // A pattern file that defines a pattern of that name: typing the name chooses that pattern, which asks its scale.
    s.execute("hatch.patterns", &json!({ "text": "*ΣΟΛΙΔ, test\n0,0,0,0,1\n" })).unwrap();
    let asked = ["ΣΟΛΙΔ", "σολιδ", "σολίδ"].map(|typed| {
        s.start("hatch").unwrap();
        s.input(cadcraft_engine::Input::Keyword("Pattern".into())).unwrap();
        s.cmdline(typed).unwrap();
        let asked = s.prompt_text();
        s.cancel();
        asked
    });
    s.execute("hatch.patterns", &json!({ "clear": true })).unwrap();
    assert!(asked.iter().all(|a| a.contains("scale")), "the loaded pattern, in any spelling: {asked:?}");
}

/// A LISP command with a Greek name runs when typed in upper case, lower case or with an accent.
#[test]
fn a_greek_lisp_command_runs_in_any_case_or_accent() {
    for typed in ["ΛΙΝΕ", "λινε", "λίνε"] {
        let mut s = Session::new();
        s.cmdline("(defun c:ΛΙΝΕ () (command \"_.CIRCLE\" '(0 0) 5) (princ))").unwrap();
        s.cmdline(typed).unwrap();
        let circles = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Circle(_))).count();
        assert_eq!((circles, lines(&s)), (1, 0), "{typed}: {}", s.prompt_text());
    }
}

/// A block called Μ opens at BEDIT when its name is typed in either case.
#[test]
fn a_block_opens_in_any_case_or_accent_at_a_prompt_that_also_takes_a_selection() {
    for typed in ["\u{39c}", "\u{3bc}"] {
        let mut s = Session::new();
        let l = s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap()["handles"][0].clone();
        s.execute("block", &json!({ "name": "\u{39c}", "base": [0, 0], "handles": [l] })).unwrap();
        s.start("BEDIT").unwrap();
        s.cmdline(typed).unwrap();
        let open = s.state().unwrap().block_editor().map(str::to_string);
        assert_eq!(open.as_deref(), Some("\u{39c}"), "{typed}: {}", s.prompt_text());
    }
}

/// Σ is the S key, Select at BLOCKREPLACE; once a block has that name, typing it in either case
/// names the block.
#[test]
fn a_block_named_like_the_keys_of_select_is_that_block_at_blockreplace() {
    let mut s = Session::new();
    // Σ is the S key: the Select option of BLOCKREPLACE, while no block has that name.
    s.start("BLOCKREPLACE").unwrap();
    s.cmdline("Σ").unwrap();
    assert!(s.prompt_text().contains("Select a block reference"), "{}", s.prompt_text());
    s.cancel();
    let l = s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap()["handles"][0].clone();
    s.execute("block", &json!({ "name": "Σ", "base": [0, 0], "handles": [l] })).unwrap();
    s.execute("insert", &json!({ "name": "Σ", "at": [0, 0] })).unwrap();
    for typed in ["Σ", "σ"] {
        s.start("BLOCKREPLACE").unwrap();
        s.cmdline(typed).unwrap();
        let asked = s.prompt_text();
        s.cancel();
        assert!(asked.contains("replacement block for Σ"), "{typed}: {asked}");
    }
}

/// Λ is the L key. At COUNT's "Select object to count or [List/Spaces]" the Latin L is the selection
/// word Last, not the List option: the Greek key is read the same way, and so is the whole word.
#[test]
fn a_greek_selection_word_wins_over_a_keyword_on_the_same_key() {
    let answer = |typed: &str| {
        let mut s = Session::new();
        s.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap();
        s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap();
        s.start("COUNT").unwrap();
        let from = s.log.len();
        s.cmdline(typed).unwrap();
        s.log[from..].join("\n")
    };
    let last = answer("L");
    assert!(last.contains("like the selected one"), "L counts the objects like the last one: {last}");
    assert_ne!(last, answer("List"));
    for typed in ["Λ", "λ", "ΛΑΣΤ"] {
        assert_eq!(answer(typed), last, "{typed}");
    }
}
