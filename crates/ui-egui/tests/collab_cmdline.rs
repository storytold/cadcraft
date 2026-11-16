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
///
/// The set is [`cadcraft_collab::diff_all`], as the room builds it: a block definition is a change
/// of its own, and its objects live in the block, not in a space's delta.
fn submit(server: &mut Session, replicas: &mut [&mut Session], author: usize, text: &str) -> usize {
    let op = {
        let state = replicas[author].state().unwrap();
        cmdline::op_for(text, &state.doc, &state.space, &format!("local-{author}"), 0).unwrap()
    };
    let before = server.doc().unwrap().clone();
    server.execute_intent(&op.command, &op.params, &op.context).unwrap();
    let after = server.doc().unwrap();
    let mut registry = cadcraft_collab::TableRegistry::new();
    let changes = cadcraft_collab::diff_all(&before, after, &mut registry, &Default::default()).unwrap();
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
        error.to_string().contains(
            "line, circle, arc, polyline, ellipse, rect, text, dimlinear, dimaligned, mleader, spline, block, insert, move, copy, rotate, mirror, scale, offset, trim, array, hatch, wipeout, imageattach or erase"
        ),
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
fn a_refused_text_or_hatch_does_not_change_the_drawing() {
    let mut session = Session::new();
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    session.execute("rectang", &json!({"p1": [0.0, 0.0], "p2": [10.0, 5.0]})).unwrap();
    let state = session.state().unwrap();
    let before = state.doc.clone();
    let before_revision = state.revision;
    let line = state.doc.model.iter().next().expect("the line").handle.hex();
    let rect = state.doc.model.iter().nth(1).expect("the rectangle").handle.hex();

    // An empty or unquoted text, a height that is not positive, an open or unknown boundary, a
    // zero scale and an unknown pattern: none of them becomes an operation.
    let refused = [
        "text 0,0 1 0 \"\"".to_string(),
        "text 0,0 1 0 unquoted".to_string(),
        "text 0,0 0 0 \"note\"".to_string(),
        "text 0,0 1 0 \"unterminated".to_string(),
        format!("hatch {line} pattern ANSI31 scale 1 angle 0"),
        format!("hatch {rect} pattern ANSI31 scale 0 angle 0"),
        format!("hatch {rect} pattern NOPE scale 1 angle 0"),
        "hatch FF pattern ANSI31 scale 1 angle 0".to_string(),
    ];
    for text in &refused {
        let error = cmdline::op_for(text, &state.doc, &state.space, "local-0", 0).unwrap_err();
        assert!(!error.to_string().is_empty(), "{text}");
        assert_eq!(session.doc().unwrap(), &*before, "`{text}` never becomes an operation");
        assert_eq!(session.state().unwrap().revision, before_revision, "`{text}` does not redraw");
    }
    // A refused line leaves no undo step either: the last real step is still the rectangle.
    assert_eq!(session.undo().unwrap().as_deref(), Some("Rectangle"), "no refused line pushed an undo step");
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
fn two_replicas_that_draw_text_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    submit(&mut server, &mut [&mut alice, &mut bob], 0, "text 10,20 2.5 30 \"Shared note\"");

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the text");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap(), "and agree with the authoritative drawing");
    assert_eq!(alice.doc().unwrap().entity_count(), 1, "a text is one object");
    let text = alice.doc().unwrap().model.iter().next().expect("the text arrived");
    let EntityKind::Text(t) = &text.kind else { panic!("expected a text, got {:?}", text.kind) };
    assert_eq!(t.value, "Shared note");
    assert!((t.insert.x - 10.0).abs() < 1e-12 && (t.insert.y - 20.0).abs() < 1e-12, "the insertion point, got {:?}", t.insert);
    assert!((t.height - 2.5).abs() < 1e-12, "the height, got {}", t.height);
    assert!((t.rotation - 30f64.to_radians()).abs() < 1e-12, "the angle in radians, got {}", t.rotation);
}

#[test]
fn two_replicas_that_hatch_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    submit(&mut server, &mut [&mut alice, &mut bob], 0, "rect 0,0 10,5");
    // Bob hatches Alice's rectangle by the handle the change set gave him.
    let rect = bob.doc().unwrap().model.iter().next().expect("the rectangle arrived").handle;
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("hatch {} pattern ANSI31 scale 2 angle 45", rect.hex()));

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the hatch");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap(), "and agree with the authoritative drawing");
    assert_eq!(alice.doc().unwrap().entity_count(), 2, "the boundary stays and the hatch is a second object");
    let hatch = alice.doc().unwrap().model.iter().find(|entity| matches!(entity.kind, EntityKind::Hatch(_))).expect("the hatch arrived");
    let EntityKind::Hatch(h) = &hatch.kind else { panic!("expected a hatch") };
    assert_eq!(h.pattern, "ANSI31");
    assert!((h.scale - 2.0).abs() < 1e-12, "the pattern scale, got {}", h.scale);
    assert!((h.angle - 45f64.to_radians()).abs() < 1e-12, "the angle in radians, got {}", h.angle);
    assert!(!h.associative, "a line's hatch is a static fill, not an association");
    assert_eq!(h.loops.len(), 1, "one closed boundary makes one loop");
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

#[test]
fn undo_and_redo_of_text_and_hatch_return_the_state_before_and_after() {
    let mut session = Session::new();
    session.execute("rectang", &json!({"p1": [0.0, 0.0], "p2": [10.0, 5.0]})).unwrap();
    let rect = session.doc().unwrap().model.iter().next().expect("the rectangle").handle;

    // The hatch is one step; undo takes the fill away, redo puts it back.
    let before = session.doc().unwrap().clone();
    run(&mut session, &format!("hatch {} pattern ANSI31 scale 2 angle 45", rect.hex()));
    let after = session.doc().unwrap().clone();
    assert_eq!(session.doc().unwrap().entity_count(), 2);
    assert_ne!(&after, &before);
    assert_eq!(session.undo().unwrap().as_deref(), Some("Hatch..."));
    assert_eq!(session.doc().unwrap(), &before, "undo of the hatch removes the fill");
    assert_eq!(session.redo().unwrap().as_deref(), Some("Hatch..."));
    assert_eq!(session.doc().unwrap(), &after, "redo fills it again");

    // So is the text.
    let before = session.doc().unwrap().clone();
    run(&mut session, "text 20,10 1 0 \"Note\"");
    let after = session.doc().unwrap().clone();
    assert_ne!(&after, &before);
    assert_eq!(session.undo().unwrap().as_deref(), Some("Single Line Text"));
    assert_eq!(session.doc().unwrap(), &before, "undo of the text removes it");
    assert_eq!(session.redo().unwrap().as_deref(), Some("Single Line Text"));
    assert_eq!(session.doc().unwrap(), &after, "redo writes it again");
}

