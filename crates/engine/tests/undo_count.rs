//! UNDO takes the number of operations to undo; U (Edit > Undo, Cmd+Z) undoes one at once.

use cadcraft_engine::Session;

fn count(s: &Session) -> usize {
    s.doc().unwrap().model.len()
}

fn session_with_circles(n: usize) -> Session {
    let mut s = Session::new();
    for i in 0..n {
        s.cmdline(&format!("CIRCLE {},0 1", i * 3)).unwrap();
    }
    assert_eq!(count(&s), n);
    s
}

#[test]
fn undo_takes_a_count() {
    let mut s = session_with_circles(4);
    s.script("UNDO 3\nCIRCLE 20,0 1\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    // Three undone, then the next line runs as a command: 4 - 3 + 1.
    assert_eq!(count(&s), 2, "{:?}", s.log);
    // The undone steps can be redone, and UNDO itself adds no undo step.
    let mut s = session_with_circles(4);
    s.cmdline("UNDO 3").unwrap();
    assert_eq!(count(&s), 1);
    s.execute("redo", &serde_json::json!({ "count": 3 })).unwrap();
    assert_eq!(count(&s), 4);
    // Enter takes the default of one.
    s.cmdline("UNDO").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(count(&s), 3);
}

#[test]
fn undo_options_and_u() {
    // An option that is not available yet ends UNDO without undoing; the next token is its value
    // (Auto ON) or the next command.
    let mut s = session_with_circles(2);
    s.script("UNDO A ON\nUNDO M\nCIRCLE 20,0 1\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(count(&s), 3, "{:?}", s.log);
    // U undoes one operation without asking.
    s.cmdline("U").unwrap();
    assert!(s.running.is_none());
    assert_eq!(count(&s), 2);
}
