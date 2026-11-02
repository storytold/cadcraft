//! MTEXT contents from scripts and the command line; TEXTEDIT's prompt and options.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Space};

fn kinds(s: &Session) -> Vec<EntityKind> {
    s.doc().ok().and_then(|d| d.space(&Space::Model)).map(|st| st.iter().map(|e| e.kind.clone()).collect()).unwrap_or_default()
}

fn mtexts(s: &Session) -> Vec<String> {
    kinds(s)
        .into_iter()
        .filter_map(|k| match k {
            EntityKind::MText(t) => Some(t.contents),
            _ => None,
        })
        .collect()
}

fn texts(s: &Session) -> Vec<String> {
    kinds(s)
        .into_iter()
        .filter_map(|k| match k {
            EntityKind::Text(t) => Some(t.value),
            _ => None,
        })
        .collect()
}

#[test]
fn script_mtext_takes_lines_until_an_empty_line() {
    let mut s = Session::new();
    s.script("MTEXT 0,10 20,0\nFirst line\nSecond line\n\nLINE 0,0 5,5\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(mtexts(&s), vec!["First line\\PSecond line".to_string()]);
    assert_eq!(kinds(&s).len(), 2, "{:?}", kinds(&s));

    // Typed at the command line: spaces stay in the text, Enter on an empty line ends it.
    let mut s = Session::new();
    s.cmdline("MTEXT 0,10 20,0").unwrap();
    s.cmdline("Hello world").unwrap();
    assert!(s.running.is_some());
    s.cmdline("").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(mtexts(&s), vec!["Hello world".to_string()]);
}

#[test]
fn textedit_prompt_mode_and_undo() {
    let mut s = Session::new();
    s.cmdline("TEXT 0,0 1 0 old").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("TEXTEDIT").unwrap();
    let p = s.prompt_text();
    assert!(!p.contains("or or"), "{p}");
    // Edit, then Undo restores the old text.
    s.cmdline("0.5,0.5").unwrap();
    s.cmdline("new").unwrap();
    assert_eq!(texts(&s), vec!["new".to_string()]);
    s.cmdline("u").unwrap();
    assert_eq!(texts(&s), vec!["old".to_string()]);
    // Mode Single: the command ends after one edit.
    s.cmdline("m").unwrap();
    s.cmdline("s").unwrap();
    s.cmdline("0.5,0.5").unwrap();
    s.cmdline("once").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(texts(&s), vec!["once".to_string()]);
}
