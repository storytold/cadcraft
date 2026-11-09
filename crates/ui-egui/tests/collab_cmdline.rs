//! A shared session's command line: a complete line with explicit addresses becomes an operation,
//! two replicas that type lines converge on the same drawing, a refused line changes nothing, and
//! applying another participant's change keeps this participant's cursor and selection.
//!
//! The authoritative apply is [`Session::execute_intent`] on a headless session, as the room runs
//! it; every participant (the author included) reaches the result through the change set, which is
//! what the server sends (ADR-0001, ADR-0004).

use cadcraft_collab::cmdline;
use cadcraft_collab::diff;
use cadcraft_doc::{EntityKind, Space};
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
    submit(&mut server, &mut [&mut alice, &mut bob], 0, "polyline 20,0 30,0 30,10");

    // Bob addresses Alice's line by the handle the change set gave him; no selection is involved.
    let line = bob.doc().unwrap().model.iter().next().expect("the line arrived").handle;
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("move {} delta 10,0", line.hex()));

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap(), "and agree with the authoritative drawing");
    assert_eq!(alice.doc().unwrap().entity_count(), 3);
}

#[test]
fn two_replicas_that_erase_and_draw_a_polyline_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    submit(&mut server, &mut [&mut alice, &mut bob], 0, "line 0,0 10,0");
    submit(&mut server, &mut [&mut alice, &mut bob], 0, "polyline 20,0 30,0 30,10");
    assert_eq!(alice.doc().unwrap().entity_count(), 2);

    // Bob erases Alice's line by the handle he got from the change set.
    let line = bob.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).expect("the line arrived").handle;
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("erase {}", line.hex()));

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the erase");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap());
    assert_eq!(alice.doc().unwrap().entity_count(), 1, "only the polyline is left");
    assert!(alice.doc().unwrap().entity(line).is_none());
}

#[test]
fn a_refused_line_does_not_change_the_drawing() {
    let mut session = Session::new();
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    let state = session.state().unwrap();
    let before = state.doc.clone();
    let before_revision = state.revision;

    let error = cmdline::op_for("explode 1FF", &state.doc, &state.space, "local-0", 0).unwrap_err();

    assert!(
        error.to_string().contains("line, circle, polyline, ellipse, rect, move, copy, rotate, mirror, scale, offset, trim, array or erase"),
        "{error}"
    );
    assert_eq!(session.doc().unwrap(), &*before, "a refused line never becomes an operation");
    assert_eq!(session.state().unwrap().revision, before_revision, "and does not redraw");
}

#[test]
fn a_refused_rect_copy_or_rotate_does_not_change_the_drawing() {
    let mut session = Session::new();
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    let state = session.state().unwrap();
    let before = state.doc.clone();
    let before_revision = state.revision;
    let line = state.doc.model.iter().next().expect("the line").handle.hex();

    // A rectangle with no area; handles that name nothing; a rotate without a base point.
    let refused = ["rect 0,0 0,5".to_string(), "copy FF 1,1".to_string(), format!("rotate {line} base 0,0"), "rotate FF base 0,0 45".to_string()];
    for text in &refused {
        let error = cmdline::op_for(text, &state.doc, &state.space, "local-0", 0).unwrap_err();
        assert!(!error.to_string().is_empty(), "{text}");
        assert_eq!(session.doc().unwrap(), &*before, "`{text}` never becomes an operation");
        assert_eq!(session.state().unwrap().revision, before_revision, "`{text}` does not redraw");
    }
}

#[test]
fn a_refused_ellipse_mirror_or_scale_does_not_change_the_drawing() {
    let mut session = Session::new();
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    let state = session.state().unwrap();
    let before = state.doc.clone();
    let before_revision = state.revision;
    let line = state.doc.model.iter().next().expect("the line").handle.hex();

    // Radii that are not ellipses; a mirror line with no direction; an unaddressable or
    // non-positive scale.
    let refused = [
        "ellipse 0,0".to_string(),
        "ellipse 0,0 0".to_string(),
        "ellipse 0,0 3,5".to_string(),
        "ellipse 0,0 5,3 later".to_string(),
        format!("mirror {line} 5,5 5,5"),
        format!("mirror {line} 0,0"),
        "mirror FF 0,0 1,1".to_string(),
        format!("scale {line} base 0,0 0"),
        format!("scale {line} base 0,0 -1"),
        format!("scale {line} 0,0 2"),
        "scale FF base 0,0 2".to_string(),
    ];
    for text in &refused {
        let error = cmdline::op_for(text, &state.doc, &state.space, "local-0", 0).unwrap_err();
        assert!(!error.to_string().is_empty(), "{text}");
        assert_eq!(session.doc().unwrap(), &*before, "`{text}` never becomes an operation");
        assert_eq!(session.state().unwrap().revision, before_revision, "`{text}` does not redraw");
    }
}

