//! A malformed `handle` on a table edit command or TABLEEXPORT is an error. It never falls back
//! to the selected table (#934, follow-up to #330).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::json;

fn cell_text(s: &Session, h: &str) -> String {
    let h = Handle::parse_hex(h).unwrap();
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Table(t) => t.cells[0][0].text.clone(),
        _ => String::new(),
    }
}

#[test]
fn malformed_table_handle_does_not_edit_the_selected_table() {
    let mut s = Session::new();
    let h = s.execute("table", &json!({ "at": [0, 0], "cells": [["a", "b"]], "rowHeight": 1.0, "colWidth": 3.0 })).unwrap()["handle"]
        .as_str()
        .unwrap()
        .to_string();
    s.set_selection(vec![Handle::parse_hex(&h).unwrap()]);

    for bad in [json!("zzz"), json!(""), json!("#1"), json!(-1), json!(true), json!([1])] {
        let e = s.execute("table.set", &json!({ "handle": bad, "row": 0, "col": 0, "text": "X" })).expect_err(&format!("handle {bad}"));
        assert!(e.to_string().contains("is not an object handle"), "handle {bad}: {e}");
        assert_eq!(cell_text(&s, &h), "a", "handle {bad} must change nothing");
        // TABLEEXPORT resolves its table the same way: no CSV of the selected table either.
        let e = s.execute("tableexport", &json!({ "handle": bad })).expect_err(&format!("tableexport handle {bad}"));
        assert!(e.to_string().contains("is not an object handle"), "tableexport handle {bad}: {e}");
    }

    // An explicit valid handle and an omitted handle (selection) still work.
    s.execute("table.set", &json!({ "handle": h, "row": 0, "col": 0, "text": "Y" })).unwrap();
    assert_eq!(cell_text(&s, &h), "Y");
    s.execute("table.set", &json!({ "row": 0, "col": 0, "text": "Z" })).unwrap();
    assert_eq!(cell_text(&s, &h), "Z");
    assert_eq!(s.execute("tableexport", &json!({})).unwrap()["csv"], "Z,b\r\n");
}
