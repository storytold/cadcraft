//! A prompt with a value in it keeps its source, so the interface can translate it and put the
//! value back (#2611); its English text is what it always was. Prompts that only chose an English
//! word are whole sentences now, which the catalogs can translate as they are.

use cadcraft_engine::Session;

/// The current prompt's message and template source.
fn shown(s: &Session) -> (String, Option<&'static str>) {
    let p = s.current_prompt().expect("a prompt");
    (p.message.clone(), p.template.map(|t| t.source))
}

#[test]
fn setvar_asks_for_a_variable_by_its_name() {
    let mut s = Session::new();
    s.start("setvar").unwrap();
    s.cmdline("filletrad").unwrap();
    assert_eq!(shown(&s), ("Enter new value for FILLETRAD".to_string(), Some("Enter new value for {name}")));
    let values = s.current_prompt().and_then(|p| p.template).map(|t| t.values);
    assert_eq!(values, Some(vec![("name", "FILLETRAD".to_string())]));
}

#[test]
fn a_block_name_is_a_value_of_the_question() {
    let mut s = Session::new();
    s.script("LINE 0,0 10,0\n\n-BLOCK Door 0,0 L\n\nLINE 0,0 0,10\n\n").unwrap();
    s.cmdline("-block").unwrap();
    s.cmdline("door").unwrap();
    assert_eq!(shown(&s), ("Block \"door\" already exists. Redefine it?".to_string(), Some("Block \"{name}\" already exists. Redefine it?")));
    let values = s.current_prompt().and_then(|p| p.template).map(|t| t.values);
    assert_eq!(values, Some(vec![("name", "door".to_string())]));
}

#[test]
fn polyline_widths_are_whole_sentences() {
    let mut s = Session::new();
    s.start("pline").unwrap();
    s.cmdline("0,0").unwrap();
    s.cmdline("w").unwrap();
    assert_eq!(shown(&s), ("Specify starting width".to_string(), None));
    s.cmdline("1").unwrap();
    assert_eq!(shown(&s), ("Specify ending width".to_string(), None));
    s.cmdline("").unwrap();
    s.cmdline("h").unwrap();
    assert_eq!(shown(&s), ("Specify starting half-width".to_string(), None));
    s.cmdline("1").unwrap();
    assert_eq!(shown(&s), ("Specify ending half-width".to_string(), None));
}

#[test]
fn rename_asks_for_the_type_it_renames() {
    let mut s = Session::new();
    s.start("-rename").unwrap();
    s.cmdline("la").unwrap();
    assert_eq!(shown(&s), ("Enter old layer name".to_string(), None));
    s.cmdline("0").unwrap();
    assert_eq!(shown(&s).0, "Enter new layer name");
}