#[test]
fn a_refused_offset_trim_or_array_does_not_change_the_drawing() {
    let mut session = Session::new();
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    session.execute("line", &json!({"points": [[0.0, 100.0], [10.0, 100.0]]})).unwrap();
    let state = session.state().unwrap();
    let before = state.doc.clone();
    let before_revision = state.revision;
    let line = state.doc.model.iter().next().expect("the line").handle.hex();
    let far = state.doc.model.iter().nth(1).expect("the far line").handle.hex();

    // A zero or negative offset, a trim with no crossing, a self-edge, counts of zero, a one-item
    // array, a zero step and handles that name nothing: none of them becomes an operation.
    let refused = [
        "offset FF distance 2 side 0,1".to_string(),
        format!("offset {line} distance 0 side 0,1"),
        format!("offset {line} distance -2 side 0,1"),
        format!("trim {line} edge {far} at 8,0"),
        format!("trim {line} edge {line} at 8,0"),
        "trim FF edge 1A3 at 0,0".to_string(),
        format!("array {line} rows 0 cols 3 step 10,10"),
        format!("array {line} rows 2 cols 0 step 10,10"),
        format!("array {line} rows 1 cols 1 step 10,10"),
        format!("array {line} rows 2 cols 3 step 0,10"),
        "array FF rows 2 cols 3 step 10,10".to_string(),
    ];
    for text in &refused {
        let error = cmdline::op_for(text, &state.doc, &state.space, "local-0", 0).unwrap_err();
        assert!(!error.to_string().is_empty(), "{text}");
        assert_eq!(session.doc().unwrap(), &*before, "`{text}` never becomes an operation");
        assert_eq!(session.state().unwrap().revision, before_revision, "`{text}` does not redraw");
    }
}

#[test]
fn an_erase_of_an_unknown_handle_is_refused_before_it_is_sent() {
    let mut session = Session::new();
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    let state = session.state().unwrap();
    let before = state.doc.clone();

    let error = cmdline::op_for("erase 1FF", &state.doc, &state.space, "local-0", 0).unwrap_err();

    assert!(error.to_string().contains("erase: there is no object 1FF"), "{error}");
    assert_eq!(session.doc().unwrap(), &*before);
}

#[test]
fn undo_of_an_erase_brings_the_entity_back() {
    let mut session = Session::new();
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    let line = session.doc().unwrap().model.iter().next().expect("the line").handle;

    // The step goes through the same operation the command line builds.
    let op = cmdline::op_for(&format!("erase {}", line.hex()), session.doc().unwrap(), &Space::Model, "local-0", 0).unwrap();
    session.execute(&op.command, &op.params).unwrap();
    assert!(session.doc().unwrap().entity(line).is_none(), "the erase removed the line");

    assert_eq!(session.undo().unwrap().as_deref(), Some("Erase"));
    assert!(session.doc().unwrap().entity(line).is_some(), "undo brings the line back");
}

#[test]
fn a_remote_erase_and_polyline_keep_the_receivers_cursor_and_selection() {
    let mut server = Session::new();
    let mut receiver = Session::new();
    let empty = receiver.doc().unwrap().clone();
    server.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    server.execute("line", &json!({"points": [[20.0, 0.0], [30.0, 0.0]]})).unwrap();
    let created = diff(&empty, server.doc().unwrap(), &Space::Model).unwrap();
    receiver.apply_remote(&created).unwrap();
    let kept = receiver.doc().unwrap().model.iter().next().expect("the line arrived").handle;
    let erased = receiver.doc().unwrap().model.iter().nth(1).expect("the second line arrived").handle;
    receiver.set_selection(vec![kept]);
    receiver.cursor = cadcraft_geom::Vec2::new(3.0, 4.0);

    // Another participant erases one entity and draws a polyline.
    let before = server.doc().unwrap().clone();
    server.execute("erase", &json!({"handles": [erased.hex()]})).unwrap();
    server.execute("pline", &json!({"vertices": [[50.0, 50.0], [60.0, 50.0], [60.0, 60.0]]})).unwrap();
    let remote = diff(&before, server.doc().unwrap(), &Space::Model).unwrap();
    assert!(remote.iter().any(|change| matches!(change, cadcraft_collab::Change::EntityDelete { handle } if *handle == erased)));

    receiver.apply_remote(&remote).unwrap();

    assert_eq!(receiver.selection(), vec![kept], "an entity that survives the change stays selected");
    assert_eq!(receiver.cursor, cadcraft_geom::Vec2::new(3.0, 4.0), "the cursor does not follow a remote change");
    assert_eq!(receiver.doc().unwrap(), server.doc().unwrap());
}