#[test]
fn two_replicas_that_dimension_and_spline_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    submit(&mut server, &mut [&mut alice, &mut bob], 0, "circle 0,0 1");
    submit(&mut server, &mut [&mut alice, &mut bob], 0, "circle 20,0 1");
    // Bob dimensions the distance between the two circles' centres, by the handles he got.
    let handles: Vec<String> = bob.doc().unwrap().model.iter().map(|entity| entity.handle.hex()).collect();
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("dimlinear {},{} offset 5", handles[0], handles[1]));
    // Alice draws a spline through three fit points, and closes it.
    submit(&mut server, &mut [&mut alice, &mut bob], 0, "spline 0,10 10,15 20,10 closed");

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the dimension and the spline");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap(), "and agree with the authoritative drawing");
    assert_eq!(alice.doc().unwrap().entity_count(), 4, "two circles, one dimension and one spline");
    let dimension = alice.doc().unwrap().model.iter().find(|entity| matches!(entity.kind, EntityKind::Dimension(_))).expect("the dimension arrived");
    let EntityKind::Dimension(d) = &dimension.kind else { panic!("expected a dimension") };
    assert!((d.measurement - 20.0).abs() < 1e-9, "the measured distance, got {}", d.measurement);
    assert!((d.p13.x - 0.0).abs() < 1e-9 && (d.p14.x - 20.0).abs() < 1e-9, "the two measured points, got {:?} and {:?}", d.p13, d.p14);
    let spline = alice.doc().unwrap().model.iter().find(|entity| matches!(entity.kind, EntityKind::Spline(_))).expect("the spline arrived");
    let EntityKind::Spline(s) = &spline.kind else { panic!("expected a spline") };
    assert_eq!(s.fit.len(), 3, "three fit points: {:?}", s.fit);
    assert!(s.closed, "the `closed` keyword closed the curve");
}

#[test]
fn a_refused_dimlinear_or_spline_does_not_change_the_drawing() {
    let mut session = Session::new();
    session.execute("circle", &json!({"center": [0.0, 0.0], "radius": 1.0})).unwrap();
    session.execute("circle", &json!({"center": [20.0, 0.0], "radius": 1.0})).unwrap();
    // A real step after the circles, so a refused line that pushed an undo step would show.
    session.execute("ellipse", &json!({"center": [40.0, 40.0], "major": [2.0, 0.0], "ratio": 0.5})).unwrap();
    let state = session.state().unwrap();
    let before = state.doc.clone();
    let before_revision = state.revision;
    let handles: Vec<String> = state.doc.model.iter().take(2).map(|entity| entity.handle.hex()).collect();
    let points = |n: usize| (0..n).map(|i| format!("{i},{}", i % 7)).collect::<Vec<_>>().join(" ");

    // One handle, the same handle twice, unknown handles, a non-numeric or out-of-range
    // placement; a spline of two points, of sixty-five, of the same point and one outside the
    // drawing's coordinate limit: none of them becomes an operation.
    let refused = [
        format!("dimlinear {} offset 5", handles[0]),
        format!("dimlinear {},{} offset 5", handles[0], handles[0]),
        "dimlinear FF,1FE offset 5".to_string(),
        format!("dimlinear {},{} offset later", handles[0], handles[1]),
        format!("dimlinear {},{} offset 1e13", handles[0], handles[1]),
        format!("dimlinear {},{}", handles[0], handles[1]),
        "spline 0,0 1,1".to_string(),
        format!("spline {}", points(65)),
        "spline 0,0 1,1 later".to_string(),
        "spline 0,0 1,1 1e13,0".to_string(),
        "spline 0,0 0,0 0,0".to_string(),
    ];
    for text in &refused {
        let error = cmdline::op_for(text, &state.doc, &state.space, "local-0", 0).unwrap_err();
        assert!(!error.to_string().is_empty(), "{text}");
        assert_eq!(session.doc().unwrap(), &*before, "`{text}` never becomes an operation");
        assert_eq!(session.state().unwrap().revision, before_revision, "`{text}` does not redraw");
    }
    // A refused line leaves no undo step either: the last real step is still the ellipse.
    assert_eq!(session.undo().unwrap().as_deref(), Some("Ellipse"), "no refused line pushed an undo step");
}

#[test]
fn undo_and_redo_of_dimlinear_and_spline_return_the_state_before_and_after() {
    let mut session = Session::new();
    session.execute("circle", &json!({"center": [0.0, 0.0], "radius": 1.0})).unwrap();
    session.execute("circle", &json!({"center": [20.0, 0.0], "radius": 1.0})).unwrap();
    let handles: Vec<String> = session.doc().unwrap().model.iter().map(|entity| entity.handle.hex()).collect();

    // The dimension is one step; undo takes it away, redo puts it back.
    let before = session.doc().unwrap().clone();
    run(&mut session, &format!("dimlinear {},{} offset 5", handles[0], handles[1]));
    let after = session.doc().unwrap().clone();
    assert_eq!(session.doc().unwrap().entity_count(), 3);
    assert_ne!(&after, &before);
    assert_eq!(session.undo().unwrap().as_deref(), Some("Linear"));
    assert_eq!(session.doc().unwrap(), &before, "undo of the dimension removes it");
    assert_eq!(session.redo().unwrap().as_deref(), Some("Linear"));
    assert_eq!(session.doc().unwrap(), &after, "redo puts the dimension back");

    // So is the spline, closed or not.
    let before = session.doc().unwrap().clone();
    run(&mut session, "spline 0,10 10,15 20,10 closed");
    let after = session.doc().unwrap().clone();
    assert_eq!(session.doc().unwrap().entity_count(), 4);
    assert_ne!(&after, &before);
    assert_eq!(session.undo().unwrap().as_deref(), Some("Spline"));
    assert_eq!(session.doc().unwrap(), &before, "undo of the spline removes it");
    assert_eq!(session.redo().unwrap().as_deref(), Some("Spline"));
    assert_eq!(session.doc().unwrap(), &after, "redo draws it again");
}

#[test]
fn a_remote_dimension_and_spline_keep_the_receivers_cursor_and_selection() {
    let mut server = Session::new();
    let mut receiver = Session::new();
    let empty = receiver.doc().unwrap().clone();
    server.execute("circle", &json!({"center": [0.0, 0.0], "radius": 1.0})).unwrap();
    server.execute("circle", &json!({"center": [20.0, 0.0], "radius": 1.0})).unwrap();
    let created = diff(&empty, server.doc().unwrap(), &Space::Model).unwrap();
    receiver.apply_remote(&created).unwrap();
    let first = receiver.doc().unwrap().model.iter().next().expect("the first circle arrived").handle;
    let second = receiver.doc().unwrap().model.iter().nth(1).expect("the second circle arrived").handle;
    receiver.set_selection(vec![first, second]);
    receiver.cursor = cadcraft_geom::Vec2::new(3.0, 4.0);

    // Another participant adds a dimension between the circles and a closed spline.
    let before = server.doc().unwrap().clone();
    server.execute("dimlinear", &json!({"p1": [0.0, 0.0], "p2": [20.0, 0.0], "at": [10.0, 5.0]})).unwrap();
    server.execute("spline", &json!({"fit": [[0.0, 10.0], [10.0, 15.0], [20.0, 10.0]], "closed": true})).unwrap();
    let remote = diff(&before, server.doc().unwrap(), &Space::Model).unwrap();
    assert!(
        remote.iter().filter(|change| matches!(change, cadcraft_collab::Change::EntityCreate { .. })).count() >= 2,
        "the dimension and the spline arrive as creations"
    );

    receiver.apply_remote(&remote).unwrap();

    assert_eq!(receiver.selection(), vec![first, second], "an entity that survives the change stays selected");
    assert_eq!(receiver.cursor, cadcraft_geom::Vec2::new(3.0, 4.0), "the cursor does not follow a remote change");
    assert_eq!(receiver.doc().unwrap(), server.doc().unwrap());
}

