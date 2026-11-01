//! The post-command passes (group pruning, constraint purge/re-solve, associative hatch and
//! dimension updates) run the same whether a command is executed programmatically or typed and
//! finished interactively, and their edits share that command's undo step.

use cadcraft_engine::doc::{ConstraintKind, EntityKind, Handle};
use cadcraft_engine::geom::{Bounds2, Polyline, Vec2};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn handle(v: &Value) -> Handle {
    Handle::parse_hex(v["handles"][0].as_str().or(v["handle"].as_str()).unwrap()).unwrap()
}

/// Two lines meeting at (10, 0), coincident there with the first one fixed, grouped together;
/// and a 4 x 2 rectangle with an associative hatch. Handles: line a, line b, rectangle, hatch.
fn setup() -> (Session, [Handle; 4]) {
    let mut s = Session::new();
    s.viewport_px = (1000.0, 1000.0);
    let a = handle(&s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap());
    let b = handle(&s.execute("line", &json!({ "points": [[10, 0], [10, 5]] })).unwrap());
    s.execute("gccoincident", &json!({ "h1": a.hex(), "p1": "end", "h2": b.hex(), "p2": "start" })).unwrap();
    s.execute("gcfix", &json!({ "h1": a.hex() })).unwrap();
    s.execute("group", &json!({ "name": "G", "handles": [a.hex(), b.hex()] })).unwrap();
    let r = handle(&s.execute("rectang", &json!({ "p1": [20, 0], "p2": [24, 2] })).unwrap());
    let h = handle(&s.execute("hatch", &json!({ "handles": [r.hex()] })).unwrap());
    (s, [a, b, r, h])
}

/// MOVE `hs` by (3, 2), either as one `execute` call or typed at the command line.
fn move_by(s: &mut Session, hs: &[Handle], typed: bool) {
    if typed {
        s.cmdline("move").unwrap();
        s.input(Input::Pick(hs.to_vec())).unwrap();
        s.input(Input::Enter).unwrap();
        s.cmdline("0,0").unwrap();
        s.cmdline("3,2").unwrap();
        assert!(s.running.is_none(), "{}", s.prompt_text());
    } else {
        let hs: Vec<String> = hs.iter().map(|h| h.hex()).collect();
        s.execute("move", &json!({ "handles": hs, "delta": [3, 2] })).unwrap();
    }
}

/// ERASE `hs`, either as one `execute` call or typed at the command line.
fn erase(s: &mut Session, hs: &[Handle], typed: bool) {
    if typed {
        s.cmdline("erase").unwrap();
        s.input(Input::Pick(hs.to_vec())).unwrap();
        s.cmdline("").unwrap();
        assert!(s.running.is_none(), "{}", s.prompt_text());
    } else {
        let hs: Vec<String> = hs.iter().map(|h| h.hex()).collect();
        s.execute("erase", &json!({ "handles": hs })).unwrap();
    }
}

fn line_start(s: &Session, h: Handle) -> Vec2 {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Line(l) => Vec2::new(l.a.x, l.a.y),
        k => panic!("not a line: {k:?}"),
    }
}

fn hatch_bounds(s: &Session, h: Handle) -> (bool, Bounds2) {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Hatch(x) => (
            x.associative,
            Bounds2::from_points(x.loops.iter().flat_map(|l| Polyline { vertices: l.vertices.clone(), closed: true }.tessellate(1e-4))),
        ),
        k => panic!("not a hatch: {k:?}"),
    }
}

fn groups(s: &Session) -> Vec<(String, Vec<Handle>)> {
    s.doc().unwrap().groups.iter().map(|g| (g.name.clone(), g.members.clone())).collect()
}

fn constraint_kinds(s: &Session) -> Vec<ConstraintKind> {
    s.doc().unwrap().constraints.iter().map(|c| c.kind).collect()
}

/// Everything the passes touch, for comparing the two paths.
fn passes_state(s: &Session) -> String {
    let d = s.doc().unwrap();
    format!("{:?}\n{:?}\n{:?}", d.model.iter().map(|e| (e.handle, &e.kind)).collect::<Vec<_>>(), d.groups, d.constraints)
}

#[test]
fn move_runs_every_pass_on_both_paths() {
    let mut states = Vec::new();
    for typed in [false, true] {
        let (mut s, [a, b, r, h]) = setup();
        let original = passes_state(&s);
        let undo = s.state().unwrap().undo.len();
        move_by(&mut s, &[b, r], typed);
        // Constraints: the coincidence pulls b's start back onto a's fixed end.
        assert!(line_start(&s, b).near(Vec2::new(10.0, 0.0), 1e-6), "typed={typed}: {:?}", line_start(&s, b));
        // Associativity: the hatch followed its boundary.
        let (assoc, bb) = hatch_bounds(&s, h);
        assert!(assoc, "typed={typed}");
        assert!(bb.min.near(Vec2::new(23.0, 2.0), 1e-6) && bb.max.near(Vec2::new(27.0, 4.0), 1e-6), "typed={typed}: {bb:?}");
        assert_eq!(groups(&s), vec![("G".to_string(), vec![a, b])]);
        // One undo step holds the command and every pass's edits.
        assert_eq!(s.state().unwrap().undo.len(), undo + 1, "typed={typed}");
        states.push(passes_state(&s));
        s.undo().unwrap();
        assert_eq!(passes_state(&s), original, "typed={typed}: undo restores everything");
    }
    assert_eq!(states[0], states[1], "execute and the interactive finish agree");
}

#[test]
fn erase_runs_every_pass_on_both_paths() {
    let mut states = Vec::new();
    for typed in [false, true] {
        let (mut s, [a, b, r, h]) = setup();
        let original = passes_state(&s);
        let undo = s.state().unwrap().undo.len();
        erase(&mut s, &[b, r], typed);
        // Groups: b left G.
        assert_eq!(groups(&s), vec![("G".to_string(), vec![a])], "typed={typed}");
        // Constraints: the coincidence went with b; the fix on a stays.
        assert_eq!(constraint_kinds(&s), vec![ConstraintKind::Fix], "typed={typed}");
        // Associativity: erasing the boundary ends the hatch's link.
        assert!(!hatch_bounds(&s, h).0, "typed={typed}");
        assert_eq!(s.state().unwrap().undo.len(), undo + 1, "typed={typed}");
        states.push(passes_state(&s));
        s.undo().unwrap();
        assert_eq!(passes_state(&s), original, "typed={typed}: undo restores everything");
    }
    assert_eq!(states[0], states[1], "execute and the interactive finish agree");
}
