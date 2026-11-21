//! Table formulas (#534): `=Sum(A2:A4)`, Average, Count, cell references and arithmetic;
//! recalculated on edit, row/column insertion and REGEN; cycles and bad input show an error text
//! and never panic. TABLEDIT and TABLEEXPORT.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Table};
use serde_json::{Value, json};

/// A 2-column table without title/header: A1..A3 = 1, 2, 3; B column empty.
fn session() -> (Session, String) {
    let mut s = Session::new();
    let r = s
        .execute("table", &json!({ "at": [0, 0], "cells": [["1", ""], ["2", ""], ["3", ""], ["", ""]], "rowHeight": 1.0, "colWidth": 3.0 }))
        .unwrap();
    let h = r["handle"].as_str().unwrap().to_string();
    (s, h)
}

fn table(s: &Session, h: &str) -> Table {
    let h = cadcraft_engine::doc::Handle::parse_hex(h).unwrap();
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Table(t) => t.clone(),
        _ => panic!("not a table"),
    }
}

fn text(s: &Session, h: &str, r: usize, c: usize) -> String {
    table(s, h).cells[r][c].text.clone()
}

fn set(s: &mut Session, h: &str, r: usize, c: usize, text: &str) {
    s.execute("table.set", &json!({ "handle": h, "row": r, "col": c, "text": text })).unwrap();
}

#[test]
fn sum_average_count_and_arithmetic() {
    let (mut s, h) = session();
    set(&mut s, &h, 3, 0, "=Sum(A1:A3)");
    assert_eq!(text(&s, &h, 3, 0), "6");
    assert_eq!(table(&s, &h).cells[3][0].formula.as_deref(), Some("=Sum(A1:A3)"));
    set(&mut s, &h, 0, 1, "=Average(A1:A3)");
    set(&mut s, &h, 1, 1, "=count(a1:a3, B1)");
    set(&mut s, &h, 2, 1, "=(A1+A2)*A3^2/-2");
    assert_eq!(text(&s, &h, 0, 1), "2");
    assert_eq!(text(&s, &h, 1, 1), "4");
    assert_eq!(text(&s, &h, 2, 1), "-13.5");
    // Editing a referenced cell recalculates everything that depends on it.
    set(&mut s, &h, 0, 0, "10");
    assert_eq!(text(&s, &h, 3, 0), "15");
    assert_eq!(text(&s, &h, 1, 1), "4");
    // Plain text replaces the formula.
    set(&mut s, &h, 3, 0, "total");
    assert_eq!(table(&s, &h).cells[3][0].formula, None);
}

#[test]
fn errors_show_as_text() {
    let (mut s, h) = session();
    // A cycle: B1 -> B2 -> B1.
    set(&mut s, &h, 0, 1, "=B2+1");
    set(&mut s, &h, 1, 1, "=B1*2");
    assert_eq!(text(&s, &h, 0, 1), "#CYCLE");
    assert_eq!(text(&s, &h, 1, 1), "#CYCLE");
    set(&mut s, &h, 2, 1, "=A1/(A2-2)");
    assert_eq!(text(&s, &h, 2, 1), "#DIV/0");
    set(&mut s, &h, 3, 1, "=Z99");
    assert_eq!(text(&s, &h, 3, 1), "#REF");
    // A formula that can't be read is refused at once.
    assert!(s.execute("table.set", &json!({ "handle": h, "row": 0, "col": 0, "text": "=Sum(A1:" })).is_err());
}

#[test]
fn inserting_and_deleting_rows_moves_references() {
    let (mut s, h) = session();
    set(&mut s, &h, 3, 0, "=Sum(A1:A3)");
    s.execute("table.insertrow", &json!({ "handle": h, "row": 1 })).unwrap();
    set(&mut s, &h, 1, 0, "4");
    let t = table(&s, &h);
    assert_eq!(t.cells[4][0].formula.as_deref(), Some("=Sum(A1:A4)"));
    assert_eq!(t.cells[4][0].text, "10");
    s.execute("table.deleterow", &json!({ "handle": h, "row": 0 })).unwrap();
    let t = table(&s, &h);
    assert_eq!(t.cells[3][0].formula.as_deref(), Some("=Sum(A1:A3)"));
    assert_eq!(t.cells[3][0].text, "9");
}