#[test]
fn two_replicas_that_lead_and_block_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    submit(&mut server, &mut [&mut alice, &mut bob], 0, "circle 0,0 1");
    // Bob points a multileader at Alice's circle, by the handle the change set gave him.
    let circle = bob.doc().unwrap().model.iter().next().expect("the circle arrived").handle;
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("mleader {} 20,20 \"Shared note\"", circle.hex()));

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the multileader");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap(), "and agree with the authoritative drawing");
    assert_eq!(alice.doc().unwrap().entity_count(), 2, "the circle and the leader");
    let leader = alice.doc().unwrap().model.iter().find(|entity| matches!(entity.kind, EntityKind::MLeader(_))).expect("the leader arrived");
    let EntityKind::MLeader(m) = &leader.kind else { panic!("expected a multileader") };
    assert_eq!(m.leaders.len(), 1, "one leader line");
    assert_eq!(m.leaders[0].len(), 1, "one arrowhead, at the circle's point");
    assert!((m.landing.x - 20.0).abs() < 1e-9 && (m.landing.y - 20.0).abs() < 1e-9, "the landing, got {:?}", m.landing);
    assert_eq!(m.text.as_ref().map(|text| text.contents.as_str()), Some("Shared note"));
}

#[test]
fn two_replicas_that_make_and_insert_a_block_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    submit(&mut server, &mut [&mut alice, &mut bob], 0, "line 0,0 10,0");
    let line = bob.doc().unwrap().model.iter().next().expect("the line arrived").handle;
    // Alice turns the line into a block reference at the base point.
    submit(&mut server, &mut [&mut alice, &mut bob], 0, &format!("block DOOR base 0,0 {}", line.hex()));

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the block");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap(), "and agree with the authoritative drawing");
    let block = alice.doc().unwrap().block("DOOR").expect("the definition reached the replica");
    assert_eq!(block.entities.len(), 1, "the block carries the line");
    assert!(alice.doc().unwrap().entity(line).is_none(), "BLOCK's Convert default replaced the line");
    let first = alice.doc().unwrap().model.iter().find_map(|entity| match &entity.kind {
        EntityKind::Insert(insert) if insert.block.eq_ignore_ascii_case("DOOR") => Some(insert),
        _ => None,
    });
    assert!(first.is_some(), "the converted objects are one reference to the new block");

    // Bob inserts the block elsewhere, scaled and turned; both replicas get the definition again
    // (it is unchanged) and the new reference.
    submit(&mut server, &mut [&mut alice, &mut bob], 1, "insert DOOR at 50,0 scale 2 angle 90");

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the insertion");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap());
    assert_eq!(alice.doc().unwrap().entity_count(), 2, "the converted reference and the inserted one");
    let inserted = alice
        .doc()
        .unwrap()
        .model
        .iter()
        .find_map(|entity| match &entity.kind {
            EntityKind::Insert(insert) if (insert.insert.x - 50.0).abs() < 1e-9 => Some(insert),
            _ => None,
        })
        .expect("the inserted reference arrived");
    assert!((inserted.scale.x - 2.0).abs() < 1e-9, "the scale, got {:?}", inserted.scale);
    assert!((inserted.rotation - 90f64.to_radians()).abs() < 1e-12, "the angle in radians, got {}", inserted.rotation);
}

#[test]
fn a_refused_mleader_block_or_insert_does_not_change_the_drawing() {
    let mut session = Session::new();
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    // A real step after the line, so a refused line that pushed an undo step would show.
    session.execute("ellipse", &json!({"center": [40.0, 40.0], "major": [2.0, 0.0], "ratio": 0.5})).unwrap();
    let state = session.state().unwrap();
    let before = state.doc.clone();
    let before_revision = state.revision;
    let line = state.doc.model.iter().next().expect("the line").handle.hex();

    let refused = [
        // A leader without an address, with empty text, or on an object with no point.
        "mleader 20,10 \"note\"".to_string(),
        format!("mleader {line} 20,10 \"\""),
        "mleader FF 20,10 \"note\"".to_string(),
        format!("mleader {line} 20,10 \"unterminated"),
        // A block with an empty name, an unknown handle or no base point.
        format!("block \"\" base 0,0 {line}"),
        "block X base 0,0 FF".to_string(),
        "block X base 0,0".to_string(),
        // An insert of an unknown block, a zero scale or a malformed point.
        "insert NOPE at 0,0".to_string(),
        "insert NOPE at 0,0 scale 0".to_string(),
        "insert NOPE at nowhere".to_string(),
    ];
    for text in &refused {
        let error = cmdline::op_for(text, &state.doc, &state.space, "local-0", 0).unwrap_err();
        assert!(!error.to_string().is_empty(), "{text}");
        assert_eq!(session.doc().unwrap(), &*before, "`{text}` never becomes an operation");
        assert_eq!(session.state().unwrap().revision, before_revision, "`{text}` does not redraw");
    }
    // A refused line leaves no undo step either: the last real step is still the ellipse.
    assert_eq!(session.undo().unwrap().as_deref(), Some("Ellipse"), "no refused line pushed an undo step");
}

