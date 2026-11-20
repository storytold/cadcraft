//! Editing a table larger than the TABLE command creates (an imported one) keeps all its rows
//! and columns (#915).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle, Table, TableCell};
use serde_json::json;

fn grid(rows: usize, cols: usize) -> Table {
    let mut cells = vec![vec![TableCell::default(); cols]; rows];
    cells[rows - 1][cols - 1].text = "last".into();
    Table { col_widths: vec![2.5; cols], row_heights: vec![0.27; rows], cells, ..Table::default() }
}

fn table(s: &Session, h: Handle) -> Table {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Table(t) => t.clone(),
        _ => panic!("not a table"),
    }
}

#[test]
fn editing_a_large_imported_table_keeps_every_row_and_column() {
    for (rows, cols) in [(2001, 1), (1, 201)] {
        let mut s = Session::new();
        let h = s.add_entity(EntityKind::Table(grid(rows, cols))).unwrap();
        s.execute("table.set", &json!({ "handle": h.hex(), "row": 0, "col": 0, "text": "first" })).unwrap();
        let t = table(&s, h);
        assert_eq!((t.row_heights.len(), t.col_widths.len(), t.cells.len()), (rows, cols, rows), "{rows}×{cols}");
        assert_eq!(t.cells[rows - 1].len(), cols);
        assert_eq!((t.cells[0][0].text.as_str(), t.cells[rows - 1][cols - 1].text.as_str()), ("first", "last"));
        s.undo().unwrap();
        assert_eq!(table(&s, h), grid(rows, cols));
        s.redo().unwrap();
        assert_eq!(table(&s, h), t);
    }
}

#[test]
fn a_table_past_the_cell_budget_is_refused_unchanged() {
    // Sizes without cells: refusing must not build the 2000 × 501 grid first.
    let big = Table { col_widths: vec![2.5; 501], row_heights: vec![0.27; 2000], ..Table::default() };
    let mut s = Session::new();
    let h = s.add_entity(EntityKind::Table(big.clone())).unwrap();
    let e = s.execute("table.set", &json!({ "handle": h.hex(), "row": 0, "col": 0, "text": "x" })).unwrap_err();
    assert!(e.to_string().contains("too large to edit"), "{e}");
    assert_eq!(table(&s, h), big);
}
