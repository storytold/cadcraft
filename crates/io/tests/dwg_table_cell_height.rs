//! TABLE heights inherit from the table style unless the file records a cell override.
#![cfg(not(target_arch = "wasm32"))]

use cadcraft_doc::{Common, Drawing, EntityKind, Space, Table, TableCell, TableStyle};

fn drawing() -> Drawing {
    let mut d = Drawing::new_imperial();
    d.table_styles.push(TableStyle { name: "Small".into(), text_height: 0.0625, title: false, header: false, ..TableStyle::default() });
    let table = Table {
        style: "Small".into(),
        text_height: 0.0625,
        title: false,
        header: false,
        row_heights: vec![0.2],
        col_widths: vec![2.0; 3],
        cells: vec![vec![
            TableCell { text: "plain".into(), ..TableCell::default() },
            TableCell { text: "{\\C256;formatted}".into(), ..TableCell::default() },
            TableCell { text: "explicit".into(), text_height: Some(0.125), ..TableCell::default() },
        ]],
        ..Table::default()
    };
    d.add(&Space::Model, Common::default(), EntityKind::Table(table)).unwrap();
    d
}

fn check(d: &Drawing) {
    let t = d
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(t.style, "Small");
    assert_eq!(t.text_height, 0.0625);
    for cell in &t.cells[0][..2] {
        assert!(cell.text_height.is_none() || cell.text_height == Some(0.0625), "unexpected override: {:?}", cell.text_height);
    }
    assert_eq!(t.cells[0][2].text_height, Some(0.125));
}

#[test]
fn table_cell_heights_keep_inheritance_through_dwg() {
    let bytes = cadcraft_io::write(&drawing(), "height.dwg").unwrap();
    // A style record can be present for metadata without overriding its text height.
    // Write that synthetic representation directly so no earlier DXF conversion can
    // turn the unused default into an explicit override before this import.
    let mut doc = acadrust::DwgReader::from_stream(std::io::Cursor::new(bytes)).read().unwrap();
    let table = doc
        .entities_mut()
        .find_map(|e| match e {
            acadrust::entities::EntityType::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    table.rows[0].cells[0].style = Some(acadrust::entities::table::CellStyle::new());
    let bytes = acadrust::DwgWriter::write_to_vec(&doc).unwrap();
    check(&cadcraft_io::read(&bytes, "height.dwg").unwrap());
}

#[test]
fn table_cell_heights_keep_inheritance_through_dxf() {
    let bytes = cadcraft_io::write(&drawing(), "height.dxf").unwrap();
    check(&cadcraft_io::read(&bytes, "height.dxf").unwrap());
}