#[test]
fn undo_and_redo_of_mleader_block_and_insert_return_the_state_before_and_after() {
    let mut session = Session::new();
    session.execute("circle", &json!({"center": [0.0, 0.0], "radius": 1.0})).unwrap();
    let circle = session.doc().unwrap().model.iter().next().expect("the circle").handle;

    // The multileader is one step; undo takes it away, redo puts it back.
    let before = session.doc().unwrap().clone();
    run(&mut session, &format!("mleader {} 20,20 \"note\"", circle.hex()));
    let after = session.doc().unwrap().clone();
    assert_eq!(session.doc().unwrap().entity_count(), 2);
    assert_ne!(&after, &before);
    assert_eq!(session.undo().unwrap().as_deref(), Some("Multileader"));
    assert_eq!(session.doc().unwrap(), &before, "undo of the leader removes it");
    assert_eq!(session.redo().unwrap().as_deref(), Some("Multileader"));
    assert_eq!(session.doc().unwrap(), &after, "redo points it again");

    // Defining the block and converting the circle are one step: undo brings the circle and the
    // block definition back, redo makes the reference again.
    let before = session.doc().unwrap().clone();
    run(&mut session, &format!("block PART base 0,0 {}", circle.hex()));
    let after = session.doc().unwrap().clone();
    assert!(session.doc().unwrap().block("PART").is_some(), "the definition exists");
    assert!(session.doc().unwrap().entity(circle).is_none(), "the circle became a reference");
    assert_eq!(session.undo().unwrap().as_deref(), Some("Make..."));
    assert_eq!(session.doc().unwrap(), &before, "undo removes the block and restores the circle");
    assert!(session.doc().unwrap().block("PART").is_none());
    assert_eq!(session.redo().unwrap().as_deref(), Some("Make..."));
    assert_eq!(session.doc().unwrap(), &after, "redo makes the block and its reference again");

    // The insertion is a step of its own.
    let before = session.doc().unwrap().clone();
    run(&mut session, "insert PART at 50,0 scale 2 angle 45");
    let after = session.doc().unwrap().clone();
    assert_eq!(session.doc().unwrap().entity_count(), before.entity_count() + 1);
    assert_ne!(&after, &before);
    assert_eq!(session.undo().unwrap().as_deref(), Some("Block..."));
    assert_eq!(session.doc().unwrap(), &before, "undo takes the inserted reference away");
    assert_eq!(session.redo().unwrap().as_deref(), Some("Block..."));
    assert_eq!(session.doc().unwrap(), &after, "redo inserts it again");
}

#[test]
fn a_remote_mleader_block_and_insert_keep_the_receivers_cursor_and_selection() {
    let mut server = Session::new();
    let mut receiver = Session::new();
    let empty = receiver.doc().unwrap().clone();
    server.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    server.execute("circle", &json!({"center": [20.0, 20.0], "radius": 2.0})).unwrap();
    let mut registry = cadcraft_collab::TableRegistry::new();
    let created = cadcraft_collab::diff_all(&empty, server.doc().unwrap(), &mut registry, &Default::default()).unwrap();
    receiver.apply_remote(&created).unwrap();
    let line = receiver.doc().unwrap().model.iter().next().expect("the line arrived").handle;
    let circle = receiver.doc().unwrap().model.iter().nth(1).expect("the circle arrived").handle;
    receiver.set_selection(vec![line, circle]);
    receiver.cursor = cadcraft_geom::Vec2::new(3.0, 4.0);

    // Another participant makes a block from the line, points a leader at the circle and inserts
    // the block. The line is converted, so it leaves the receiver's selection; the circle stays.
    let before = server.doc().unwrap().clone();
    server.execute("block", &json!({"name": "PART", "base": [0.0, 0.0], "handles": [line.hex()], "keep": "convert"})).unwrap();
    server.execute("mleader", &json!({"points": [[20.0, 20.0], [40.0, 30.0]], "text": "note"})).unwrap();
    server.execute("insert", &json!({"name": "PART", "at": [50.0, 0.0], "scale": 2.0, "rotation": 90.0})).unwrap();
    let remote = cadcraft_collab::diff_all(&before, server.doc().unwrap(), &mut registry, &Default::default()).unwrap();
    assert!(remote.iter().any(|change| matches!(change, cadcraft_collab::Change::BlockCreate { .. })), "the definition travels as its own change");
    assert!(
        remote.iter().any(|change| matches!(change, cadcraft_collab::Change::EntityCreate { .. })),
        "the leader and the inserted reference arrive as creations"
    );

    receiver.apply_remote(&remote).unwrap();

    assert_eq!(receiver.selection(), vec![circle], "a surviving entity stays selected; the converted line does not");
    assert_eq!(receiver.cursor, cadcraft_geom::Vec2::new(3.0, 4.0), "the cursor does not follow a remote change");
    assert_eq!(receiver.doc().unwrap(), server.doc().unwrap());
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
fn a_remote_text_and_hatch_keep_the_receivers_cursor_and_selection() {
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

    // Another participant writes a note and fills the circle; both are new objects.
    let before = server.doc().unwrap().clone();
    server.execute("text", &json!({"at": [30.0, 30.0], "text": "remote", "height": 1.0})).unwrap();
    server
        .execute(
            "hatch",
            &json!({
                "handles": [circle.hex()],
                "pattern": "ANSI31",
                "scale": 1.0,
                "angle": 0.0,
                "associative": false,
                "separate": false,
                "style": "normal"
            }),
        )
        .unwrap();
    let remote = diff(&before, server.doc().unwrap(), &Space::Model).unwrap();
    assert!(
        remote.iter().filter(|change| matches!(change, cadcraft_collab::Change::EntityCreate { .. })).count() >= 2,
        "the text and the hatch arrive as creations"
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

#[test]
fn text_and_hatch_put_their_objects_on_the_authors_layer() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();
    add_layers(&mut server, &mut [&mut alice, &mut bob], &["WALLS", "DETAILS"]);
    alice.execute("layer.current", &json!({ "name": "WALLS" })).unwrap();
    bob.execute("layer.current", &json!({ "name": "DETAILS" })).unwrap();

    // Alice draws the boundary; Bob hatches it and writes his note, so both are on DETAILS.
    submit(&mut server, &mut [&mut alice, &mut bob], 0, "rect 0,0 10,5");
    let rect = bob.doc().unwrap().model.iter().next().expect("the rectangle arrived").handle;
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("hatch {} pattern ANSI31 scale 2 angle 45", rect.hex()));
    submit(&mut server, &mut [&mut alice, &mut bob], 1, "text 12,5 1 0 \"note\"");

    assert_eq!(alice.doc().unwrap().space(&Space::Model), bob.doc().unwrap().space(&Space::Model), "the participants agree on the objects");
    assert_eq!(alice.doc().unwrap().space(&Space::Model), server.doc().unwrap().space(&Space::Model), "and agree with the authoritative drawing");
    let layers: Vec<&str> = alice.doc().unwrap().model.iter().map(|entity| entity.common.layer.as_str()).collect();
    assert_eq!(layers.iter().filter(|layer| **layer == "WALLS").count(), 1, "the boundary stays on Alice's layer: {layers:?}");
    assert_eq!(layers.iter().filter(|layer| **layer == "DETAILS").count(), 2, "Bob's hatch and text are on his: {layers:?}");
    assert_eq!(alice.doc().unwrap().header.str("CLAYER", "?"), "WALLS", "nobody's current layer follows somebody else's drawing");
    assert_eq!(bob.doc().unwrap().header.str("CLAYER", "?"), "DETAILS");
    assert_eq!(server.doc().unwrap().header.str("CLAYER", "?"), "0", "the server keeps its own current layer");
}

#[test]
fn dimlinear_and_spline_put_their_objects_on_the_authors_layer() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();
    add_layers(&mut server, &mut [&mut alice, &mut bob], &["WALLS", "DETAILS"]);
    alice.execute("layer.current", &json!({ "name": "WALLS" })).unwrap();
    bob.execute("layer.current", &json!({ "name": "DETAILS" })).unwrap();

    // Alice draws the objects to measure; Bob dimensions them and draws his own spline.
    submit(&mut server, &mut [&mut alice, &mut bob], 0, "circle 0,0 1");
    submit(&mut server, &mut [&mut alice, &mut bob], 0, "circle 20,0 1");
    let handles: Vec<String> = bob.doc().unwrap().model.iter().map(|entity| entity.handle.hex()).collect();
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("dimlinear {},{} offset 5", handles[0], handles[1]));
    submit(&mut server, &mut [&mut alice, &mut bob], 1, "spline 0,10 10,15 20,10");

    assert_eq!(alice.doc().unwrap().space(&Space::Model), bob.doc().unwrap().space(&Space::Model), "the participants agree on the objects");
    assert_eq!(alice.doc().unwrap().space(&Space::Model), server.doc().unwrap().space(&Space::Model), "and agree with the authoritative drawing");
    let layers: Vec<&str> = alice.doc().unwrap().model.iter().map(|entity| entity.common.layer.as_str()).collect();
    assert_eq!(layers.iter().filter(|layer| **layer == "WALLS").count(), 2, "the measured circles stay on Alice's layer: {layers:?}");
    assert_eq!(layers.iter().filter(|layer| **layer == "DETAILS").count(), 2, "Bob's dimension and spline are on his: {layers:?}");
    assert_eq!(alice.doc().unwrap().header.str("CLAYER", "?"), "WALLS", "nobody's current layer follows somebody else's drawing");
    assert_eq!(bob.doc().unwrap().header.str("CLAYER", "?"), "DETAILS");
    assert_eq!(server.doc().unwrap().header.str("CLAYER", "?"), "0", "the server keeps its own current layer");
}

