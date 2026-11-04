//! Keywords typed at "Select objects" (#428): BOX, SIngle, AUto, Multiple, Group, Add, Remove and
//! Undo.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

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

/// ERASE with `input` typed at "Select objects", then Enter.
fn erase(input: &str) -> Vec<f64> {
    let mut s = drawing();
    s.cmdline(input).unwrap();
    assert!(s.running.is_some(), "{input}: still selecting");
    s.cmdline("").unwrap();
    assert!(s.running.is_none(), "{input}: Enter ends the selection");
    left(&s)
}

#[test]
fn box_follows_the_direction_of_its_corners() {
    // Left to right: a window, only B.
    assert_eq!(erase("ERASE BOX 5,-1 25,1"), vec![0.0, 30.0]);
    // Right to left: a crossing, A and B.
    assert_eq!(erase("ERASE box 25,1 5,-1"), vec![30.0]);
}

#[test]
fn single_ends_the_prompt_with_the_first_selection() {
    let mut s = drawing();
    s.cmdline("ERASE SI").unwrap();
    assert!(s.prompt_text().contains("Select objects"), "{}", s.prompt_text());
    // An empty spot starts a window; its other corner selects B and ends ERASE at once.
    s.cmdline("5,-1").unwrap();
    assert!(s.running.is_some());
    s.cmdline("25,1").unwrap();
    assert!(s.running.is_none(), "SIngle needs no Enter");
    assert_eq!(left(&s), vec![0.0, 30.0]);
    // A pick, with the full word.
    let mut s = drawing();
    s.cmdline("ERASE single 35,0").unwrap();
    assert!(s.running.is_none());
    assert_eq!(left(&s), vec![0.0, 12.0]);
    // SIngle is per prompt: the next ERASE waits for Enter again.
    s.cmdline("ERASE 5,0").unwrap();
    assert!(s.running.is_some());
}

#[test]
fn auto_and_multiple_keep_picking_until_enter() {
    assert_eq!(erase("ERASE AU 15,0 35,0"), vec![0.0]);
    assert_eq!(erase("ERASE M 15,0 35,0"), vec![0.0]);
    assert_eq!(erase("ERASE multiple auto 5,0"), vec![12.0, 30.0]);
}

#[test]
fn group_selects_a_group_by_name() {
    let mut s = drawing();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("group", &json!({ "name": "Ends", "handles": [hs[0], hs[2]] })).unwrap();
    s.cmdline("ERASE G").unwrap();
    let p = s.current_prompt().unwrap().display();
    assert!(p.contains("Enter group name"), "{p}");
    s.cmdline("ends").unwrap();
    assert!(s.prompt_text().contains("Select objects"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    assert_eq!(left(&s), vec![12.0]);
    // An unknown name says so and goes back to "Select objects".
    let mut s = drawing();
    s.cmdline("ERASE group").unwrap();
    s.cmdline("Nope").unwrap();
    assert!(s.log.iter().any(|l| l.contains("No group named \"Nope\"")), "{:?}", s.log);
    assert!(s.prompt_text().contains("Select objects"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(left(&s), vec![0.0, 12.0, 30.0]);
}

#[test]
fn remove_add_and_undo() {
    // Remove takes B out of ALL; Add picks again.
    assert_eq!(erase("ERASE ALL R 15,0 A 15,0"), Vec::<f64>::new());
    assert_eq!(erase("ERASE ALL R 15,0"), vec![12.0]);
    // Undo takes back the last pick…
    assert_eq!(erase("ERASE 5,0 15,0 U"), vec![12.0, 30.0]);
    // …or the last removal, and steps back further on each Undo.
    assert_eq!(erase("ERASE ALL R 15,0 U"), Vec::<f64>::new());
    assert_eq!(erase("ERASE 5,0 15,0 U U 35,0"), vec![0.0, 12.0]);
    // Undo with nothing selected says so and keeps prompting.
    let mut s = drawing();
    s.cmdline("ERASE U").unwrap();
    assert!(s.log.iter().any(|l| l == "Nothing to undo."), "{:?}", s.log);
    assert!(s.running.is_some());
}