#[test]
fn insert_formula_options() {
    let mut s = Session::new();
    let r = s.execute("table", &json!({ "at": [0, 0], "title": "T", "header": ["Qty"], "cells": [["2"], ["5"], [""]] })).unwrap();
    let h = r["handle"].as_str().unwrap().to_string();
    // Default range: the data cells above.
    let r = s.execute("table.formula", &json!({ "handle": h, "row": 4, "col": 0, "function": "sum" })).unwrap();
    assert_eq!(r["formula"], "=Sum(A3:A4)");
    assert_eq!(text(&s, &h, 4, 0), "7");
    s.execute("table.formula", &json!({ "handle": h, "row": 4, "col": 0, "function": "equation", "expression": "A3*A4" })).unwrap();
    assert_eq!(text(&s, &h, 4, 0), "10");
    s.execute("table.formula", &json!({ "handle": h, "row": 4, "col": 0, "function": "cell", "cell": "A4" })).unwrap();
    assert_eq!(text(&s, &h, 4, 0), "5");
    assert!(s.execute("table.formula", &json!({ "handle": h, "row": 4, "col": 0, "function": "median" })).is_err());
}

#[test]
fn regen_recalculates() {
    let (mut s, h) = session();
    set(&mut s, &h, 3, 0, "=Sum(A1:A3)");
    // Make the shown value stale, as a foreign file could.
    let hh = cadcraft_engine::doc::Handle::parse_hex(&h).unwrap();
    s.doc_mut()
        .unwrap()
        .modify_entity(hh, |e| {
            if let EntityKind::Table(t) = &mut e.kind {
                t.cells[3][0].text = "999".into();
            }
        })
        .unwrap();
    s.execute("regen", &Value::Null).unwrap();
    assert_eq!(text(&s, &h, 3, 0), "6");
    assert_eq!(s.execute("table.recalc", &json!({})).unwrap()["changed"], 0);
}

#[test]
fn tabledit_at_the_command_line() {
    let (mut s, h) = session();
    // Cell A4 spans y -3..-4.
    s.cmdline("tabledit").unwrap();
    s.cmdline("1,-3.5").unwrap();
    s.cmdline("=Sum(A1:A3)").unwrap();
    assert_eq!(text(&s, &h, 3, 0), "6");
    // JSON form with a point.
    s.execute("tabledit", &json!({ "at": [4, -0.5], "text": "hello" })).unwrap();
    assert_eq!(text(&s, &h, 0, 1), "hello");
}

#[test]
fn tableexport_writes_csv() {
    let (mut s, h) = session();
    set(&mut s, &h, 3, 0, "=Sum(A1:A3)");
    set(&mut s, &h, 0, 1, "a, \"b\"");
    let r = s.execute("tableexport", &json!({ "handle": h })).unwrap();
    assert_eq!(r["csv"], "1,\"a, \"\"b\"\"\"\r\n2,\r\n3,\r\n6,\r\n");
    let dir = std::env::temp_dir().join(format!("cadcraft-tableexport-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.csv");
    s.execute("tableexport", &json!({ "handle": h, "path": path.to_str().unwrap(), "separator": "tab" })).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "1\t\"a, \"\"b\"\"\"\r\n2\t\r\n3\t\r\n6\t\r\n");
    // At the command line: select by point, then the file name.
    let path2 = dir.join("u.csv");
    s.cmdline("tableexport").unwrap();
    s.cmdline("1,-0.5").unwrap();
    s.cmdline(path2.to_str().unwrap()).unwrap();
    assert!(std::fs::read_to_string(&path2).unwrap().starts_with("1,"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_formulas_never_panic() {
    let (mut s, h) = session();
    let long = format!("={}", "A1+".repeat(2000));
    let deep = format!("={}1{}", "(".repeat(5000), ")".repeat(5000));
    let huge = "=Sum(A1:ZZZ9999999)".to_string();
    for f in [
        long.as_str(),
        deep.as_str(),
        huge.as_str(),
        "=",
        "==1",
        "=1e999",
        "=Round(1,1e300)",
        "=A1:B2",
        "=#REF+1",
        "=Sum()",
        "=1/0",
        "=-(-(-1))",
        "=Abs(-2,3)",
    ] {
        let _ = s.execute("table.set", &json!({ "handle": h, "row": 3, "col": 1, "text": f }));
        let _ = s.execute("table.formula", &json!({ "handle": h, "row": 3, "col": 1, "function": "equation", "expression": f }));
    }
    // A long chain of formula cells (each adds one to the cell above) evaluates without recursion.
    let n = 1500;
    let r = s.execute("table", &json!({ "at": [100, 0], "cols": 1, "rows": n })).unwrap();
    let h2 = r["handle"].as_str().unwrap().to_string();
    set(&mut s, &h2, 0, 0, "1");
    for i in (1..n).rev() {
        set(&mut s, &h2, i, 0, &format!("=A{i}+1"));
    }
    assert_eq!(text(&s, &h2, n - 1, 0), n.to_string());
}