#[test]
fn mleader_block_and_insert_put_their_objects_on_the_authors_layer() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();
    add_layers(&mut server, &mut [&mut alice, &mut bob], &["WALLS", "DETAILS"]);
    alice.execute("layer.current", &json!({ "name": "WALLS" })).unwrap();
    bob.execute("layer.current", &json!({ "name": "DETAILS" })).unwrap();

    // Alice draws the line on WALLS; Bob points a leader at it, makes it a block and inserts the
    // block — all three objects are his and belong on DETAILS.
    submit(&mut server, &mut [&mut alice, &mut bob], 0, "line 0,0 10,0");
    let line = bob.doc().unwrap().model.iter().next().expect("the line arrived").handle;
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("mleader {} 20,20 \"note\"", line.hex()));
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("block PART base 0,0 {}", line.hex()));
    submit(&mut server, &mut [&mut alice, &mut bob], 1, "insert PART at 50,0");

    assert_eq!(alice.doc().unwrap().space(&Space::Model), bob.doc().unwrap().space(&Space::Model), "the participants agree on the objects");
    assert_eq!(alice.doc().unwrap().space(&Space::Model), server.doc().unwrap().space(&Space::Model), "and agree with the authoritative drawing");
    let layers: Vec<&str> = alice.doc().unwrap().model.iter().map(|entity| entity.common.layer.as_str()).collect();
    assert_eq!(
        layers,
        vec!["DETAILS", "DETAILS", "DETAILS"],
        "the leader, the converted block and the inserted one are on Bob's layer; the block's own objects keep the source's"
    );
    let block = alice.doc().unwrap().block("PART").expect("the definition reached the replica");
    assert_eq!(
        block.entities.iter().map(|entity| entity.common.layer.as_str()).collect::<Vec<_>>(),
        vec!["WALLS"],
        "the block keeps the line's layer"
    );
    assert_eq!(alice.doc().unwrap().header.str("CLAYER", "?"), "WALLS", "nobody's current layer follows somebody else's drawing");
    assert_eq!(bob.doc().unwrap().header.str("CLAYER", "?"), "DETAILS");
    assert_eq!(server.doc().unwrap().header.str("CLAYER", "?"), "0", "the server keeps its own current layer");
}

#[test]
fn two_replicas_that_wipe_out_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    // A closed polyline reaches every replica the way a drawing command's change set does.
    let empty = server.doc().unwrap().clone();
    server.execute("pline", &json!({"vertices": [[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0]], "closed": true})).unwrap();
    let mut registry = cadcraft_collab::TableRegistry::new();
    let created = cadcraft_collab::diff_all(&empty, server.doc().unwrap(), &mut registry, &Default::default()).unwrap();
    alice.apply_remote(&created).unwrap();
    bob.apply_remote(&created).unwrap();
    let polyline = bob.doc().unwrap().model.iter().next().expect("the polyline arrived").handle;

    // Bob masks the polyline by the handle the change set gave him; Alice masks an explicit
    // boundary. The mask is boundary points either way, so both replicas hold the same bytes.
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("wipeout {}", polyline.hex()));
    submit(&mut server, &mut [&mut alice, &mut bob], 0, "wipeout 20,0 30,0 30,10 20,10");

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree on the masks");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap(), "and agree with the authoritative drawing");
    assert_eq!(alice.doc().unwrap().entity_count(), 3, "the polyline and the two masks");
    let boundaries: Vec<Vec<(f64, f64)>> = alice
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|entity| match &entity.kind {
            EntityKind::Wipeout(wipeout) => Some(wipeout.boundary.iter().map(|point| (point.x, point.y)).collect()),
            _ => None,
        })
        .collect();
    assert_eq!(boundaries, vec![vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (0.0, 5.0)], vec![(20.0, 0.0), (30.0, 0.0), (30.0, 10.0), (20.0, 10.0)],]);
}

#[test]
fn a_refused_wipeout_does_not_change_the_drawing() {
    let mut session = Session::new();
    // An open polyline and two real steps after it, so a refused line that pushed an undo step
    // would show.
    session.execute("pline", &json!({"vertices": [[0.0, 0.0], [10.0, 0.0], [10.0, 5.0]]})).unwrap();
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    session.execute("ellipse", &json!({"center": [40.0, 40.0], "major": [2.0, 0.0], "ratio": 0.5})).unwrap();
    let state = session.state().unwrap();
    let before = state.doc.clone();
    let before_revision = state.revision;
    let open = state.doc.model.iter().next().expect("the polyline").handle;
    let line = state.doc.model.iter().nth(1).expect("the line").handle;

    let refused = [
        // Nothing to bound, too few points, a boundary with no area, a non-numeric coordinate.
        "wipeout".to_string(),
        "wipeout 0,60 10,60".to_string(),
        "wipeout 0,60 10,60 20,60".to_string(),
        "wipeout 0,60 10,60 ten,70".to_string(),
        // A point and a handle do not mix; a handle that names nothing or is not a polyline.
        format!("wipeout 0,60 10,60 {}", open.hex()),
        "wipeout FF".to_string(),
        format!("wipeout {}", line.hex()),
        // An open polyline is not a boundary.
        format!("wipeout {}", open.hex()),
    ];
    for text in &refused {
        let error = cmdline::op_for(text, &state.doc, &state.space, "local-0", 0).unwrap_err();
        assert!(!error.to_string().is_empty(), "{text}");
        assert_eq!(session.doc().unwrap(), &*before, "`{text}` never becomes an operation");
        assert_eq!(session.state().unwrap().revision, before_revision, "`{text}` does not redraw");
    }
    // A refused line leaves no undo step either: the last real step is still the ellipse.
    assert_eq!(session.undo().unwrap().as_deref(), Some("Ellipse"), "no refused line pushed an undo step");
}