#[test]
fn two_replicas_that_draw_a_rect_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    submit(&mut server, &mut [&mut alice, &mut bob], 0, "rect 0,0 10,5");

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the rectangle");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap(), "and agree with the authoritative drawing");
    assert_eq!(alice.doc().unwrap().entity_count(), 1, "a rectangle is one object");
    let rect = alice.doc().unwrap().model.iter().next().expect("the rectangle arrived");
    match &rect.kind {
        EntityKind::LwPolyline(pl) => {
            assert!(pl.closed, "the rectangle is a closed polyline");
            assert_eq!(pl.vertices.len(), 4, "four corners");
        }
        other => panic!("a rectangle should be a closed polyline, got {other:?}"),
    }
}

#[test]
fn two_replicas_that_copy_and_rotate_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    submit(&mut server, &mut [&mut alice, &mut bob], 0, "rect 0,0 10,5");
    // Bob copies the rectangle by the handle he got from the change set; no selection is involved.
    let rect = bob.doc().unwrap().model.iter().next().expect("the rectangle arrived").handle;
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("copy {} 20,0", rect.hex()));
    // Alice rotates both rectangles a quarter turn about the origin.
    let both = alice.doc().unwrap().model.iter().map(|entity| entity.handle.hex()).collect::<Vec<_>>().join(",");
    submit(&mut server, &mut [&mut alice, &mut bob], 0, &format!("rotate {both} base 0,0 90"));

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the copy and the rotation");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap());
    assert_eq!(alice.doc().unwrap().entity_count(), 2, "the copy is a second object");
    for entity in alice.doc().unwrap().model.iter() {
        let EntityKind::LwPolyline(pl) = &entity.kind else { panic!("a rotated rectangle is still a polyline") };
        for vertex in &pl.vertices {
            assert!(vertex.p.x <= 1e-9, "a quarter turn puts every corner at x <= 0, got {:?}", vertex.p);
            assert!(vertex.p.y >= -1e-9, "a quarter turn puts every corner at y >= 0, got {:?}", vertex.p);
        }
    }
}

#[test]
fn two_replicas_that_draw_an_ellipse_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    submit(&mut server, &mut [&mut alice, &mut bob], 0, "ellipse 10,20 5,3 30");

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the ellipse");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap(), "and agree with the authoritative drawing");
    assert_eq!(alice.doc().unwrap().entity_count(), 1, "an ellipse is one object");
    let ellipse = alice.doc().unwrap().model.iter().next().expect("the ellipse arrived");
    let EntityKind::Ellipse(e) = &ellipse.kind else { panic!("expected an ellipse, got {:?}", ellipse.kind) };
    assert!((e.ratio - 0.6).abs() < 1e-12, "the minor/major ratio, got {}", e.ratio);
    assert!((e.center.x - 10.0).abs() < 1e-12 && (e.center.y - 20.0).abs() < 1e-12, "the centre stays put, got {:?}", e.center);
    assert!((e.major.x.hypot(e.major.y) - 5.0).abs() < 1e-12, "the major radius, got {:?}", e.major);
}

#[test]
fn two_replicas_that_mirror_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    submit(&mut server, &mut [&mut alice, &mut bob], 0, "rect 0,0 10,5");
    // Bob mirrors Alice's rectangle across the x axis by the handle the change set gave him.
    let rect = bob.doc().unwrap().model.iter().next().expect("the rectangle arrived").handle;
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("mirror {} 0,0 1,0", rect.hex()));

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the mirror");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap());
    assert_eq!(alice.doc().unwrap().entity_count(), 2, "the source stays and the mirror is a second object");
    assert!(alice.doc().unwrap().entity(rect).is_some(), "the source rectangle is not erased");
    let mirrored = alice.doc().unwrap().model.iter().find(|entity| entity.handle != rect).expect("the mirrored copy arrived");
    let EntityKind::LwPolyline(pl) = &mirrored.kind else { panic!("a mirrored rectangle is still a polyline") };
    for vertex in &pl.vertices {
        assert!(vertex.p.y <= 1e-9, "the mirror across the x axis puts every corner at y <= 0, got {:?}", vertex.p);
    }
}

