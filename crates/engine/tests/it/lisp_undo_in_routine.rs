//! U, UNDO and REDO inside a LISP routine, as AutoCAD 2024 runs them (accoreconsole: a line A, a
//! line B, a circle C and a line D, then one routine, then the drawing's `entnext` order read back).

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{EntityCodec, set_entity_codec};
use cadcraft_engine::doc::Handle;
use serde_json::json;

/// A session with A, B, C and D in draw order, each its own undo step, and their handles.
fn drawing() -> (Session, Vec<Handle>) {
    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.execute("line", &json!({ "points": [[0, 10], [10, 10]] })).unwrap();
    s.execute("circle", &json!({ "center": [5, 25], "radius": 3 })).unwrap();
    s.execute("line", &json!({ "points": [[0, 40], [10, 40]] })).unwrap();
    let hs = handles(&s);
    (s, hs)
}

fn handles(s: &Session) -> Vec<Handle> {
    s.doc().unwrap().model.iter().map(|e| e.handle).collect()
}

/// The drawing's objects in draw order: A to D by their handles, E for any other.
fn order(s: &Session, hs: &[Handle]) -> String {
    let name = |h: &Handle| hs.iter().position(|x| x == h).map_or('E', |i| (b'A' + i as u8) as char);
    handles(s).iter().map(|h| name(h).to_string()).collect::<Vec<_>>().join(" ")
}

/// Run `parts` as ONE routine, `@A`…`@D` standing for the objects.
fn routine(s: &mut Session, hs: &[Handle], parts: &[&str]) {
    let mut code = format!("(progn {})", parts.join(" "));
    for (i, h) in hs.iter().enumerate() {
        code = code.replace(&format!("@{}", (b'A' + i as u8) as char), &format!("(handent \"{}\")", h.hex()));
    }
    s.cmdline(&code).unwrap();
    assert!(s.running.is_none(), "{code}: {}", s.prompt_text());
}

const EB: &str = "(command \"_.ERASE\" @B \"\")";
const EC: &str = "(command \"_.ERASE\" @C \"\")";
const U: &str = "(command \"_.U\")";
const LINE: &str = "(command \"_.LINE\" '(0.0 50.0) '(10.0 50.0) \"\")";
const MAKE: &str = "(entmake '((0 . \"LINE\") (8 . \"0\") (10 0.0 60.0 0.0) (11 10.0 60.0 0.0)))";

#[test]
fn u_in_a_routine_takes_back_the_routines_last_command() {
    // (case, the routine, the drawing's order afterwards)
    let cases: [(&str, &[&str], &str); 9] = [
        ("U-001: ERASE, U", &[EB, U], "A B C D"),
        ("U-002: two erases, U", &[EB, EC, U], "A C D"),
        ("U-003: two erases, U, U", &[EB, EC, U, U], "A B C D"),
        ("U-005: entdel, ERASE, U", &["(entdel @B)", EC, U], "A C D"),
        ("U-006: ERASE, entdel, U", &[EB, "(entdel @C)", U], "A B C D"),
        ("U-007: LINE, U, REDO", &[LINE, U, "(command \"_.REDO\")"], "A B C D E"),
        ("U-008: two erases, UNDO 2", &[EB, EC, "(command \"_.UNDO\" \"2\")"], "A B C D"),
        ("U-012: ERASE, entmake, U", &[EB, MAKE, U], "A B C D"),
        // OOPS-011 had an OOPS after the U, which brought B back: B was still erased.
        ("OOPS-011: ERASE, LINE, U", &[EB, LINE, U], "A C D"),
    ];
    for (case, parts, want) in cases {
        let (mut s, hs) = drawing();
        routine(&mut s, &hs, parts);
        assert_eq!(order(&s, &hs), want, "{case}");
    }
}

