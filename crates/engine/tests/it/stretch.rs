//! STRETCH: repeated crossing selections until Enter, the `C`/`W` selection keywords, and the
//! Displacement option (typed, or Enter at the base point prompt).

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;

/// Every line as ((ax, ay), (bx, by)), sorted by the first x.
fn lines(s: &Session) -> Vec<((f64, f64), (f64, f64))> {
    let mut v: Vec<_> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some(((l.a.x, l.a.y), (l.b.x, l.b.y))) } else { None })
        .collect();
    v.sort_by(|a, b| a.0.0.total_cmp(&b.0.0).then(a.0.1.total_cmp(&b.0.1)));
    v
}

fn session_with_lines(lines: &[&str]) -> Session {
    let mut s = Session::new();
    for l in lines {
        s.cmdline(&format!("LINE {l}")).unwrap();
        s.cmdline("").unwrap();
    }
    s
}

#[test]
fn stretch_script_with_crossing_keyword() {
    let mut s = session_with_lines(&["0,0 10,0"]);
    s.script("STRETCH C 12,1 8,-1  0,0 5,0\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), vec![((0.0, 0.0), (15.0, 0.0))]);
}

#[test]
fn stretch_keeps_selecting_until_enter() {
    let mut s = session_with_lines(&["0,0 10,0", "0,5 10,5"]);
    // Two implied crossing windows (right to left), each around one right-hand end.
    s.cmdline("STRETCH").unwrap();
    for t in ["12,1", "8,-1", "12,6", "8,4", ""] {
        s.cmdline(t).unwrap();
    }
    assert!(s.prompt_text().contains("base point"), "{}", s.prompt_text());
    s.cmdline("0,0").unwrap();
    s.cmdline("0,2").unwrap();
    assert!(s.running.is_none());
    assert_eq!(lines(&s), vec![((0.0, 0.0), (10.0, 2.0)), ((0.0, 5.0), (10.0, 7.0))]);
}

#[test]
fn stretch_displacement_option() {
    // Enter at the base point prompt asks for a displacement.
    let mut s = session_with_lines(&["0,0 10,0"]);
    s.script("STRETCH C 12,1 8,-1\n\n\n3,4\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), vec![((0.0, 0.0), (13.0, 4.0))]);
    // The Displacement keyword does the same.
    let mut s = session_with_lines(&["0,0 10,0"]);
    s.script("STRETCH C 12,1 8,-1  D 3,4\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), vec![((0.0, 0.0), (13.0, 4.0))]);
    // Enter at the second point uses the base point as the displacement.
    let mut s = session_with_lines(&["0,0 10,0"]);
    s.script("STRETCH C 12,1 8,-1  2,0\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), vec![((0.0, 0.0), (12.0, 0.0))]);
}

/// #2779: a press-and-drag at "Select objects" opens a window, as at any selection prompt, and
/// STRETCH takes it. Before, the drag opened none and its end counted as one click: on an object
/// that picked it, so the object moved whole, as with MOVE.
#[test]
fn a_dragged_crossing_window_stretches() {
    let mut s = session_with_lines(&["0,0 10,0", "0,5 10,5"]);
    s.cmdline("STRETCH").unwrap();
    // Dragged right to left from empty space, ending on the lower line: a crossing window round
    // its right end, not a pick of the line.
    assert!(s.begin_window(cadcraft_engine::doc::geom::Vec2::new(12.0, 1.0)));
    s.input(cadcraft_engine::Input::Point(cadcraft_engine::doc::geom::Vec2::new(8.0, 0.0))).unwrap();
    assert!(s.pending_window.is_none());
    for t in ["", "0,0", "0,2"] {
        s.cmdline(t).unwrap();
    }
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), vec![((0.0, 0.0), (10.0, 2.0)), ((0.0, 5.0), (10.0, 5.0))]);

    // A drag that starts on an object still opens a window (PICKDRAG 2) and stretches.
    let mut s = session_with_lines(&["0,0 10,0"]);
    s.cmdline("STRETCH").unwrap();
    assert!(s.begin_window(cadcraft_engine::doc::geom::Vec2::new(10.0, 0.0)));
    s.input(cadcraft_engine::Input::Point(cadcraft_engine::doc::geom::Vec2::new(8.0, -1.0))).unwrap();
    for t in ["", "0,0", "3,0"] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(lines(&s), vec![((0.0, 0.0), (13.0, 0.0))]);

    // Dragged left to right round the whole line: a window, so the line moves whole.
    let mut s = session_with_lines(&["0,0 10,0"]);
    s.cmdline("STRETCH").unwrap();
    assert!(s.begin_window(cadcraft_engine::doc::geom::Vec2::new(-1.0, -1.0)));
    s.input(cadcraft_engine::Input::Point(cadcraft_engine::doc::geom::Vec2::new(11.0, 1.0))).unwrap();
    for t in ["", "0,0", "0,3"] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(lines(&s), vec![((0.0, 3.0), (10.0, 3.0))]);

    // At "Remove objects" a dragged window takes objects out again: the upper line stays put.
    let mut s = session_with_lines(&["0,0 10,0", "0,5 10,5"]);
    s.cmdline("STRETCH").unwrap();
    assert!(s.begin_window(cadcraft_engine::doc::geom::Vec2::new(12.0, 6.0)));
    s.input(cadcraft_engine::Input::Point(cadcraft_engine::doc::geom::Vec2::new(8.0, -1.0))).unwrap();
    s.cmdline("r").unwrap();
    assert!(s.prompt_text().contains("Remove objects"), "{}", s.prompt_text());
    assert!(s.begin_window(cadcraft_engine::doc::geom::Vec2::new(12.0, 6.0)));
    s.input(cadcraft_engine::Input::Point(cadcraft_engine::doc::geom::Vec2::new(8.0, 4.0))).unwrap();
    for t in ["", "0,0", "0,2"] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(lines(&s), vec![((0.0, 0.0), (10.0, 2.0)), ((0.0, 5.0), (10.0, 5.0))]);

    // Past "Select objects" a drag opens no window: the base point is a point.
    let mut s = session_with_lines(&["0,0 10,0"]);
    s.cmdline("STRETCH").unwrap();
    for t in ["12,1", "8,-1", ""] {
        s.cmdline(t).unwrap();
    }
    assert!(!s.begin_window(cadcraft_engine::doc::geom::Vec2::new(0.0, 0.0)));
    assert!(s.pending_window.is_none());
}