#[test]
fn two_replicas_that_scale_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    submit(&mut server, &mut [&mut alice, &mut bob], 0, "rect 0,0 10,5");
    let rect = bob.doc().unwrap().model.iter().next().expect("the rectangle arrived").handle;
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("scale {} base 0,0 2", rect.hex()));

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the scale");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap());
    assert_eq!(alice.doc().unwrap().entity_count(), 1, "a scale changes the object in place");
    let EntityKind::LwPolyline(pl) = &alice.doc().unwrap().entity(rect).expect("the rectangle is still there").kind else {
        panic!("a scaled rectangle is still a polyline")
    };
    for vertex in &pl.vertices {
        assert!(vertex.p.x.abs() <= 20.0 + 1e-9 && vertex.p.y.abs() <= 10.0 + 1e-9, "twice as large, got {:?}", vertex.p);
    }
    assert!(pl.vertices.iter().any(|vertex| (vertex.p.x - 20.0).abs() < 1e-9), "the far corner moved to x = 20");
}

#[test]
fn two_replicas_that_offset_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    submit(&mut server, &mut [&mut alice, &mut bob], 0, "line 0,0 10,0");
    let line = bob.doc().unwrap().model.iter().next().expect("the line arrived").handle;
    // Bob offsets Alice's line by two units towards positive y.
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("offset {} distance 2 side 0,1", line.hex()));

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the offset");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap(), "and agree with the authoritative drawing");
    assert_eq!(alice.doc().unwrap().entity_count(), 2, "the source stays and the offset is a second line");
    assert!(alice.doc().unwrap().entity(line).is_some(), "the source line stays");
    assert!(
        alice.doc().unwrap().model.iter().any(|entity| {
            let EntityKind::Line(l) = &entity.kind else { return false };
            (l.a.y - 2.0).abs() < 1e-9 && (l.b.y - 2.0).abs() < 1e-9
        }),
        "the offset copy is two units away"
    );
}

#[test]
fn two_replicas_that_trim_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    submit(&mut server, &mut [&mut alice, &mut bob], 0, "line 0,0 10,0");
    submit(&mut server, &mut [&mut alice, &mut bob], 1, "line 5,-5 5,5");
    let target = bob.doc().unwrap().model.iter().next().expect("the line arrived").handle;
    let edge = bob.doc().unwrap().model.iter().nth(1).expect("the edge arrived").handle;
    // Alice trims the line at the crossing with the edge, removing the piece holding 8,0.
    submit(&mut server, &mut [&mut alice, &mut bob], 0, &format!("trim {} edge {} at 8,0", target.hex(), edge.hex()));

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the trim");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap(), "and agree with the authoritative drawing");
    assert_eq!(alice.doc().unwrap().entity_count(), 2, "the trimmed line keeps its handle and the edge stays");
    let EntityKind::Line(l) = &alice.doc().unwrap().entity(target).expect("the trimmed line is still there").kind else {
        panic!("a trimmed line is still a line")
    };
    assert!((l.b.x - 5.0).abs() < 1e-9, "the piece past the crossing was removed, got {:?}", l.b);
}

#[test]
fn two_replicas_that_array_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    submit(&mut server, &mut [&mut alice, &mut bob], 0, "circle 0,0 1");
    let circle = bob.doc().unwrap().model.iter().next().expect("the circle arrived").handle;
    // Two rows and three columns: the source plus five copies.
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("array {} rows 2 cols 3 step 20,10", circle.hex()));

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the array");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap(), "and agree with the authoritative drawing");
    assert_eq!(alice.doc().unwrap().entity_count(), 6, "six items in all");
    let centres: Vec<(f64, f64)> = alice
        .doc()
        .unwrap()
        .model
        .iter()
        .map(|entity| {
            let EntityKind::Circle(c) = &entity.kind else { panic!("an array of circles is circles") };
            (c.center.x, c.center.y)
        })
        .collect();
    for expected in [(0.0, 0.0), (20.0, 0.0), (40.0, 0.0), (0.0, 10.0), (20.0, 10.0), (40.0, 10.0)] {
        assert!(
            centres.iter().any(|got| (got.0 - expected.0).abs() < 1e-9 && (got.1 - expected.1).abs() < 1e-9),
            "missing the item at {expected:?} in {centres:?}"
        );
    }
}

