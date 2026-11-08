//! Recent Commands for the shortcut menus (#512): the commands started most recently, newest
//! first, each once.

use cadcraft_engine::Session;

#[test]
fn recent_commands_newest_first_without_repeats() {
    let mut s = Session::new();
    assert!(s.recent_commands(5).is_empty());
    for line in ["LINE 0,0 10,0", "", "CIRCLE 0,0 5", "L 0,0 0,10", "", "ZOOM E"] {
        s.cmdline(line).unwrap();
    }
    assert_eq!(s.recent_commands(5), ["zoom", "line", "circle"]);
    assert_eq!(s.recent_commands(2), ["zoom", "line"]);
    assert!(s.recent_commands(0).is_empty());
    // Unknown names never get in.
    assert!(s.cmdline("NOSUCHCOMMAND").is_err());
    s.echo("Command: NOSUCHCOMMAND");
    assert_eq!(s.recent_commands(5), ["zoom", "line", "circle"]);
}

#[test]
fn recent_commands_include_the_drawings_lisp_commands() {
    let mut s = Session::new();
    s.cmdline("(defun c:pidhola () (princ))").unwrap();
    for line in ["LINE 0,0 10,0", "", "PIDHOLA", "pidhola"] {
        s.cmdline(line).unwrap();
    }
    assert!(s.is_lisp_command("PIDHOLA") && s.is_lisp_command("pidhola") && !s.is_lisp_command("line"));
    assert_eq!(s.recent_commands(5), ["pidhola", "line"]);
    // Once the drawing no longer defines it, it leaves the list.
    s.cmdline("(setq c:pidhola nil)").unwrap();
    assert_eq!(s.recent_commands(5), ["line"]);
}

/// START gives a registered alias precedence, so a `c:` function named like one is not a command
/// it can run; nor is one whose name starts with a prefix START takes off.
#[test]
fn a_function_start_would_not_run_is_not_a_lisp_command() {
    let mut s = Session::new();
    s.cmdline("(defun c:c () (princ))").unwrap();
    s.cmdline("(defun c:-pid () (princ))").unwrap();
    s.cmdline("(defun c:pid () (princ))").unwrap();
    assert!(!s.is_lisp_command("c"), "C is CIRCLE's alias");
    assert!(!s.is_lisp_command("-pid"));
    assert!(s.is_lisp_command("pid"));
}