#[test]
fn undo_and_redo_of_a_wipeout_return_the_state_before_and_after() {
    let mut session = Session::new();
    let before = session.doc().unwrap().clone();

    run(&mut session, "wipeout 0,0 10,0 10,10");
    let after = session.doc().unwrap().clone();
    assert_eq!(session.doc().unwrap().entity_count(), 1);
    assert!(matches!(session.doc().unwrap().model.iter().next().expect("the mask").kind, EntityKind::Wipeout(_)));
    assert_ne!(&after, &before);

    assert_eq!(session.undo().unwrap().as_deref(), Some("Wipeout"));
    assert_eq!(session.doc().unwrap(), &before, "undo of the wipeout removes the mask");
    assert_eq!(session.redo().unwrap().as_deref(), Some("Wipeout"));
    assert_eq!(session.doc().unwrap(), &after, "redo masks again");
}

#[test]
fn a_remote_wipeout_keeps_the_receivers_cursor_and_selection() {
    let mut server = Session::new();
    let mut receiver = Session::new();
    let empty = receiver.doc().unwrap().clone();
    server.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    server.execute("circle", &json!({"center": [20.0, 20.0], "radius": 2.0})).unwrap();
    let mut registry = cadcraft_collab::TableRegistry::new();
    let created = cadcraft_collab::diff_all(&empty, server.doc().unwrap(), &mut registry, &Default::default()).unwrap();
    receiver.apply_remote(&created).unwrap();
    let line = receiver.doc().unwrap().model.iter().next().expect("the line arrived").handle;
    let circle = receiver.doc().unwrap().model.iter().nth(1).expect("the circle arrived").handle;
    receiver.set_selection(vec![line, circle]);
    receiver.cursor = cadcraft_geom::Vec2::new(3.0, 4.0);

    // Another participant masks part of the drawing; the mask is one new object.
    let before = server.doc().unwrap().clone();
    server.execute("wipeout", &json!({"points": [[0.0, 0.0], [5.0, 0.0], [5.0, 5.0], [0.0, 5.0]]})).unwrap();
    let remote = cadcraft_collab::diff_all(&before, server.doc().unwrap(), &mut registry, &Default::default()).unwrap();
    assert!(remote.iter().any(|change| matches!(change, cadcraft_collab::Change::EntityCreate { .. })), "the mask arrives as a creation");

    receiver.apply_remote(&remote).unwrap();

    assert_eq!(receiver.selection(), vec![line, circle], "an entity that survives the change stays selected");
    assert_eq!(receiver.cursor, cadcraft_geom::Vec2::new(3.0, 4.0), "the cursor does not follow a remote change");
    assert_eq!(receiver.doc().unwrap(), server.doc().unwrap());
}

#[test]
fn wipeout_puts_its_object_on_the_authors_layer() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();
    add_layers(&mut server, &mut [&mut alice, &mut bob], &["WALLS", "DETAILS"]);
    alice.execute("layer.current", &json!({ "name": "WALLS" })).unwrap();
    bob.execute("layer.current", &json!({ "name": "DETAILS" })).unwrap();

    // Alice draws the closed boundary on WALLS; Bob masks it, so his mask is on DETAILS.
    submit(&mut server, &mut [&mut alice, &mut bob], 0, "rect 0,0 10,5");
    let rect = bob.doc().unwrap().model.iter().next().expect("the rectangle arrived").handle;
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("wipeout {}", rect.hex()));

    assert_eq!(alice.doc().unwrap().space(&Space::Model), bob.doc().unwrap().space(&Space::Model), "the participants agree on the objects");
    assert_eq!(alice.doc().unwrap().space(&Space::Model), server.doc().unwrap().space(&Space::Model), "and agree with the authoritative drawing");
    let layers: Vec<&str> = alice.doc().unwrap().model.iter().map(|entity| entity.common.layer.as_str()).collect();
    assert_eq!(layers, vec!["WALLS", "DETAILS"], "the boundary stays on Alice's layer; Bob's mask is on his");
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

/// A 1×1 red PNG: a raster IMAGEATTACH decodes.
const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";

/// The app installs the raster reader from the lower layer; the test installs the same decoder so
/// IMAGEATTACH really decodes the bytes the operation carries.
fn install_image_decoder() {
    fn decode(bytes: &[u8]) -> Result<cadcraft_doc::Raster, String> {
        let image = image::load_from_memory(bytes).map_err(|error| error.to_string())?.into_rgba8();
        cadcraft_doc::Raster::new(image.width(), image.height(), image.into_raw()).ok_or_else(|| "the image is empty".to_string())
    }
    cadcraft_engine::cmd::file::set_image_decoder(decode);
}

