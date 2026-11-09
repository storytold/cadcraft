//! A shared session's command line: a complete line with explicit addresses becomes an operation,
//! two replicas that type lines converge on the same drawing, a refused line changes nothing, and
//! applying another participant's change keeps this participant's cursor and selection.
//!
//! The authoritative apply is [`Session::execute_intent`] on a headless session, as the room runs
//! it; every participant (the author included) reaches the result through the change set, which is
//! what the server sends (ADR-0001, ADR-0004).

use cadcraft_collab::cmdline;
use cadcraft_collab::diff;
use cadcraft_doc::Space;
use cadcraft_engine::Session;
use serde_json::json;

/// Apply `text`, typed by `author`, to the authoritative session and send the change set to every
/// replica. Returns the number of changes the operation produced.
fn submit(server: &mut Session, replicas: &mut [&mut Session], author: usize, text: &str) -> usize {
    let op = {
        let state = replicas[author].state().unwrap();
        cmdline::op_for(text, &state.doc, &state.space, &format!("local-{author}"), 0).unwrap()
    };
    let before = server.doc().unwrap().clone();
    server.execute_intent(&op.command, &op.params, &op.context).unwrap();
    let after = server.doc().unwrap();
    let changes = diff(&before, after, &Space::Model).unwrap();
    let n = changes.len();
    for replica in replicas.iter_mut() {
        replica.apply_remote(&changes).unwrap();
    }
    n
}

#[test]
fn two_replicas_that_type_commands_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    // Each one types its own command; the result reaches both through the change set.
    submit(&mut server, &mut [&mut alice, &mut bob], 0, "line 0,0 10,0");
    submit(&mut server, &mut [&mut alice, &mut bob], 1, "circle 5,5 2");

    // Bob addresses Alice's line by the handle the change set gave him; no selection is involved.
    let line = bob.doc().unwrap().model.iter().next().expect("the line arrived").handle;
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("move {} delta 10,0", line.hex()));

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap(), "and agree with the authoritative drawing");
    assert_eq!(alice.doc().unwrap().entity_count(), 2);
}

#[test]
fn a_refused_line_does_not_change_the_drawing() {
    let mut session = Session::new();
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    let state = session.state().unwrap();
    let before = state.doc.clone();
    let before_revision = state.revision;

    let error = cmdline::op_for("explode 1FF", &state.doc, &state.space, "local-0", 0).unwrap_err();

    assert!(error.to_string().contains("line, circle or move"), "{error}");
    assert_eq!(session.doc().unwrap(), &*before, "a refused line never becomes an operation");
    assert_eq!(session.state().unwrap().revision, before_revision, "and does not redraw");
}

#[test]
fn an_unaddressable_move_is_refused_before_it_is_sent() {
    let mut session = Session::new();
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    let state = session.state().unwrap();
    let before = state.doc.clone();

    let error = cmdline::op_for("move delta 10,0", &state.doc, &state.space, "local-0", 0).unwrap_err();

    assert!(error.to_string().contains("address the objects"), "{error}");
    assert_eq!(session.doc().unwrap(), &*before);
}

#[test]
fn a_remote_change_keeps_the_receivers_cursor_and_selection() {
    let mut server = Session::new();
    let mut receiver = Session::new();
    // Seed the receiver with one line through the same path an operation takes.
    let empty = receiver.doc().unwrap().clone();
    server.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    let created = diff(&empty, server.doc().unwrap(), &Space::Model).unwrap();
    receiver.apply_remote(&created).unwrap();
    let line = receiver.doc().unwrap().model.iter().next().expect("the line arrived").handle;
    receiver.set_selection(vec![line]);
    receiver.cursor = cadcraft_geom::Vec2::new(3.0, 4.0);

    // Another participant changes the same entity and adds a new one.
    let before = server.doc().unwrap().clone();
    server.execute("move", &json!({"handles": [line.hex()], "delta": [5.0, 0.0]})).unwrap();
    server.execute("circle", &json!({"center": [50.0, 50.0], "radius": 1.0})).unwrap();
    let remote = diff(&before, server.doc().unwrap(), &Space::Model).unwrap();
    assert!(remote.iter().any(|change| matches!(change, cadcraft_collab::Change::EntityReplace { handle, .. } if *handle == line)));

    receiver.apply_remote(&remote).unwrap();

    assert_eq!(receiver.selection(), vec![line], "a replaced entity keeps its place in the selection");
    assert_eq!(receiver.cursor, cadcraft_geom::Vec2::new(3.0, 4.0), "the cursor does not follow a remote change");
    assert_eq!(receiver.doc().unwrap(), server.doc().unwrap());
}
