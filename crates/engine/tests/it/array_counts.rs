//! The counts the ARRAY commands ask for take a whole number as typed, as AutoCAD 2024's do
//! (measured in its core console at ARRAYRECT's rows and columns, ARRAYPOLAR's items and rows,
//! and -ARRAY's rows and items): +3 and 03 are three, and 5.0, 1e1, 8/2 and 3' are refused and
//! asked for again.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
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
