//! COPY at the command line as AutoCAD 2024 runs it (accoreconsole, a line from (0, 0) to (1, 0)
//! copied with `(command "_.COPY" line "" ...)`, then the start point of every line):
//!
//! - COPYMODE 1 makes one copy and ends; Multiple copies until Exit or Enter, and Undo takes back
//!   the last placement, a whole array at once;
//! - Enter at the first second point copies by the base point taken as a displacement;
//! - Array asks for a number of items, the original included (2 to 32767), and puts them the
//!   second point apart, or spread up to it with Fit; Enter there leaves them at the base.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

/// A session with one line from (0, 0) to (1, 0) and COPYMODE set to `mode`.
fn with_line(mode: i64) -> Session {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [1, 0]] })).unwrap();
    s.execute("setvar", &json!({ "name": "COPYMODE", "value": mode })).unwrap();
    s
}

/// The start point of every line, sorted.
fn starts(s: &Session) -> Vec<(f64, f64)> {
    let mut v: Vec<(f64, f64)> =
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some((l.a.x, l.a.y)) } else { None }).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

fn copymode(s: &mut Session) -> serde_json::Value {
    s.execute("getvar", &json!({ "name": "COPYMODE" })).unwrap()["value"].clone()
}

/// Types COPY, selects everything, then each of `inputs`.
fn copy(s: &mut Session, inputs: &[&str]) {
    for t in ["COPY", "all", ""].iter().chain(inputs) {
        s.cmdline(t).unwrap();
    }
}

#[test]
fn copymode_1_makes_one_copy_and_ends() {
    let mut s = with_line(1);
    copy(&mut s, &["0,0"]);
    assert_eq!(s.current_prompt().unwrap().display(), "Specify second point or [Array] <use first point as displacement>:");
    s.cmdline("10,0").unwrap();
    assert!(s.current_prompt().is_none(), "COPY ended");
    assert_eq!(starts(&s), [(0.0, 0.0), (10.0, 0.0)]);
    assert_eq!(copymode(&mut s), json!(1));
}

#[test]
fn the_base_point_prompt_offers_mode_and_multiple() {
    let mut s = with_line(1);
    copy(&mut s, &[]);
    assert_eq!(s.current_prompt().unwrap().display(), "Specify base point or [Displacement/mOde/Multiple] <Displacement>:");
    // Multiple, this once: the option goes and COPYMODE stays 1.
    s.cmdline("M").unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Specify base point or [Displacement/mOde] <Displacement>:");
    for t in ["0,0", "10,0", "20,0"] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(s.current_prompt().unwrap().display(), "Specify second point or [Array/Exit/Undo] <Exit>:");
    s.cmdline("E").unwrap();
    assert!(s.current_prompt().is_none());
    assert_eq!(starts(&s), [(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]);
    assert_eq!(copymode(&mut s), json!(1));
    // mOde sets COPYMODE.
    let mut s = with_line(0);
    copy(&mut s, &["O"]);
    assert_eq!(s.current_prompt().unwrap().display(), "Enter a copy mode option [Single/Multiple] <Multiple>:");
    s.cmdline("S").unwrap();
    assert_eq!(copymode(&mut s), json!(1));
    for t in ["0,0", "5,0"] {
        s.cmdline(t).unwrap();
    }
    assert!(s.current_prompt().is_none(), "one copy, now single");
    assert_eq!(starts(&s), [(0.0, 0.0), (5.0, 0.0)]);
}

#[test]
fn copymode_0_copies_until_enter_and_undo_takes_back_the_last() {
    let mut s = with_line(0);
    copy(&mut s, &["0,0", "10,0", "20,0", "U", "30,0", ""]);
    assert!(s.current_prompt().is_none());
    assert_eq!(starts(&s), [(0.0, 0.0), (10.0, 0.0), (30.0, 0.0)]);
    // Undoing the only copy goes back to the first prompt.
    let mut s = with_line(0);
    copy(&mut s, &["0,0", "10,0", "U"]);
    assert!(s.log.iter().any(|l| l == "Command has been completely undone."), "{:?}", s.log);
    assert_eq!(s.current_prompt().unwrap().display(), "Specify second point or [Array] <use first point as displacement>:");
    s.cmdline("30,0").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(starts(&s), [(0.0, 0.0), (30.0, 0.0)]);
    // An array is one placement: Undo takes all its items back.
    let mut s = with_line(0);
    copy(&mut s, &["0,0", "10,0", "A", "3", "0,5", "U", "40,0", ""]);
    assert_eq!(starts(&s), [(0.0, 0.0), (10.0, 0.0), (40.0, 0.0)]);
    // An array after a copy, and more after it.
    let mut s = with_line(0);
    copy(&mut s, &["3,4", "10,4", "A", "2", "20,4", ""]);
    assert_eq!(starts(&s), [(0.0, 0.0), (7.0, 0.0), (17.0, 0.0)]);
}

#[test]
fn enter_at_the_first_second_point_copies_by_the_base_point() {
    for mode in [0, 1] {
        let mut s = with_line(mode);
        copy(&mut s, &["3,4", ""]);
        assert!(s.current_prompt().is_none(), "COPYMODE {mode}");
        assert_eq!(starts(&s), [(0.0, 0.0), (3.0, 4.0)], "COPYMODE {mode}");
    }
    // Displacement, and Enter at the base point (its default), make one copy too.
    for inputs in [["D", "5,6"], ["", "7,8"]] {
        let mut s = with_line(0);
        copy(&mut s, &inputs);
        assert!(s.current_prompt().is_none(), "{inputs:?}");
        assert_eq!(starts(&s).len(), 2, "{inputs:?}");
    }
}

#[test]
fn array_puts_the_items_a_second_point_apart_or_fits_them() {
    // (inputs after the base point (0, 0), AutoCAD's lines)
    let cases: [(&[&str], &[(f64, f64)]); 5] = [
        (&["A", "4", "10,5"], &[(0.0, 0.0), (10.0, 5.0), (20.0, 10.0), (30.0, 15.0)]),
        (&["A", "4", "F", "30,0"], &[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0), (30.0, 0.0)]),
        // Fit, then Array again: back to the spacing.
        (&["A", "3", "F", "A", "10,0"], &[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]),
        // The base point again, or Enter: every item at the base.
        (&["A", "3", "0,0"], &[(0.0, 0.0); 3]),
        (&["A", "3", ""], &[(0.0, 0.0); 3]),
    ];
    for (inputs, want) in cases {
        let mut s = with_line(1);
        copy(&mut s, &["0,0"]);
        for t in inputs {
            s.cmdline(t).unwrap();
        }
        assert!(s.current_prompt().is_none(), "{inputs:?}");
        assert_eq!(starts(&s), want, "{inputs:?}");
    }
    // Enter at the array's point with the base at (3, 4): still on the original, the base
    // isn't taken as a displacement there.
    let mut s = with_line(1);
    copy(&mut s, &["3,4", "A", "3", ""]);
    assert_eq!(starts(&s), [(0.0, 0.0); 3]);
    let mut s = with_line(1);
    copy(&mut s, &["0,0", "A", "4"]);
    assert_eq!(s.current_prompt().unwrap().display(), "Specify second point or [Fit]:");
    s.cmdline("F").unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Specify second point or [Array]:");
}

