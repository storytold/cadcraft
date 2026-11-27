//! Transparent commands with their own prompts run nested inside the active command.

use cadcraft_engine::Session;

fn lines(s: &Session) -> usize {
    s.doc().map(|d| d.entity_count()).unwrap_or(0)
}

#[test]
fn transparent_zoom_resumes_the_running_command() {
    let mut s = Session::new();
    s.cmdline("LINE 0,0").unwrap();
    s.cmdline("'ZOOM").unwrap();
    // The nested command prompts; the outer one is suspended, not cancelled.
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("zoom"));
    assert!(s.prompt_text().starts_with(">>"), "{}", s.prompt_text());
    assert!(!s.log.iter().any(|l| l.contains("*Cancel*")), "{:?}", s.log);
    s.cmdline("W").unwrap();
    s.cmdline("-5,-5").unwrap();
    s.cmdline("15,15").unwrap();
    // ZOOM ended: LINE prompts again for the next point.
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("line"), "{}", s.prompt_text());
    assert!(!s.prompt_text().starts_with(">>"));
    s.cmdline("10,0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(lines(&s), 1);
    // Enter repeats the outer command, not the transparent one.
    assert_eq!(s.last_command.as_deref(), Some("line"));
    s.cmdline("").unwrap();
    s.cancel();
    // Scripts nest the same way.
    s.script("LINE 0,0 '-PAN 0,0 1,1 5,0\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), 2);
    // Enter ends the nested command; Esc and a new command cancel both levels.
    s.cmdline("CIRCLE 0,0").unwrap();
    s.cmdline("'PAN").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("circle"));
    s.cmdline("'ZOOM").unwrap();
    s.cancel();
    assert!(s.running.is_none() && s.suspended.is_none());
    s.cmdline("CIRCLE 0,0").unwrap();
    s.cmdline("'ZOOM").unwrap();
    s.start("LINE").unwrap();
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("line"));
    assert!(s.suspended.is_none());
}

/// #3273 review: GETVAR typed transparently inside LINE reads the command state LINE is in, and
/// LINE then carries on.
#[test]
fn transparent_getvar_sees_the_command_it_runs_inside() {
    let mut s = Session::new();
    s.cmdline("LINE 0,0").unwrap();
    s.cmdline("'GETVAR CMDNAMES").unwrap();
    // The value, then LINE resuming.
    let tail = |s: &Session| s.log[s.log.len().saturating_sub(2)..].to_vec();
    assert_eq!(tail(&s), ["CMDNAMES = LINE", "Resuming LINE command."]);
    s.cmdline("'GETVAR CMDACTIVE").unwrap();
    assert_eq!(tail(&s), ["CMDACTIVE = 3", "Resuming LINE command."], "a command, and a transparent one in it");
    // A transparent command still prompting is named after the one it runs inside.
    s.cmdline("'PAN").unwrap();
    assert_eq!(cadcraft_engine::sysvars::get(&s, "CMDNAMES"), Some(serde_json::json!("LINE'PAN")));
    s.cmdline("").unwrap();
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("line"), "{}", s.prompt_text());
    s.cmdline("10,0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(lines(&s), 1);
    assert_eq!(cadcraft_engine::sysvars::get(&s, "CMDACTIVE"), Some(serde_json::json!(0)));
}
