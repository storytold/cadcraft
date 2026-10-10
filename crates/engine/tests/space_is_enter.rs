//! A space ends an input and an empty input is Enter, in scripts and on the command line alike.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Space};

fn kinds(s: &Session) -> Vec<EntityKind> {
    s.doc().ok().and_then(|d| d.space(&Space::Model)).map(|st| st.iter().map(|e| e.kind.clone()).collect()).unwrap_or_default()
}

fn moved_line(s: &Session) -> bool {
    matches!(kinds(s).as_slice(), [EntityKind::Line(l)] if l.a.x == 5.0 && l.a.y == 5.0)
}

fn text_value(s: &Session) -> Option<String> {
    kinds(s).iter().find_map(|k| match k {
        EntityKind::Text(t) => Some(t.value.clone()),
        _ => None,
    })
}

#[test]
fn trailing_and_double_spaces_are_enter() {
    // Script: the space at the end of `ERASE L ` ends the selection.
    let mut s = Session::new();
    s.script("LINE 0,0 5,5\n\nERASE L \n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(kinds(&s).is_empty());

    // Command line: the same trailing space, and a double space ending the selection mid-line.
    let mut s = Session::new();
    s.cmdline("LINE 0,0 5,5").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("ERASE L ").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(kinds(&s).is_empty());
    let mut s = Session::new();
    s.cmdline("LINE 0,0 5,5").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("MOVE L  0,0 5,5").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(moved_line(&s), "{:?}", kinds(&s));
    // The script path reads it the same way.
    let mut s = Session::new();
    s.script("LINE 0,0 5,5\n\nMOVE L  0,0 5,5\n").unwrap();
    assert!(moved_line(&s), "{:?}", kinds(&s));

    // Free text keeps its spaces: in a script line, and typed at the TEXT prompt.
    let mut s = Session::new();
    s.script("TEXT 0,0 2.5 0 Hello  big world\n").unwrap();
    assert_eq!(text_value(&s).as_deref(), Some("Hello  big world"));
    let mut s = Session::new();
    s.cmdline("TEXT 0,0 2.5 0").unwrap();
    s.cmdline("Hello big world ").unwrap();
    assert_eq!(text_value(&s).as_deref(), Some("Hello big world"));
}
