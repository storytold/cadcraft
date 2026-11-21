//! TRIM of a line, an arc or a circle picked on a cut, as AutoCAD 2024 does it (measured with
//! accoreconsole on these drawings): a cut within 5e-7 of the pick, along the object, is passed
//! over, and the piece runs on across it to the next cut either side. With no cut left, nothing is
//! trimmed.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::{Value, json};

fn handle(v: &Value) -> Handle {
    Handle::parse_hex(v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap()).unwrap()
}

/// Vertical lines through x = `xs`, from y = -15 to 15; their handles.
fn edges(s: &mut Session, xs: &[f64]) -> Vec<String> {
    xs.iter().map(|x| handle(&s.execute("line", &json!({ "points": [[x, -15], [x, 15]] })).unwrap()).hex()).collect()
}

/// Every object but the edges: lines as their ends, arcs and circles as their angles in degrees.
fn shapes(s: &Session, edges: &[String]) -> Vec<(Handle, Vec<f64>)> {
    let r = |v: f64| (v * 1e9).round() / 1e9;
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter(|e| !edges.contains(&e.handle.hex()))
        .map(|e| {
            let v = match &e.kind {
                EntityKind::Line(l) => vec![l.a.x, l.a.y, l.b.x, l.b.y],
                EntityKind::Arc(a) => vec![a.start.to_degrees(), a.end.to_degrees()],
                EntityKind::Circle(_) => Vec::new(),
                k => panic!("{k:?}"),
            };
            (e.handle, v.into_iter().map(r).collect())
        })
        .collect()
}

/// Trim `h` at `pick` with `edges`: whether the trim was made.
fn trim(s: &mut Session, h: Handle, pick: [f64; 2], edges: &[String]) -> bool {
    s.execute("trim", &json!({ "handle": h.hex(), "pick": pick, "edges": edges })).is_ok()
}

/// A line from (0, 0) to (10, 0) with vertical lines through `xs`, trimmed at `pick`.
fn line_trimmed(xs: &[f64], pick: [f64; 2]) -> (bool, Vec<(Handle, Vec<f64>)>, Handle) {
    let mut s = Session::new();
    let h = handle(&s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap());
    let es = edges(&mut s, xs);
    let made = trim(&mut s, h, pick, &es);
    (made, shapes(&s, &es), h)
}

#[test]
fn a_line_picked_on_its_only_cut_is_left_as_it_was() {
    for pick in [[5.0, 0.0], [5.0000004, 0.0], [4.9999996, 0.0]] {
        let (made, got, h) = line_trimmed(&[5.0], pick);
        assert!(!made, "{pick:?}");
        assert_eq!(got, vec![(h, vec![0.0, 0.0, 10.0, 0.0])], "{pick:?}");
    }
}

#[test]
fn a_line_picked_on_one_of_two_cuts_loses_the_pieces_both_sides() {
    // Cuts at x = 3 and 7: on 3, 0 to 7 goes; 4e-7 past 7, 3 to the end; between them, 3 to 7.
    let (_, got, h) = line_trimmed(&[3.0, 7.0], [3.0, 0.0]);
    assert_eq!(got, vec![(h, vec![7.0, 0.0, 10.0, 0.0])]);
    let (_, got, h) = line_trimmed(&[3.0, 7.0], [7.0000004, 0.0]);
    assert_eq!(got, vec![(h, vec![0.0, 0.0, 3.0, 0.0])]);
    let (_, got, h) = line_trimmed(&[3.0, 7.0], [5.0, 0.0]);
    assert_eq!(got.len(), 2);
    assert_eq!(got[0], (h, vec![0.0, 0.0, 3.0, 0.0]));
    assert_eq!(got[1].1, vec![7.0, 0.0, 10.0, 0.0]);
}

/// An arc of radius 10 about the origin from 0 to 180 degrees, with vertical lines through `xs`,
/// trimmed at `pick`.
fn arc_trimmed(xs: &[f64], pick: [f64; 2]) -> (bool, Vec<(Handle, Vec<f64>)>, Handle) {
    let mut s = Session::new();
    let h = handle(&s.execute("arc", &json!({ "center": [0, 0], "radius": 10, "start": 0, "end": 180 })).unwrap());
    let es = edges(&mut s, xs);
    let made = trim(&mut s, h, pick, &es);
    (made, shapes(&s, &es), h)
}

