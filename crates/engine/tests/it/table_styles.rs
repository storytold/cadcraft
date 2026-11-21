//! Table cell styles (#534): Title/Header/Data and custom cell styles in TABLESTYLE (JSON and
//! typed), applied to rows and cells, and drawn with their fill, borders, alignment and number
//! format.

use cadcraft_engine::Session;
use cadcraft_engine::doc::color::Color;
use cadcraft_engine::doc::{CellFormat, EntityKind, Handle, Table, TableStyle};
use cadcraft_engine::render;
use serde_json::json;

fn style(s: &Session, name: &str) -> TableStyle {
    s.doc().unwrap().table_styles.iter().find(|t| t.name == name).cloned().unwrap()
}

fn table(s: &Session, h: &str) -> Table {
    match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
        EntityKind::Table(t) => t.clone(),
        _ => panic!("not a table"),
    }
}

fn new_table(s: &mut Session) -> String {
    let r = s
        .execute("table", &json!({ "at": [0, 0], "title": "Schedule", "header": ["Item", "Cost"], "cells": [["a", "1"], ["b", "2.5"], ["Total", ""]], "colWidth": 4.0 }))
        .unwrap();
    r["handle"].as_str().unwrap().to_string()
}

#[test]
fn define_cell_styles_with_json() {
    let mut s = Session::new();
    s.execute("tablestyle", &json!({ "name": "Costs", "cellStyle": { "name": "Header", "fill": "yellow", "textColor": 5 } })).unwrap();
    let r = s
        .execute(
            "tablestyle.cellstyle",
            &json!({ "style": "Costs", "name": "Totals", "align": "MR", "format": "decimal", "precision": 2, "margin": 0.1,
                     "borders": { "all": { "lineweight": 0.5 }, "top": { "color": "red" }, "inside": false } }),
        )
        .unwrap();
    let names: Vec<&str> = r["cellStyles"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Title", "Header", "Data", "Totals"]);
    let st = style(&s, "Costs");
    let h = st.cell_style("Header");
    assert_eq!((h.fill, h.text_color, h.align), (Some(Color::Index(2)), Color::Index(5), 5));
    let t = st.cell_style("totals");
    assert_eq!((t.align, t.format, t.precision, t.margin_h), (6, CellFormat::Decimal, 2, 0.1));
    assert_eq!(t.borders.top.color, Color::Index(1));
    assert!(!t.borders.inside_v.visible && t.borders.left.visible);
    assert_eq!(t.borders.left.lineweight, cadcraft_engine::doc::Lineweight::Mm100(50));
    // Bad values are refused, never stored.
    assert!(s.execute("tablestyle.cellstyle", &json!({ "style": "Costs", "name": "Totals", "align": "XX" })).is_err());
    assert!(s.execute("tablestyle.cellstyle", &json!({ "style": "Costs", "name": "Totals", "textHeight": -1 })).is_err());
    assert!(s.execute("tablestyle.cellstyle", &json!({ "style": "Nope", "name": "Totals" })).is_err());
    // Delete a custom style; deleting a built-in one resets it.
    s.execute("tablestyle.cellstyle", &json!({ "style": "Costs", "name": "Totals", "delete": true })).unwrap();
    s.execute("tablestyle.cellstyle", &json!({ "style": "Costs", "name": "Header", "delete": true })).unwrap();
    assert!(style(&s, "Costs").cell_styles.is_empty());
}

#[test]
fn rows_and_cells_take_cell_styles() {
    let mut s = Session::new();
    s.execute("tablestyle.cellstyle", &json!({ "name": "Totals", "fill": "green", "format": "decimal", "precision": 2, "align": "MR" })).unwrap();
    let h = new_table(&mut s);
    // Row 4 (Total) gets the custom style; B5 sums the costs.
    s.execute("table.cellstyle", &json!({ "handle": h, "row": 4, "style": "Totals" })).unwrap();
    s.execute("table.set", &json!({ "handle": h, "row": 4, "col": 1, "text": "=Sum(B3:B4)" })).unwrap();
    let t = table(&s, &h);
    assert_eq!(t.row_style(4), "Totals");
    assert_eq!(t.cells[4][1].text, "3.50");
    // A single cell back to Data: the result is formatted the Data way.
    s.execute("table.cellstyle", &json!({ "handle": h, "row": 4, "col": 1, "style": "Data" })).unwrap();
    assert_eq!(table(&s, &h).cells[4][1].text, "3.5");
    assert!(s.execute("table.cellstyle", &json!({ "handle": h, "row": 4, "style": "Missing" })).is_err());
    assert!(s.execute("table.cellstyle", &json!({ "handle": h, "row": 99, "style": "Data" })).is_err());
    // Changing the cell style reformats the tables using it.
    s.execute("tablestyle.cellstyle", &json!({ "name": "Data", "format": "percent", "precision": 0 })).unwrap();
    assert_eq!(table(&s, &h).cells[4][1].text, "350%");
}

#[test]
fn styles_are_drawn() {
    let mut s = Session::new();
    s.execute("tablestyle.cellstyle", &json!({ "name": "Header", "fill": "red", "borders": { "inside": false } })).unwrap();
    s.execute("tablestyle.cellstyle", &json!({ "name": "Data", "align": "TR" })).unwrap();
    let h = new_table(&mut s);
    let t = table(&s, &h);
    let g = render::table_geometry(s.doc().unwrap(), &t);
    // One fill per header cell.
    assert_eq!(g.fills.len(), 2);
    assert!(g.fills.iter().all(|f| f.color == Color::Index(1)));
    // No vertical line between the two header cells.
    let y_header = (-t.row_heights[0] - t.row_heights[1], -t.row_heights[0]);
    let inner = g.lines.iter().any(|l| {
        (l.a.x - 4.0).abs() < 1e-9
            && (l.b.x - 4.0).abs() < 1e-9
            && l.a.y.min(l.b.y) < y_header.1 - 1e-9
            && l.a.y.max(l.b.y) > y_header.0 + 1e-9
            && l.a.y.max(l.b.y) <= y_header.1 + 1e-9
    });
    assert!(!inner);
    // Data text sits top right inside the margin.
    let a = g.texts.iter().find(|x| x.text == "1").unwrap();
    assert_eq!(a.attach, 3);
    let margin = style(&s, "Standard").margin;
    assert!((a.at.x - (8.0 - margin)).abs() < 1e-9);
    // The display list has the fill triangles.
    let list = render::build(s.doc().unwrap(), &cadcraft_engine::doc::Space::Model, &render::Options::default());
    assert!(list.prims.iter().any(|p| p.kind == render::Kind::Tris));
}

#[test]
fn typed_cell_style() {
    let mut s = Session::new();
    s.cmdline("tablestyle.cellstyle").unwrap();
    s.cmdline("Notes").unwrap();
    s.cmdline("h").unwrap();
    s.cmdline("0.3").unwrap();
    s.cmdline("f").unwrap();
    s.cmdline("cyan").unwrap();
    s.cmdline("a").unwrap();
    s.cmdline("BC").unwrap();
    s.cmdline("fo").unwrap();
    s.cmdline("percent").unwrap();
    s.cmdline("x").unwrap();
    let c = style(&s, "Standard").cell_style("Notes");
    assert_eq!((c.text_height, c.fill, c.align, c.format), (0.3, Some(Color::Index(4)), 8, CellFormat::Percent));
    assert_eq!(c.name, "Notes");
}
