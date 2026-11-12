//! Selection modes typed at "Select objects": Window, Crossing, Fence, WPolygon, CPolygon.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;

/// Three lines on the X axis: A 0–10, B 12–20, C 30–40.
fn drawing() -> Session {
    let mut s = Session::new();
    for l in ["LINE 0,0 10,0", "LINE 12,0 20,0", "LINE 30,0 40,0"] {
        s.cmdline(l).unwrap();
        s.cmdline("").unwrap();
    }
    s
}

/// The start X of the lines left in the drawing.
fn left(s: &Session) -> Vec<f64> {
    let mut v: Vec<f64> = s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some(l.a.x) } else { None }).collect();
    v.sort_by(f64::total_cmp);
    v
}

fn erase(input: &str) -> Vec<f64> {
    let mut s = drawing();
    s.cmdline(input).unwrap();
    assert!(s.running.is_some(), "{input}: still selecting");
    s.cmdline("").unwrap();
    assert!(s.running.is_none(), "{input}: Enter ends the selection");
    left(&s)
}

#[test]
fn crossing_and_window_ignore_the_drag_direction() {
    // Left to right would be a window; C makes it a crossing: A (touching) and B (inside).
    assert_eq!(erase("ERASE C 5,-1 25,1"), vec![30.0]);
    // Right to left would be a crossing; W makes it a window: only B.
    assert_eq!(erase("ERASE W 25,1 5,-1"), vec![0.0, 30.0]);
    // Full words work too.
    assert_eq!(erase("ERASE crossing 25,1 5,-1"), vec![30.0]);
}

#[test]
fn fence_selects_what_it_crosses() {
    let mut s = drawing();
    // Enter ends the fence, a second Enter ends "Select objects".
    s.cmdline("ERASE F 6,-1 6,1 35,1 35,-1").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_some());
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    // Crosses A at x=6 and C at x=35, but not B.
    assert_eq!(left(&s), vec![12.0]);
    // Undo drops the last fence point.
    let mut s = drawing();
    s.cmdline("ERASE F 6,-1 6,1 35,1 U").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(left(&s), vec![12.0, 30.0]);
}

#[test]
fn window_and_crossing_polygons() {
    let mut s = drawing();
    s.cmdline("ERASE WP 5,-1 25,-1 25,1 5,1").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(left(&s), vec![0.0, 30.0]);
    let mut s = drawing();
    s.cmdline("ERASE CP 5,-1 25,-1 25,1 5,1").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(left(&s), vec![30.0]);
}

#[test]
fn escape_during_a_mode_cancels_the_command() {
    let mut s = drawing();
    s.cmdline("ERASE C").unwrap();
    assert_eq!(s.current_prompt().map(|p| p.message), Some("Specify first corner".to_string()));
    s.cmdline("5,-1").unwrap();
    assert_eq!(s.current_prompt().map(|p| p.message), Some("Specify opposite corner".to_string()));
    s.cancel();
    assert!(s.running.is_none());
    assert_eq!(left(&s), vec![0.0, 12.0, 30.0]);
    // The next command starts at a plain "Select objects".
    s.cmdline("ERASE 15,0").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(left(&s), vec![0.0, 30.0]);
}