#[test]
fn an_arc_picked_on_its_only_cut_is_left_as_it_was() {
    // The line x = 0 cuts it at 90 degrees: on the cut and 4e-7 along from it nothing is trimmed,
    // 6e-7 along the arc from 90 to 180 goes.
    for pick in [[0.0, 10.0], [-4e-7, 10.0]] {
        let (made, got, h) = arc_trimmed(&[0.0], pick);
        assert!(!made, "{pick:?}");
        assert_eq!(got, vec![(h, vec![0.0, 180.0])], "{pick:?}");
    }
    let (_, got, h) = arc_trimmed(&[0.0], [-6e-7, 10.0]);
    assert_eq!(got, vec![(h, vec![0.0, 90.0])]);
}

#[test]
fn an_arc_picked_on_its_last_cut_loses_the_rest() {
    // Cuts at 60 and 120 degrees, picked on 120: from 60 to the end goes.
    let (_, got, h) = arc_trimmed(&[5.0, -5.0], [-5.0, 75f64.sqrt()]);
    assert_eq!(got, vec![(h, vec![0.0, 60.0])]);
}

/// A circle of radius 10 about the origin, with vertical lines through `xs`, trimmed at `pick`.
fn circle_trimmed(xs: &[f64], pick: [f64; 2]) -> (bool, Vec<(Handle, Vec<f64>)>, Handle) {
    let mut s = Session::new();
    let h = handle(&s.execute("circle", &json!({ "center": [0, 0], "radius": 10 })).unwrap());
    let es = edges(&mut s, xs);
    let made = trim(&mut s, h, pick, &es);
    (made, shapes(&s, &es), h)
}

#[test]
fn a_circle_picked_on_a_cut_loses_the_arcs_both_sides() {
    // Lines at x = 5 and -5 cut it at 60, 120, 240 and 300 degrees. Picked on 60, and 4e-7 along
    // from it, the arc from 300 round to 120 goes; picked at 90, the arc from 60 to 120.
    let y = 75f64.sqrt();
    for pick in [[5.0, y], [4.9999998, y + 2e-7 * 5.0 / y]] {
        let (_, got, h) = circle_trimmed(&[5.0, -5.0], pick);
        assert_eq!(got, vec![(h, vec![120.0, 300.0])], "{pick:?}");
    }
    let (_, got, h) = circle_trimmed(&[5.0, -5.0], [0.0, 10.0]);
    assert_eq!(got, vec![(h, vec![120.0, 60.0])]);
}

#[test]
fn a_circle_picked_on_one_of_two_cuts_is_left_as_it_was() {
    // One line, cutting it at 60 and 300 degrees: picked on 60, one cut is left and the trim is
    // refused.
    let (made, got, h) = circle_trimmed(&[5.0], [5.0, 75f64.sqrt()]);
    assert!(!made);
    assert_eq!(got, vec![(h, Vec::new())]);
}

#[test]
fn quick_mode_leaves_an_object_picked_on_its_only_cut() {
    // The line, the arc and the circle each meet the edges, at the pick, so Quick mode doesn't
    // erase them as objects no edge crosses.
    let y = 75f64.sqrt();
    let cases = [
        ("line", json!({ "points": [[0, 0], [10, 0]] }), 5.0, [5.0, 0.0]),
        ("arc", json!({ "center": [0, 0], "radius": 10, "start": 0, "end": 180 }), 0.0, [0.0, 10.0]),
        ("circle", json!({ "center": [0, 0], "radius": 10 }), 5.0, [5.0, y]),
    ];
    for (cmd, shape, x, pick) in cases {
        let mut s = Session::new();
        let h = handle(&s.execute(cmd, &shape).unwrap());
        let es = edges(&mut s, &[x]);
        let r = s.execute("trim", &json!({ "handle": h.hex(), "pick": pick, "mode": "quick" }));
        assert!(r.is_err(), "{cmd}");
        assert!(s.doc().unwrap().entity(h).is_some(), "{cmd} was erased");
        assert_eq!(shapes(&s, &es).len(), 1, "{cmd}");
    }
}

#[test]
fn an_arc_picked_just_off_one_end_loses_the_piece_at_that_end() {
    // From 0 to 90 degrees, cut at 45: picked just before its start the piece from 0 goes, just
    // past its end the piece to 90.
    let x = 10.0 * std::f64::consts::FRAC_PI_4.cos();
    let trimmed = |pick: [f64; 2]| {
        let mut s = Session::new();
        let h = handle(&s.execute("arc", &json!({ "center": [0, 0], "radius": 10, "start": 0, "end": 90 })).unwrap());
        let es = edges(&mut s, &[x]);
        assert!(trim(&mut s, h, pick, &es));
        shapes(&s, &es)
    };
    assert_eq!(trimmed([10.0, -0.01])[0].1, vec![45.0, 90.0]);
    assert_eq!(trimmed([-0.01, 10.0])[0].1, vec![0.0, 45.0]);
}
