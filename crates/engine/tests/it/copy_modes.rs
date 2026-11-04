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
    // Multiple, then mOde with Enter: the mode shown (Single, COPYMODE 1) is the mode COPY runs in.
    let mut s = with_line(1);
    copy(&mut s, &["M", "O"]);
    assert_eq!(s.current_prompt().unwrap().display(), "Enter a copy mode option [Single/Multiple] <Single>:");
    for t in ["", "0,0", "5,0"] {
        s.cmdline(t).unwrap();
    }
    assert!(s.current_prompt().is_none(), "one copy");
    assert_eq!(starts(&s), [(0.0, 0.0), (5.0, 0.0)]);
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
    // A point from LISP is refused too (AutoCAD 2024 with '(1 2): the same message).
    for bad in ["1", "0", "2.5", "32768", "-3", "1'", "8/2", "x", "4.0", "1e1", "3\"", "(list 1 2)"] {
        let logged = s.log.len();
        s.cmdline(bad).unwrap();
        let said: Vec<&str> = s.log.iter().skip(logged).map(String::as_str).filter(|l| l.starts_with("Requires")).collect();
        assert_eq!(said, ["Requires an integer between 2 and 32767."], "{bad}");
        assert_eq!(s.current_prompt().unwrap().display(), "Enter number of items to array:", "{bad}");
    }
    s.cmdline("32767").unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Specify second point or [Fit]:");
    // A sign or a leading zero is still a whole number, and so is the LISP real 3.0 (AutoCAD 2024:
    // three items each).
    for three in ["+3", "03", "(+ 1.5 1.5)"] {
        let mut s = with_line(1);
        copy(&mut s, &["0,0", "A", three, "5,0"]);
        assert_eq!(starts(&s), [(0.0, 0.0), (5.0, 0.0), (10.0, 0.0)], "{three}");
    }
    // Enter at the number ends COPY with nothing copied.
    let mut s = with_line(1);
    copy(&mut s, &["0,0", "A", ""]);
    assert!(s.current_prompt().is_none());
    assert_eq!(starts(&s), [(0.0, 0.0)]);
}

#[test]
fn maxarray_doesnt_limit_the_items() {
    // AutoCAD 2024 with MAXARRAY 100000: four lines and 26000 items made 104000 lines (measured).
    // Here MAXARRAY is 100 (its least): two lines and 60 items, and 101 lines and 2 items, go past
    // it and are still made.
    let mut s = with_line(1);
    s.execute("line", &json!({ "points": [[0, 5], [1, 5]] })).unwrap();
    s.execute("setvar", &json!({ "name": "MAXARRAY", "value": 100 })).unwrap();
    copy(&mut s, &["0,0", "A", "60", "10,0"]);
    assert!(s.current_prompt().is_none());
    assert_eq!(starts(&s).len(), 120);
    let mut s = with_line(1);
    s.execute("setvar", &json!({ "name": "MAXARRAY", "value": 100 })).unwrap();
    for y in 1..=100 {
        s.execute("line", &json!({ "points": [[0, y], [1, y]] })).unwrap();
    }
    copy(&mut s, &["0,0", "A", "2", "10,0"]);
    assert!(s.current_prompt().is_none());
    assert_eq!(starts(&s).len(), 202);
}

#[test]
fn the_copies_stay_within_ten_million() {
    // Not AutoCAD's limit, ARRAY's: 400 lines times 25000 more items is exactly 10 million copies
    // and is taken; one item more is refused before anything is made.
    let mut s = with_line(1);
    for y in 1..400 {
        s.execute("line", &json!({ "points": [[0, y], [1, y]] })).unwrap();
    }
    copy(&mut s, &["0,0", "A", "25002"]);
    assert_eq!(
        s.log.last().map(String::as_str),
        Some("The array would create 10000400 objects; the limit is 10000000. Use fewer items or select fewer objects.")
    );
    assert_eq!(s.current_prompt().unwrap().display(), "Enter number of items to array:");
    assert_eq!(starts(&s).len(), 400);
    s.cmdline("25001").unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Specify second point or [Fit]:");
}

#[test]
fn copymode_is_a_profile_variable() {
    // OPTIONS reports it, a saved profile brings it back, reset puts it back to 0, and typing its
    // name asks for it as for any variable.
    let mut s = with_line(1);
    assert_eq!(s.execute("options", &json!({})).unwrap()["COPYMODE"], json!(1));
    let saved = cadcraft_engine::cmd::options::profile(&s);
    let mut t = Session::new();
    cadcraft_engine::cmd::options::restore(&mut t, &saved);
    assert_eq!(copymode(&mut t), json!(1));
    s.execute("options", &json!({ "reset": true })).unwrap();
    assert_eq!(copymode(&mut s), json!(0));
    s.cmdline("COPYMODE").unwrap();
    s.cmdline("1").unwrap();
    assert_eq!(copymode(&mut s), json!(1));
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
