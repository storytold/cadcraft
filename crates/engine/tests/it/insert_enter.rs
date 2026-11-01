//! Enter or Space at INSERT's insertion point prompt ends the command, as in AutoCAD, instead of
//! asking again (the Blocks palette's Repeat Placement stops this way).

use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::json;

#[test]
fn enter_at_the_insertion_point_ends_insert() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": "A", "base": [0, 0], "handles": hs, "keep": "delete"})).unwrap();

    s.start("insert").unwrap();
    s.input(Input::Text("A".into())).unwrap();
    assert!(s.prompt_text().starts_with("INSERT Specify insertion point"), "{}", s.prompt_text());
    s.input(Input::Enter).unwrap();
    assert_eq!(s.prompt_text(), "Command:");
    assert_eq!(s.doc().unwrap().model.iter().count(), 0, "nothing is placed");
    assert_eq!(s.last_finished, None, "ending it this way is cancelling it");
}

/// How the last command ended: by itself, with what it added; a cancelled one changes nothing.
#[test]
fn the_last_finished_command_says_whether_it_added_objects() {
    let mut s = Session::new();
    s.start("line").unwrap();
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    s.input(Input::Point(Vec2::new(1.0, 1.0))).unwrap();
    s.input(Input::Enter).unwrap();
    let line = (s.commands_started, true);
    assert_eq!(s.last_finished, Some(line));
    s.start("circle").unwrap();
    s.input(Input::Cancel).unwrap();
    assert_eq!(s.last_finished, Some(line));
}

/// A transparent command runs inside the current one: it does not count as a new command.
#[test]
fn only_a_command_of_its_own_counts_as_started() {
    let mut s = Session::new();
    s.start("line").unwrap();
    let started = s.commands_started;
    s.start("'zoom").unwrap();
    assert_eq!(s.commands_started, started);
    s.start("circle").unwrap();
    assert_eq!(s.commands_started, started + 1);
}