#[test]
fn undo_of_each_new_command_returns_the_state_before_it() {
    let mut session = Session::new();
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    let line = session.doc().unwrap().model.iter().next().expect("the line").handle;

    // The rectangle is a step of its own.
    let before = session.doc().unwrap().clone();
    run(&mut session, "rect 0,0 10,5");
    assert_eq!(session.doc().unwrap().entity_count(), 2);
    assert_eq!(session.undo().unwrap().as_deref(), Some("Rectangle"));
    assert_eq!(session.doc().unwrap(), &before, "undo of the rectangle returns the drawing before it");

    // So is the copy.
    let before = session.doc().unwrap().clone();
    run(&mut session, &format!("copy {} 5,0", line.hex()));
    assert_eq!(session.doc().unwrap().entity_count(), 2);
    assert_eq!(session.undo().unwrap().as_deref(), Some("Copy"));
    assert_eq!(session.doc().unwrap(), &before, "undo of the copy removes the copy");

    // And the rotation puts the line back where it was.
    let before = session.doc().unwrap().clone();
    run(&mut session, &format!("rotate {} base 0,0 45", line.hex()));
    assert_ne!(session.doc().unwrap(), &before, "the rotation moved the line");
    assert_eq!(session.undo().unwrap().as_deref(), Some("Rotate"));
    assert_eq!(session.doc().unwrap(), &before, "undo of the rotation puts the line back");
}

#[test]
fn undo_of_ellipse_mirror_and_scale_returns_the_state_before_it() {
    let mut session = Session::new();
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    let line = session.doc().unwrap().model.iter().next().expect("the line").handle;

    // The ellipse is a step of its own.
    let before = session.doc().unwrap().clone();
    run(&mut session, "ellipse 10,10 4,2");
    assert_eq!(session.doc().unwrap().entity_count(), 2);
    assert_eq!(session.undo().unwrap().as_deref(), Some("Ellipse"));
    assert_eq!(session.doc().unwrap(), &before, "undo of the ellipse returns the drawing before it");

    // So is the mirror: the copy goes and the source stays.
    let before = session.doc().unwrap().clone();
    run(&mut session, &format!("mirror {} 0,0 0,1", line.hex()));
    assert_eq!(session.doc().unwrap().entity_count(), 2);
    assert_eq!(session.undo().unwrap().as_deref(), Some("Mirror"));
    assert_eq!(session.doc().unwrap(), &before, "undo of the mirror removes the copy");

    // And the scale puts the line's endpoints back.
    let before = session.doc().unwrap().clone();
    run(&mut session, &format!("scale {} base 0,0 3", line.hex()));
    assert_ne!(session.doc().unwrap(), &before, "the scale moved the line's far end");
    assert_eq!(session.undo().unwrap().as_deref(), Some("Scale"));
    assert_eq!(session.doc().unwrap(), &before, "undo of the scale restores the line");
}

#[test]
fn undo_and_redo_of_offset_trim_and_array_return_the_state_before_and_after() {
    // Offset: one step, undone and redone as a whole.
    let mut session = Session::new();
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    let line = session.doc().unwrap().model.iter().next().expect("the line").handle;
    let before = session.doc().unwrap().clone();
    run(&mut session, &format!("offset {} distance 2 side 0,1", line.hex()));
    let after = session.doc().unwrap().clone();
    assert_ne!(&after, &before);
    assert_eq!(session.undo().unwrap().as_deref(), Some("Offset"));
    assert_eq!(session.doc().unwrap(), &before, "undo of the offset removes the copy");
    assert_eq!(session.redo().unwrap().as_deref(), Some("Offset"));
    assert_eq!(session.doc().unwrap(), &after, "redo puts the copy back");

    // Trim: the cut is one step; redo cuts again.
    let mut session = Session::new();
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    session.execute("line", &json!({"points": [[5.0, -5.0], [5.0, 5.0]]})).unwrap();
    let target = session.doc().unwrap().model.iter().next().expect("the line").handle;
    let edge = session.doc().unwrap().model.iter().nth(1).expect("the edge").handle;
    let before = session.doc().unwrap().clone();
    run(&mut session, &format!("trim {} edge {} at 8,0", target.hex(), edge.hex()));
    let after = session.doc().unwrap().clone();
    assert_ne!(&after, &before);
    assert_eq!(session.undo().unwrap().as_deref(), Some("Trim"));
    assert_eq!(session.doc().unwrap(), &before, "undo of the trim restores the whole line");
    assert_eq!(session.redo().unwrap().as_deref(), Some("Trim"));
    assert_eq!(session.doc().unwrap(), &after, "redo trims it again");

    // Array: all copies are one step.
    let mut session = Session::new();
    session.execute("circle", &json!({"center": [0.0, 0.0], "radius": 1.0})).unwrap();
    let circle = session.doc().unwrap().model.iter().next().expect("the circle").handle;
    let before = session.doc().unwrap().clone();
    run(&mut session, &format!("array {} rows 2 cols 3 step 20,10", circle.hex()));
    let after = session.doc().unwrap().clone();
    assert_eq!(session.doc().unwrap().entity_count(), 6);
    assert_ne!(&after, &before);
    assert_eq!(session.undo().unwrap().as_deref(), Some("Array"));
    assert_eq!(session.doc().unwrap(), &before, "undo of the array removes every copy");
    assert_eq!(session.redo().unwrap().as_deref(), Some("Array"));
    assert_eq!(session.doc().unwrap(), &after, "redo makes every copy again");
}

