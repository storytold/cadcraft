//! DIMASSOC 1 creates dimension objects without coupling them to source geometry; 2 couples them.
use cadcraft_engine::Session;
use cadcraft_engine::doc::{Dimension, EntityKind, Handle};
use serde_json::{Value, json};

/// Read the single entity handle returned by a dimension command.
fn handle(v: &Value) -> Handle {
    Handle::parse_hex(v["handle"].as_str().unwrap()).unwrap()
}

/// Clone the asserted dimension so later edits can be compared with its previous state.
fn dimension(s: &Session, h: Handle) -> Dimension {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Dimension(d) => d.clone(),
        other => panic!("expected dimension, got {other:?}"),
    }
}

/// Create a synthetic source line and return its first entity handle.
fn line(s: &mut Session, a: [i32; 2], b: [i32; 2]) -> Handle {
    let v = s.execute("line", &json!({"points": [a, b]})).unwrap();
    Handle::parse_hex(v["handles"][0].as_str().unwrap()).unwrap()
}

/// Set the drawing-wide associativity mode before creating the test dimensions.
fn mode(s: &mut Session, n: i32) {
    s.execute("setvar", &json!({"name": "DIMASSOC", "value": n})).unwrap();
}

/// Typed and JSON dimensions retain their mode through DXF, source edits, undo and redo.
#[test]
fn linear_dimensions_respect_the_mode_through_edits_and_dxf() {
    for n in [1, 2] {
        for typed in [false, true] {
            let mut s = Session::new();
            mode(&mut s, n);
            let source = line(&mut s, [0, 0], [10, 0]);
            let d = if typed {
                s.cmdline("dimlinear 0,0 10,0 5,4").unwrap();
                s.last_dim.unwrap()
            } else {
                handle(&s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [10, 0], "at": [5, 4]})).unwrap())
            };
            assert_eq!(dimension(&s, d).assoc.len(), if n == 2 { 2 } else { 0 }, "mode{n}, typed{typed}");
            let saved = cadcraft_io::write_dxf(s.doc().unwrap());
            let has_link = saved.lines().any(|l| l.trim() == "DIMASSOC");
            assert_eq!(has_link, n == 2, "mode{n}: only associative dimensions save a DIMASSOC object");
            let mut reopened = Session::new();
            reopened.open_drawing(cadcraft_io::read_dxf(saved.as_bytes()).unwrap(), "roundtrip.dxf", None);
            for session in [&mut s, &mut reopened] {
                session.execute("properties.set", &json!({"handles": [source.hex()], "end": [15, 0]})).unwrap();
                let edited = dimension(session, d);
                assert_eq!(edited.p14.x, if n == 2 { 15.0 } else { 10.0 });
                assert_eq!(cadcraft_engine::render::dimension_in(session.doc().unwrap(), &edited).measurement, if n == 2 { 15.0 } else { 10.0 });
                session.undo().unwrap();
                assert_eq!(dimension(session, d).p14.x, 10.0);
                session.redo().unwrap();
                assert_eq!(dimension(session, d).p14.x, edited.p14.x);
            }
        }
    }
}

/// Continue, Baseline and Quick Dimension attach source links only in mode 2.
#[test]
fn indexed_chains_and_qdim_respect_the_mode() {
    for n in [1, 2] {
        for cmd in ["dimcontinue", "dimbaseline", "qdim"] {
            let mut s = Session::new();
            mode(&mut s, n);
            let source = line(&mut s, [0, 0], [10, 0]);
            s.execute("point", &json!({"at": [20, 0]})).unwrap();
            s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [10, 0], "at": [5, 4]})).unwrap();
            let params = if cmd == "qdim" { json!({"handles": [source.hex()], "at": [5, 6]}) } else { json!({"points": [[20, 0]]}) };
            let out = s.execute(cmd, &params).unwrap();
            let hs = out["handles"].as_array().unwrap();
            assert!(!hs.is_empty());
            for h in hs {
                let d = dimension(&s, Handle::parse_hex(h.as_str().unwrap()).unwrap());
                assert_eq!(d.assoc.len(), if n == 2 { 2 } else { 0 }, "mode{n}, {cmd}");
            }
        }
    }
}

/// Even explicitly selected angular source lines produce associations only in mode 2.
#[test]
fn explicit_angular_links_respect_the_mode() {
    for n in [1, 2] {
        let mut s = Session::new();
        mode(&mut s, n);
        let a = line(&mut s, [0, 0], [10, 0]);
        let b = line(&mut s, [0, 0], [0, 10]);
        let d = handle(&s.execute("dimangular", &json!({"lines": [a.hex(), b.hex()], "at": [4, 4]})).unwrap());
        assert_eq!(dimension(&s, d).assoc.len(), if n == 2 { 4 } else { 0 });
    }
}

/// Changing the creation mode preserves old links and does not disable explicit reassociation.
#[test]
fn mode_change_keeps_existing_links_and_allows_explicit_reassociation() {
    let mut s = Session::new();
    let source = line(&mut s, [0, 0], [10, 0]);
    let params = json!({"p1": [0, 0], "p2": [10, 0], "at": [5, 4]});
    let existing = handle(&s.execute("dimlinear", &params).unwrap());
    mode(&mut s, 1);
    let new = handle(&s.execute("dimlinear", &params).unwrap());
    assert_eq!(dimension(&s, existing).assoc.len(), 2);
    assert!(dimension(&s, new).assoc.is_empty());
    s.execute("dimreassociate", &json!({"handles": [new.hex()]})).unwrap();
    assert_eq!(dimension(&s, new).assoc.len(), 2);
    s.execute("properties.set", &json!({"handles": [source.hex()], "end": [15, 0]})).unwrap();
    assert_eq!(dimension(&s, existing).p14.x, 15.0);
    assert_eq!(dimension(&s, new).p14.x, 15.0);
}