#[test]
fn two_replicas_that_attach_an_image_converge() {
    install_image_decoder();
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    // The bytes travel in the operation; the file name is only the label saved in the drawing
    // (ADR-0021). The placement, the width, the rotation and the name are explicit.
    submit(&mut server, &mut [&mut alice, &mut bob], 0, &format!("imageattach site.png data {PNG_1X1} at 10,5 width 100 rotation 15 name \"Site\""));
    // A second participant attaches another image, sized with `scale`.
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("imageattach logo.jpg data {PNG_1X1} scale 2"));

    assert_eq!(alice.doc().unwrap().space(&Space::Model), bob.doc().unwrap().space(&Space::Model), "the participants agree on the images");
    assert_eq!(alice.doc().unwrap().space(&Space::Model), server.doc().unwrap().space(&Space::Model), "and agree with the authoritative drawing");
    assert_eq!(alice.doc().unwrap().entity_count(), 2);
    let images: Vec<(String, String)> = alice
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|entity| match &entity.kind {
            EntityKind::Image(image) => Some((image.path.clone(), image.name.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        images,
        vec![("site.png".to_string(), "Site".to_string()), ("logo.jpg".to_string(), "logo".to_string())],
        "both replicas save the same label and definition name"
    );
    assert!(alice.doc().unwrap().rasters.is_empty(), "the pixels stay with the author");
    assert!(bob.doc().unwrap().rasters.is_empty(), "the change set carries the placement, not the raster");
    assert!(!server.doc().unwrap().rasters.is_empty(), "the server decoded the bytes the operation carried");
}

#[test]
fn a_refused_imageattach_does_not_change_the_drawing() {
    let mut session = Session::new();
    // Two real steps after the first, so a refused line that pushed an undo step would show.
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    session.execute("circle", &json!({"center": [20.0, 20.0], "radius": 2.0})).unwrap();
    session.execute("rectang", &json!({"p1": [30.0, 0.0], "p2": [40.0, 5.0]})).unwrap();
    let state = session.state().unwrap();
    let before = state.doc.clone();
    let before_revision = state.revision;
    let over_limit = "A".repeat((1 << 20) - 4096 + 1);

    let refused = [
        // Nothing to attach, no bytes, the file-reading form instead of `data`.
        "imageattach".to_string(),
        "imageattach map.png".to_string(),
        format!("imageattach map.png {PNG_1X1}"),
        format!("imageattach map.png path {PNG_1X1}"),
        // Empty and broken base64.
        "imageattach map.png data \"\"".to_string(),
        "imageattach map.png data not-base64!".to_string(),
        // A label that is not a raster type.
        format!("imageattach notes.txt data {PNG_1X1}"),
        // Data over the operation's limit.
        format!("imageattach map.png data {over_limit}"),
        // A coordinate that is not a number.
        format!("imageattach map.png data {PNG_1X1} at ten,5"),
        // An empty name.
        format!("imageattach map.png data {PNG_1X1} name \"\""),
        // A rotation with no size to rotate.
        format!("imageattach map.png data {PNG_1X1} rotation 15"),
        // A size given twice and a zero size.
        format!("imageattach map.png data {PNG_1X1} width 10 scale 2"),
        format!("imageattach map.png data {PNG_1X1} width 0"),
    ];
    for text in &refused {
        let error = cmdline::op_for(text, &state.doc, &state.space, "local-0", 0).unwrap_err();
        assert!(!error.to_string().is_empty(), "{}", &text[..text.len().min(48)]);
        assert_eq!(session.doc().unwrap(), &*before, "`{}` never becomes an operation", &text[..text.len().min(48)]);
        assert_eq!(session.state().unwrap().revision, before_revision, "`{}` does not redraw", &text[..text.len().min(48)]);
    }
    // A refused line leaves no undo step either: the last real step is still the rectangle.
    assert_eq!(session.undo().unwrap().as_deref(), Some("Rectangle"), "no refused line pushed an undo step");
}

#[test]
fn undo_and_redo_of_an_imageattach_return_the_state_before_and_after() {
    install_image_decoder();
    let mut session = Session::new();
    let before = session.doc().unwrap().clone();

    run(&mut session, &format!("imageattach map.png data {PNG_1X1} at 1,2 width 10"));
    let after = session.doc().unwrap().clone();
    assert_eq!(session.doc().unwrap().entity_count(), 1);
    assert!(matches!(session.doc().unwrap().model.iter().next().expect("the image").kind, EntityKind::Image(_)));
    assert_ne!(&after, &before);

    assert_eq!(session.undo().unwrap().as_deref(), Some("Raster Image Reference..."));
    assert_eq!(session.doc().unwrap(), &before, "undo removes the image and its pixels in one step");
    assert!(session.doc().unwrap().rasters.is_empty());
    assert_eq!(session.redo().unwrap().as_deref(), Some("Raster Image Reference..."));
    assert_eq!(session.doc().unwrap(), &after, "redo attaches it again");
}

#[test]
fn a_remote_imageattach_keeps_the_receivers_cursor_and_selection() {
    install_image_decoder();
    let mut server = Session::new();
    let mut receiver = Session::new();
    let empty = receiver.doc().unwrap().clone();
    server.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    server.execute("circle", &json!({"center": [20.0, 20.0], "radius": 2.0})).unwrap();
    let mut registry = cadcraft_collab::TableRegistry::new();
    let created = cadcraft_collab::diff_all(&empty, server.doc().unwrap(), &mut registry, &Default::default()).unwrap();
    receiver.apply_remote(&created).unwrap();
    let line = receiver.doc().unwrap().model.iter().next().expect("the line arrived").handle;
    let circle = receiver.doc().unwrap().model.iter().nth(1).expect("the circle arrived").handle;
    receiver.set_selection(vec![line, circle]);
    receiver.cursor = cadcraft_geom::Vec2::new(3.0, 4.0);

    // Another participant attaches an image; it is one new object with no address to follow.
    let before = server.doc().unwrap().clone();
    server.execute("imageattach", &json!({"path": "map.png", "data": PNG_1X1, "at": [0.0, 0.0], "width": 10.0})).unwrap();
    let remote = cadcraft_collab::diff_all(&before, server.doc().unwrap(), &mut registry, &Default::default()).unwrap();
    assert!(remote.iter().any(|change| matches!(change, cadcraft_collab::Change::EntityCreate { .. })), "the image arrives as a creation");

    receiver.apply_remote(&remote).unwrap();

    assert_eq!(receiver.selection(), vec![line, circle], "an entity that survives the change stays selected");
    assert_eq!(receiver.cursor, cadcraft_geom::Vec2::new(3.0, 4.0), "the cursor does not follow a remote change");
    assert_eq!(receiver.doc().unwrap().space(&Space::Model), server.doc().unwrap().space(&Space::Model));
    assert!(receiver.doc().unwrap().rasters.is_empty(), "the receiver does not get the author's pixels");
}

#[test]
fn imageattach_puts_its_object_on_the_authors_layer() {
    install_image_decoder();
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();
    add_layers(&mut server, &mut [&mut alice, &mut bob], &["WALLS", "DETAILS"]);
    alice.execute("layer.current", &json!({ "name": "WALLS" })).unwrap();
    bob.execute("layer.current", &json!({ "name": "DETAILS" })).unwrap();

    // Bob attaches the image; it belongs to the layer he is drawing on, not the server's.
    submit(&mut server, &mut [&mut alice, &mut bob], 1, &format!("imageattach plan.png data {PNG_1X1} at 0,0 width 50"));

    assert_eq!(alice.doc().unwrap().space(&Space::Model), bob.doc().unwrap().space(&Space::Model), "the participants agree on the image");
    assert_eq!(alice.doc().unwrap().space(&Space::Model), server.doc().unwrap().space(&Space::Model), "and agree with the authoritative drawing");
    let layers: Vec<&str> = alice.doc().unwrap().model.iter().map(|entity| entity.common.layer.as_str()).collect();
    assert_eq!(layers, vec!["DETAILS"], "the image is on its author's layer");
    assert_eq!(alice.doc().unwrap().header.str("CLAYER", "?"), "WALLS", "nobody's current layer follows somebody else's command");
    assert_eq!(bob.doc().unwrap().header.str("CLAYER", "?"), "DETAILS");
    assert_eq!(server.doc().unwrap().header.str("CLAYER", "?"), "0", "the server keeps its own current layer");
}

#[test]
fn two_replicas_that_draw_an_arc_and_a_dimaligned_converge() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();

    // Each participant types one of the two new commands; the change set carries both.
    submit(&mut server, &mut [&mut alice, &mut bob], 0, "arc 0,0 10 0 90");
    submit(&mut server, &mut [&mut alice, &mut bob], 1, "dimaligned 0,0 10,0 5,2");

    assert_eq!(alice.doc().unwrap(), bob.doc().unwrap(), "the participants agree");
    assert_eq!(alice.doc().unwrap(), server.doc().unwrap(), "and agree with the authoritative drawing");
    assert_eq!(alice.doc().unwrap().entity_count(), 2, "the arc and the dimension");
    let arc = alice
        .doc()
        .unwrap()
        .model
        .iter()
        .find_map(|entity| match &entity.kind {
            EntityKind::Arc(arc) => Some(arc.clone()),
            _ => None,
        })
        .expect("the arc arrived");
    assert!((arc.radius - 10.0).abs() < 1e-9, "the radius is the one typed");
    assert!((arc.start - 0.0_f64.to_radians()).abs() < 1e-9 && (arc.end - 90.0_f64.to_radians()).abs() < 1e-9, "the angles are degrees in the line");
    let dimensions = alice.doc().unwrap().model.iter().filter(|entity| matches!(entity.kind, EntityKind::Dimension(_))).count();
    assert_eq!(dimensions, 1, "the aligned dimension arrived");
}