#[test]
fn array_takes_2_to_32767_items() {
    let mut s = with_line(1);
    copy(&mut s, &["0,0", "A"]);
    assert_eq!(s.current_prompt().unwrap().display(), "Enter number of items to array:");
    for bad in ["1", "0", "2.5", "32768", "-3"] {
        s.cmdline(bad).unwrap();
        assert_eq!(s.log.last().map(String::as_str), Some("Requires an integer between 2 and 32767."), "{bad}");
        assert_eq!(s.current_prompt().unwrap().display(), "Enter number of items to array:", "{bad}");
    }
    s.cmdline("32767").unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Specify second point or [Fit]:");
    // Enter at the number ends COPY with nothing copied.
    let mut s = with_line(1);
    copy(&mut s, &["0,0", "A", ""]);
    assert!(s.current_prompt().is_none());
    assert_eq!(starts(&s), [(0.0, 0.0)]);
}

#[test]
fn maxarray_caps_the_items_times_the_objects() {
    // Two lines and MAXARRAY 100 (its least): 50 copies of each at most, so 51 items.
    let mut s = with_line(1);
    s.execute("line", &json!({ "points": [[0, 5], [1, 5]] })).unwrap();
    s.execute("setvar", &json!({ "name": "MAXARRAY", "value": 100 })).unwrap();
    copy(&mut s, &["0,0", "A", "52"]);
    assert_eq!(s.log.last().map(String::as_str), Some("Requires an integer between 2 and 51 (MAXARRAY limits the size of the array)."));
    for t in ["51", "10,0"] {
        s.cmdline(t).unwrap();
    }
    assert!(s.current_prompt().is_none());
    assert_eq!(starts(&s).len(), 102);
    // More objects than MAXARRAY: no array at all.
    let mut s = with_line(1);
    s.execute("setvar", &json!({ "name": "MAXARRAY", "value": 100 })).unwrap();
    for y in 1..=100 {
        s.execute("line", &json!({ "points": [[0, y], [1, y]] })).unwrap();
    }
    copy(&mut s, &["0,0", "A", "2"]);
    assert_eq!(s.log.last().map(String::as_str), Some("The selection is too large to array (MAXARRAY limits the size of the array)."));
}

#[test]
fn copymode_takes_0_or_1() {
    let mut s = Session::new();
    assert_eq!(copymode(&mut s), json!(0));
    for bad in [json!(2), json!(-1), json!("x")] {
        assert!(s.execute("setvar", &json!({ "name": "COPYMODE", "value": bad })).is_err(), "{bad}");
    }
    assert_eq!(copymode(&mut s), json!(0));
}