#[test]
fn the_routine_is_still_one_undo_step_after_it() {
    let (mut s, hs) = drawing();
    let depth = s.state().unwrap().undo.len();
    routine(&mut s, &hs, &[EB, EC, LINE, U, "(entdel @A)"]);
    assert_eq!(order(&s, &hs), "D");
    assert_eq!(s.state().unwrap().undo.len(), depth + 1);
    s.cmdline("U").unwrap();
    assert_eq!(order(&s, &hs), "A B C D");
    s.cmdline("REDO").unwrap();
    assert_eq!(order(&s, &hs), "D");
}

#[test]
fn u_with_nothing_of_the_routine_left_takes_back_the_step_before_it() {
    let (mut s, hs) = drawing();
    let depth = s.state().unwrap().undo.len();
    routine(&mut s, &hs, &[EB, U, U]);
    // The second U takes back D's line, drawn before the routine.
    assert_eq!(order(&s, &hs), "A B C");
    assert_eq!(s.state().unwrap().undo.len(), depth - 1, "the routine changed nothing more of its own");
    // A command after that makes the routine's step, from where the U took the drawing.
    routine(&mut s, &hs, &[EB, U, U, "(command \"_.ERASE\" @A \"\")"]);
    assert_eq!(order(&s, &hs), "B");
    s.cmdline("U").unwrap();
    assert_eq!(order(&s, &hs), "A B");
}

#[test]
fn undo_off_still_refuses_inside_a_routine() {
    let (mut s, hs) = drawing();
    routine(&mut s, &hs, &["(command \"_.UNDO\" \"_C\" \"_N\")", EB, U]);
    assert_eq!(order(&s, &hs), "A C D");
}

#[test]
fn redo_inside_a_routine_brings_back_the_last_u_first() {
    // U past the routine's own command, then REDO: the drawing's step comes back, not the erase.
    let (mut s, hs) = drawing();
    let depth = s.state().unwrap().undo.len();
    routine(&mut s, &hs, &[EB, U, U, "(command \"_.REDO\")"]);
    assert_eq!(order(&s, &hs), "A B C D");
    assert_eq!(s.state().unwrap().undo.len(), depth, "the routine left nothing of its own to undo");
    // A command inside the routine ends what REDO could bring back from before it.
    let (mut s, hs) = drawing();
    s.cmdline("U").unwrap();
    assert_eq!(order(&s, &hs), "A B C");
    routine(&mut s, &hs, &[EB, "(command \"_.REDO\")"]);
    assert_eq!(order(&s, &hs), "A C");
}

#[test]
fn a_command_without_prompts_is_a_step_of_the_routine_too() {
    // HATCHTOBACK runs as its JSON form from a routine; U takes it back, not the hatch.
    let mut s = Session::new();
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    let rect = handles(&s)[0];
    s.execute("hatch", &json!({ "handles": [rect.hex()], "pattern": "SOLID" })).unwrap();
    let both = handles(&s);
    assert_eq!(both.len(), 2);
    s.cmdline("(progn (command \"_.HATCHTOBACK\") (command \"_.U\"))").unwrap();
    assert_eq!(handles(&s), both);
}

#[test]
fn undo_control_one_keeps_only_the_routines_last_command() {
    // As UNDO Control One keeps only the drawing's last step: the second U goes on to the step
    // before the routine (D's line), the drawing as it was then.
    let (mut s, hs) = drawing();
    routine(&mut s, &hs, &["(command \"_.UNDO\" \"_C\" \"_O\")", EB, EC, U, U]);
    assert_eq!(order(&s, &hs), "A B C");
}

#[test]
fn a_routine_keeps_as_many_steps_as_the_drawing() {
    use cadcraft_engine::undoctl::MAX_UNDO;
    let (mut s, hs) = drawing();
    // Two lines more than the routine keeps steps for, then a U for each kept step and one more,
    // which goes on to the drawing's step before the routine (D's line): the drawing as it was
    // then, without the two lines whose steps went.
    let code =
        format!("(progn (repeat {} (command \"_.LINE\" '(0.0 50.0) '(10.0 50.0) \"\")) (repeat {} (command \"_.U\")))", MAX_UNDO + 2, MAX_UNDO + 1);
    s.cmdline(&code).unwrap();
    assert!(s.running.is_none());
    assert_eq!(order(&s, &hs), "A B C");
}
