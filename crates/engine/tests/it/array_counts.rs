//! The counts the ARRAY commands ask for take a whole number as typed, as AutoCAD 2024's do
//! (measured in its core console at ARRAYRECT's rows and columns, ARRAYPOLAR's items and rows,
//! and -ARRAY's rows and items): +3 and 03 are three, and 5.0, 1e1, 8/2 and 3' are refused and
//! asked for again.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{ArrayKind, EntityKind, Handle};
use serde_json::json;

/// Texts AutoCAD 2024 refuses at an array count.
const REFUSED: [&str; 6] = ["5.0", "4.", "1e1", "8/2", "3'", "4.5"];

fn typed(s: &mut Session, inputs: &[&str]) {
    for t in inputs {
        s.cmdline(t).unwrap();
    }
}

/// Each refused text gets the count message, and the same prompt again.
fn refuses_and_asks_again(s: &mut Session) {
    let asked = s.prompt_text();
    for t in REFUSED {
        s.cmdline(t).unwrap();
        assert!(s.log.last().is_some_and(|l| l.starts_with("Requires an integer between 1 and")), "{t}: {:?}", s.log.last());
        assert_eq!(s.prompt_text(), asked, "{t}");
    }
}

fn lines(s: &Session) -> usize {
    s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count()
}

fn with_line() -> Session {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[10, 0], [12, 0]] })).unwrap();
    s
}

#[test]
fn arrayrect_columns_and_rows() {
    let mut s = with_line();
    typed(&mut s, &["arrayrect", "l", "", "cou"]);
    assert!(s.prompt_text().contains("Enter the number of columns"), "{}", s.prompt_text());
    refuses_and_asks_again(&mut s);
    s.cmdline("+3").unwrap();
    assert!(s.prompt_text().contains("Enter the number of rows"), "{}", s.prompt_text());
    refuses_and_asks_again(&mut s);
    typed(&mut s, &["03", "as", "n", "x"]);
    assert!(s.current_prompt().is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), 9);
}

/// The bounds are CADCraft's, which this rule leaves as they were: a count runs from 1 to what
/// MAXARRAY allows for the array's other counts. With MAXARRAY 100 and three rows, 0 and 34 columns
/// are refused and 33 taken; then 4 rows are refused and 3 taken.
#[test]
fn a_count_runs_from_one_to_the_maxarray_limit() {
    let mut s = with_line();
    s.execute("setvar", &json!({ "name": "MAXARRAY", "value": 100 })).unwrap();
    typed(&mut s, &["arrayrect", "l", "", "cou"]);
    let refused = |s: &mut Session, texts: &[&str], max: u64| {
        let asked = s.prompt_text();
        for t in texts {
            s.cmdline(t).unwrap();
            let msg = format!("Requires an integer between 1 and {max} ");
            assert!(s.log.last().is_some_and(|l| l.starts_with(&msg)), "{t}: {:?}", s.log.last());
            assert_eq!(s.prompt_text(), asked, "{t}");
        }
    };
    assert!(s.prompt_text().contains("Enter the number of columns"), "{}", s.prompt_text());
    refused(&mut s, &["0", "34"], 33);
    s.cmdline("33").unwrap();
    assert!(s.prompt_text().contains("Enter the number of rows"), "{}", s.prompt_text());
    refused(&mut s, &["0", "4"], 3);
    typed(&mut s, &["3", "as", "n", "x"]);
    assert!(s.current_prompt().is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), 99);
}

#[test]
fn arraypolar_items() {
    let mut s = with_line();
    typed(&mut s, &["arraypolar", "l", "", "0,0", "i"]);
    assert!(s.prompt_text().contains("Enter number of items in array"), "{}", s.prompt_text());
    refuses_and_asks_again(&mut s);
    typed(&mut s, &["+5", "as", "n", "x"]);
    assert!(s.current_prompt().is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), 5);
}

/// ARRAYEDIT's counts go through the same count parser as ARRAYRECT's, ARRAYPOLAR's and -ARRAY's
/// (`count()` in `crates/engine/src/cmd/array.rs`), so they refuse and take the same texts: here 5.0,
/// 5., 4.5, 1e1 and 8/2 are refused at a rectangular array's columns and rows and a polar array's
/// items, and +5, 05 and 5 are taken. No AutoCAD measurement of ARRAYEDIT is claimed.
#[test]
fn arrayedit_counts() {
    let refused = ["5.0", "5.", "4.5", "1e1", "8/2"];
    let edit = |s: &mut Session, h: Handle, option: &str, asked: &str, count: &str, after: &[&str]| {
        s.set_selection(vec![h]);
        typed(s, &["arrayedit", option]);
        assert!(s.prompt_text().contains(asked), "{}", s.prompt_text());
        let prompt = s.prompt_text();
        for t in refused {
            s.cmdline(t).unwrap();
            assert!(s.log.last().is_some_and(|l| l.starts_with("Requires an integer between 1 and")), "{t}: {:?}", s.log.last());
            assert_eq!(s.prompt_text(), prompt, "{t}");
        }
        s.cmdline(count).unwrap();
        typed(s, after);
        assert!(s.current_prompt().is_none(), "{}", s.prompt_text());
        let d = s.doc().unwrap();
        d.array_params(d.entity(h).unwrap()).unwrap().kind
    };
    let mut s = with_line();
    let l = s.doc().unwrap().model.iter().next().unwrap().handle.hex();
    let r = s.execute("arrayrect", &json!({ "handles": [l], "cols": 4, "rows": 3, "colSpacing": 5, "rowSpacing": 5 })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let ArrayKind::Rect(a) = edit(&mut s, h, "col", "Enter the number of columns", "+5", &["", "x"]) else { panic!("not rectangular") };
    assert_eq!((a.cols, a.rows), (5, 3));
    let ArrayKind::Rect(a) = edit(&mut s, h, "r", "Enter the number of rows", "05", &["", "", "x"]) else { panic!("not rectangular") };
    assert_eq!((a.cols, a.rows), (5, 5));

    let mut s = with_line();
    let l = s.doc().unwrap().model.iter().next().unwrap().handle.hex();
    let r = s.execute("arraypolar", &json!({ "handles": [l], "center": [0, -50], "count": 6 })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let ArrayKind::Polar(a) = edit(&mut s, h, "i", "Enter number of items in array", "5", &["x"]) else { panic!("not polar") };
    assert_eq!(a.items, 5);
}

#[test]
fn classic_array_rows_columns_and_items() {
    let mut s = with_line();
    typed(&mut s, &["-array", "l", "", "r"]);
    assert!(s.prompt_text().contains("Enter the number of rows (---)"), "{}", s.prompt_text());
    refuses_and_asks_again(&mut s);
    s.cmdline("03").unwrap();
    assert!(s.prompt_text().contains("Enter the number of columns (|||)"), "{}", s.prompt_text());
    refuses_and_asks_again(&mut s);
    typed(&mut s, &["+2", "5", "5"]);
    assert!(s.current_prompt().is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), 6);

    let mut s = with_line();
    typed(&mut s, &["-array", "l", "", "p", "0,0"]);
    assert!(s.prompt_text().contains("Enter the number of items in the array"), "{}", s.prompt_text());
    refuses_and_asks_again(&mut s);
    typed(&mut s, &["+3", "360", "y"]);
    assert!(s.current_prompt().is_none(), "{}", s.prompt_text());
    assert_eq!(lines(&s), 3);
}