/// Run a one-line command the way [`submit`] does, but against one session.
fn run(session: &mut Session, text: &str) {
    let op = {
        let state = session.state().unwrap();
        cmdline::op_for(text, &state.doc, &state.space, "local-0", 0).unwrap()
    };
    session.execute(&op.command, &op.params).unwrap();
}

#[test]
fn a_remote_rect_copy_and_rotate_keep_the_receivers_cursor_and_selection() {
    let mut server = Session::new();
    let mut receiver = Session::new();
    let empty = receiver.doc().unwrap().clone();
    server.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    server.execute("circle", &json!({"center": [20.0, 20.0], "radius": 2.0})).unwrap();
    let created = diff(&empty, server.doc().unwrap(), &Space::Model).unwrap();
    receiver.apply_remote(&created).unwrap();
    let line = receiver.doc().unwrap().model.iter().next().expect("the line arrived").handle;
    let circle = receiver.doc().unwrap().model.iter().nth(1).expect("the circle arrived").handle;
    receiver.set_selection(vec![line, circle]);
    receiver.cursor = cadcraft_geom::Vec2::new(3.0, 4.0);

    // Another participant draws a rectangle, copies the line and rotates the circle.
    let before = server.doc().unwrap().clone();
    server.execute("rectang", &json!({"p1": [30.0, 0.0], "p2": [40.0, 5.0]})).unwrap();
    server.execute("copy", &json!({"handles": [line.hex()], "delta": [0.0, 30.0]})).unwrap();
    server.execute("rotate", &json!({"handles": [circle.hex()], "base": [0.0, 0.0], "angle": 90.0})).unwrap();
    let remote = diff(&before, server.doc().unwrap(), &Space::Model).unwrap();
    assert!(
        remote.iter().any(|change| matches!(change, cadcraft_collab::Change::EntityCreate { .. })),
        "the rectangle and the copy arrive as creations"
    );
    assert!(
        remote.iter().any(|change| matches!(change, cadcraft_collab::Change::EntityReplace { handle, .. } if *handle == circle)),
        "the rotated circle arrives as a replacement"
    );

    receiver.apply_remote(&remote).unwrap();

    assert_eq!(receiver.selection(), vec![line, circle], "a surviving entity keeps its place in the selection");
    assert_eq!(receiver.cursor, cadcraft_geom::Vec2::new(3.0, 4.0), "the cursor does not follow a remote change");
    assert_eq!(receiver.doc().unwrap(), server.doc().unwrap());
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

#[test]
fn a_remote_ellipse_mirror_and_scale_keep_the_receivers_cursor_and_selection() {
    let mut server = Session::new();
    let mut receiver = Session::new();
    let empty = receiver.doc().unwrap().clone();
    server.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    server.execute("circle", &json!({"center": [20.0, 20.0], "radius": 2.0})).unwrap();
    let created = diff(&empty, server.doc().unwrap(), &Space::Model).unwrap();
    receiver.apply_remote(&created).unwrap();
    let line = receiver.doc().unwrap().model.iter().next().expect("the line arrived").handle;
    let circle = receiver.doc().unwrap().model.iter().nth(1).expect("the circle arrived").handle;
    receiver.set_selection(vec![line, circle]);
    receiver.cursor = cadcraft_geom::Vec2::new(3.0, 4.0);

    // Another participant draws an ellipse, mirrors the line and scales the circle.
    let before = server.doc().unwrap().clone();
    server.execute("ellipse", &json!({"center": [30.0, 30.0], "major": [4.0, 0.0], "ratio": 0.5})).unwrap();
    server.execute("mirror", &json!({"handles": [line.hex()], "p1": [0.0, 0.0], "p2": [0.0, 10.0]})).unwrap();
    server.execute("scale", &json!({"handles": [circle.hex()], "base": [0.0, 0.0], "factor": 2.0})).unwrap();
    let remote = diff(&before, server.doc().unwrap(), &Space::Model).unwrap();
    assert!(
        remote.iter().any(|change| matches!(change, cadcraft_collab::Change::EntityCreate { .. })),
        "the ellipse and the mirrored copy arrive as creations"
    );
    assert!(
        remote.iter().any(|change| matches!(change, cadcraft_collab::Change::EntityReplace { handle, .. } if *handle == circle)),
        "the scaled circle arrives as a replacement"
    );

    receiver.apply_remote(&remote).unwrap();

    assert_eq!(receiver.selection(), vec![line, circle], "an entity that survives the change stays selected");
    assert_eq!(receiver.cursor, cadcraft_geom::Vec2::new(3.0, 4.0), "the cursor does not follow a remote change");
    assert_eq!(receiver.doc().unwrap(), server.doc().unwrap());
}

#[test]
fn a_remote_offset_trim_and_array_keep_the_receivers_cursor_and_selection() {
    let mut server = Session::new();
    let mut receiver = Session::new();
    let empty = receiver.doc().unwrap().clone();
    server.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    server.execute("circle", &json!({"center": [20.0, 20.0], "radius": 2.0})).unwrap();
    server.execute("line", &json!({"points": [[5.0, -10.0], [5.0, 10.0]]})).unwrap();
    let created = diff(&empty, server.doc().unwrap(), &Space::Model).unwrap();
    receiver.apply_remote(&created).unwrap();
    let line = receiver.doc().unwrap().model.iter().next().expect("the line arrived").handle;
    let circle = receiver.doc().unwrap().model.iter().nth(1).expect("the circle arrived").handle;
    let edge = receiver.doc().unwrap().model.iter().nth(2).expect("the edge arrived").handle;
    receiver.set_selection(vec![line, circle]);
    receiver.cursor = cadcraft_geom::Vec2::new(3.0, 4.0);

    // Another participant offsets the line, trims it at the edge and arrays the circle.
    let before = server.doc().unwrap().clone();
    server.execute("offset", &json!({"handle": line.hex(), "distance": 2.0, "side": [0.0, 1.0], "layer": "current"})).unwrap();
    server.execute("trim", &json!({"handle": line.hex(), "edges": [edge.hex()], "pick": [8.0, 0.0]})).unwrap();
    server.execute(
        "array",
        &json!({"handles": [circle.hex()], "type": "rectangular", "rows": 2, "cols": 3, "rowSpacing": 10.0, "colSpacing": 20.0, "associative": false}),
    )
    .unwrap();
    let remote = diff(&before, server.doc().unwrap(), &Space::Model).unwrap();
    assert!(
        remote.iter().any(|change| matches!(change, cadcraft_collab::Change::EntityCreate { .. })),
        "the offset and the array arrive as creations"
    );
    assert!(
        remote.iter().any(|change| matches!(change, cadcraft_collab::Change::EntityReplace { handle, .. } if *handle == line)),
        "the trimmed line arrives as a replacement"
    );

    receiver.apply_remote(&remote).unwrap();

    assert_eq!(receiver.selection(), vec![line, circle], "an entity that survives the change stays selected");
    assert_eq!(receiver.cursor, cadcraft_geom::Vec2::new(3.0, 4.0), "the cursor does not follow a remote change");
    assert_eq!(receiver.doc().unwrap(), server.doc().unwrap());
}

#[test]
fn offset_and_array_put_their_objects_on_the_authors_layer() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();
    add_layers(&mut server, &mut [&mut alice, &mut bob], &["WALLS", "DETAILS"]);
    alice.execute("layer.current", &json!({ "name": "WALLS" })).unwrap();
    bob.execute("layer.current", &json!({ "name": "DETAILS" })).unwrap();

    // Alice draws on WALLS; Bob offsets the line and arrays it, so his copies are on DETAILS.
    submit(&mut server, &mut [&mut alice, &mut bob], 0, "line 0,0 10,0");
    let line = bob.doc().unwrap().model.iter().next().expect("the line arrived").handle;
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("offset {} distance 2 side 0,1", line.hex()));
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("array {} rows 2 cols 2 step 20,20", line.hex()));

    assert_eq!(alice.doc().unwrap().space(&Space::Model), bob.doc().unwrap().space(&Space::Model), "the participants agree on the objects");
    assert_eq!(alice.doc().unwrap().space(&Space::Model), server.doc().unwrap().space(&Space::Model), "and agree with the authoritative drawing");
    let layers: Vec<&str> = alice.doc().unwrap().model.iter().map(|entity| entity.common.layer.as_str()).collect();
    assert_eq!(layers.iter().filter(|layer| **layer == "WALLS").count(), 1, "the source line stays on its author's layer: {layers:?}");
    assert_eq!(layers.iter().filter(|layer| **layer == "DETAILS").count(), 4, "the offset and three array copies are on Bob's: {layers:?}");
    assert_eq!(alice.doc().unwrap().header.str("CLAYER", "?"), "WALLS", "nobody's current layer follows somebody else's drawing");
    assert_eq!(bob.doc().unwrap().header.str("CLAYER", "?"), "DETAILS");
    assert_eq!(server.doc().unwrap().header.str("CLAYER", "?"), "0", "the server keeps its own current layer");
}