#[test]
fn a_refused_arc_or_dimaligned_does_not_change_the_drawing() {
    let mut session = Session::new();
    // Two real steps after the first, so a refused line that pushed an undo step would show.
    session.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    session.execute("circle", &json!({"center": [20.0, 20.0], "radius": 2.0})).unwrap();
    session.execute("rectang", &json!({"p1": [30.0, 0.0], "p2": [40.0, 5.0]})).unwrap();
    let state = session.state().unwrap();
    let before = state.doc.clone();
    let before_revision = state.revision;

    let refused = [
        // An arc with no radius, a non-numeric and an out-of-limit coordinate or angle.
        "arc 0,0 0 0 90".to_string(),
        "arc ten,0 1 0 90".to_string(),
        "arc 0,0 1e13 0 90".to_string(),
        "arc 1e13,0 1 0 90".to_string(),
        "arc 0,0 1 zero 90".to_string(),
        "arc 0,0 1 0 later".to_string(),
        // An aligned dimension with coincident points, a non-numeric and an out-of-limit point.
        "dimaligned 0,0 0,0 5,2".to_string(),
        "dimaligned 0,0 10,0 later,2".to_string(),
        "dimaligned 0,0 1e13,0 5,2".to_string(),
    ];
    for text in &refused {
        let error = cmdline::op_for(text, &state.doc, &state.space, "local-0", 0).unwrap_err();
        assert!(!error.to_string().is_empty(), "{text}");
        assert_eq!(session.doc().unwrap(), &*before, "`{text}` never becomes an operation");
        assert_eq!(session.state().unwrap().revision, before_revision, "`{text}` does not redraw");
    }
    // A refused line leaves no undo step either: the last real step is still the rectangle.
    assert_eq!(session.undo().unwrap().as_deref(), Some("Rectangle"), "no refused line pushed an undo step");
}

#[test]
fn undo_and_redo_of_an_arc_and_a_dimaligned_return_the_state_before_and_after() {
    let mut session = Session::new();

    let before = session.doc().unwrap().clone();
    run(&mut session, "arc 0,0 10 0 90");
    let after_arc = session.doc().unwrap().clone();
    assert_ne!(&after_arc, &before);
    assert_eq!(session.undo().unwrap().as_deref(), Some("Arc"));
    assert_eq!(session.doc().unwrap(), &before, "undo takes the arc away in one step");
    assert_eq!(session.redo().unwrap().as_deref(), Some("Arc"));
    assert_eq!(session.doc().unwrap(), &after_arc, "redo draws it again");

    let before_dim = session.doc().unwrap().clone();
    run(&mut session, "dimaligned 0,0 10,0 5,2");
    let after_dim = session.doc().unwrap().clone();
    assert_eq!(session.doc().unwrap().entity_count(), 2);
    assert_eq!(session.undo().unwrap().as_deref(), Some("Aligned"));
    assert_eq!(session.doc().unwrap(), &before_dim, "undo takes the dimension away in one step");
    assert_eq!(session.redo().unwrap().as_deref(), Some("Aligned"));
    assert_eq!(session.doc().unwrap(), &after_dim, "redo builds it again");
}

#[test]
fn a_remote_arc_and_dimaligned_keep_the_receivers_cursor_and_selection() {
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

    // Another participant draws an arc and an aligned dimension; both are new objects with no
    // address to follow, so the receiver's cursor and selection stay where they were.
    let before = server.doc().unwrap().clone();
    server.execute("arc", &json!({"center": [30.0, 30.0], "radius": 5.0, "start": 0.0, "end": 90.0})).unwrap();
    server.execute("dimaligned", &json!({"p1": [0.0, 0.0], "p2": [10.0, 0.0], "at": [5.0, 2.0]})).unwrap();
    let remote = diff(&before, server.doc().unwrap(), &Space::Model).unwrap();
    assert!(
        remote.iter().any(|change| matches!(change, cadcraft_collab::Change::EntityCreate { .. })),
        "the arc and the dimension arrive as creations"
    );

    receiver.apply_remote(&remote).unwrap();

    assert_eq!(receiver.selection(), vec![line, circle], "an entity that survives the change stays selected");
    assert_eq!(receiver.cursor, cadcraft_geom::Vec2::new(3.0, 4.0), "the cursor does not follow a remote change");
    assert_eq!(receiver.doc().unwrap(), server.doc().unwrap());
}

#[test]
fn arc_and_dimaligned_put_their_objects_on_the_authors_layer() {
    let mut server = Session::new();
    let mut alice = Session::new();
    let mut bob = Session::new();
    add_layers(&mut server, &mut [&mut alice, &mut bob], &["WALLS", "DETAILS"]);
    alice.execute("layer.current", &json!({ "name": "WALLS" })).unwrap();
    bob.execute("layer.current", &json!({ "name": "DETAILS" })).unwrap();

    // Both commands create objects, so both belong to the layer their author draws on.
    submit(&mut server, &mut [&mut alice, &mut bob], 1, "arc 0,0 10 0 90");
    submit(&mut server, &mut [&mut alice, &mut bob], 1, "dimaligned 0,0 10,0 5,2");

    assert_eq!(alice.doc().unwrap().space(&Space::Model), bob.doc().unwrap().space(&Space::Model), "the participants agree on the objects");
    assert_eq!(alice.doc().unwrap().space(&Space::Model), server.doc().unwrap().space(&Space::Model), "and agree with the authoritative drawing");
    let layers: Vec<&str> = alice.doc().unwrap().model.iter().map(|entity| entity.common.layer.as_str()).collect();
    assert_eq!(layers, vec!["DETAILS", "DETAILS"], "the arc and the dimension are on Bob's layer");
    assert_eq!(alice.doc().unwrap().header.str("CLAYER", "?"), "WALLS", "nobody's current layer follows somebody else's command");
    assert_eq!(bob.doc().unwrap().header.str("CLAYER", "?"), "DETAILS");
    assert_eq!(server.doc().unwrap().header.str("CLAYER", "?"), "0", "the server keeps its own current layer");
}