/// Add `names` to the authoritative drawing and copy them to every replica the way a table
/// operation reaches them: as table changes of one change set.
fn add_layers(server: &mut Session, replicas: &mut [&mut Session], names: &[&str]) {
    let before = server.doc().unwrap().clone();
    for name in names {
        server.execute("layer.new", &json!({ "name": name })).unwrap();
    }
    let mut registry = cadcraft_collab::TableRegistry::new();
    let changes = cadcraft_collab::diff_all(&before, server.doc().unwrap(), &mut registry, &Default::default()).unwrap();
    assert!(!changes.is_empty(), "the layers have to reach the replicas");
    for replica in replicas.iter_mut() {
        replica.apply_remote(&changes).unwrap();
    }
}

#[test]
fn two_replicas_on_different_layers_draw_into_the_authors_layer() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();
    add_layers(&mut server, &mut [&mut alice, &mut bob], &["WALLS", "DETAILS"]);
    // Each participant works on their own layer; neither choice is an operation.
    alice.execute("layer.current", &json!({ "name": "WALLS" })).unwrap();
    bob.execute("layer.current", &json!({ "name": "DETAILS" })).unwrap();

    submit(&mut server, &mut [&mut alice, &mut bob], 0, "line 0,0 10,0");
    submit(&mut server, &mut [&mut alice, &mut bob], 1, "polyline 20,0 30,0 30,10");
    // The ellipse is drawn by Alice too; the mirrored copy belongs to the layer of its source.
    submit(&mut server, &mut [&mut alice, &mut bob], 0, "ellipse 40,0 3,2");
    let polyline = bob.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::LwPolyline(_))).expect("the polyline arrived").handle;
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("mirror {} 0,0 1,0", polyline.hex()));

    assert_eq!(alice.doc().unwrap().space(&Space::Model), bob.doc().unwrap().space(&Space::Model), "the participants agree on the objects");
    assert_eq!(alice.doc().unwrap().space(&Space::Model), server.doc().unwrap().space(&Space::Model), "and agree with the authoritative drawing");
    assert_eq!(
        alice.doc().unwrap().layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
        bob.doc().unwrap().layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
        "and on the layers"
    );

    let layers: Vec<String> = alice.doc().unwrap().model.iter().map(|entity| entity.common.layer.clone()).collect();
    assert_eq!(layers, vec!["WALLS", "DETAILS", "WALLS", "DETAILS"], "each object is on its author's layer; a mirror keeps its source's");
    // Nobody's current layer followed somebody else's drawing.
    assert_eq!(alice.doc().unwrap().header.str("CLAYER", "?"), "WALLS");
    assert_eq!(bob.doc().unwrap().header.str("CLAYER", "?"), "DETAILS");
    assert_eq!(server.doc().unwrap().header.str("CLAYER", "?"), "0", "the server keeps its own current layer");
}

#[test]
fn a_layer_only_the_author_has_is_refused_before_the_document_changes() {
    let mut server = Session::new();
    let mut alice = Session::new();
    // The author made a layer that never reached the session, and draws on it.
    alice.execute("layer.new", &json!({ "name": "LOCAL" })).unwrap();
    alice.execute("layer.current", &json!({ "name": "LOCAL" })).unwrap();
    let before = server.doc().unwrap().clone();
    let op = {
        let state = alice.state().unwrap();
        // Three points: had the command run, the first segment would already be there.
        cmdline::op_for("line 0,0 10,0 10,5", &state.doc, &state.space, "local-0", 0).unwrap()
    };
    assert_eq!(op.context.clay.as_deref(), Some("LOCAL"), "the operation names the layer it would draw on");

    let error = server.execute_intent(&op.command, &op.params, &op.context).unwrap_err();

    assert!(error.to_string().contains("there is no layer `LOCAL`"), "{error}");
    assert_eq!(server.doc().unwrap(), &before, "nothing was created, not even the first segment");
    assert_eq!(server.doc().unwrap().entity_count(), 0);
}
