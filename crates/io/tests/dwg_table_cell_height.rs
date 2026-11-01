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
        assert!(cell.text_height.is_none(), "unexpected override: {:?}", cell.text_height);
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

// ---- cells with their own style record (review on #2588) ----

use acadrust::entities::table::{CellContent, CellStyle, CellStylePropertyFlags};

/// Write the drawing as a DWG, edit its first cell through acadrust (`edit`), write it again as a
/// DWG (or, with `as_dxf`, as a DXF by acadrust's writer) and read it with CADCraft. Returns the first cell's
/// text height override.
fn first_cell_height(as_dxf: bool, edit: impl FnOnce(&mut acadrust::entities::table::TableCell)) -> Option<f64> {
    let bytes = cadcraft_io::write(&drawing(), "height.dwg").unwrap();
    let mut doc = acadrust::DwgReader::from_stream(std::io::Cursor::new(bytes)).read().unwrap();
    let table = doc
        .entities_mut()
        .find_map(|e| match e {
            acadrust::entities::EntityType::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    edit(&mut table.rows[0].cells[0]);
    let d = if as_dxf {
        let bytes = acadrust::DxfWriter::new(&doc).write_to_vec().unwrap();
        cadcraft_io::read(&bytes, "height.dxf").unwrap()
    } else {
        let bytes = acadrust::DwgWriter::write_to_vec(&doc).unwrap();
        cadcraft_io::read(&bytes, "height.dwg").unwrap()
    };
    d.model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Table(t) => Some(t.cells[0][0].text_height),
            _ => None,
        })
        .unwrap()
}

fn styled(height: f64, flags: CellStylePropertyFlags) -> CellStyle {
    let mut s = CellStyle::new();
    s.text_height = height;
    s.property_flags = flags;
    s
}

#[test]
fn a_cell_style_with_the_text_height_flag_keeps_its_height() {
    let h = first_cell_height(false, |c| c.style = Some(styled(0.5, CellStylePropertyFlags::TEXT_HEIGHT)));
    assert_eq!(h, Some(0.5));
}

#[test]
fn content_with_format_override_bit_128_keeps_the_style_height() {
    let h = first_cell_height(false, |c| {
        c.style = Some(styled(0.5, CellStylePropertyFlags::NONE));
        if c.contents.is_empty() {
            c.contents.push(CellContent::new());
        }
        c.contents[0].format_override_flags |= 128;
    });
    assert_eq!(h, Some(0.5));
}

#[test]
fn a_cell_with_no_style_record_inherits_the_table_style() {
    assert_eq!(first_cell_height(false, |c| c.style = None), None);
}

/// A style record with a height and neither override is not an override, through DWG.
#[test]
fn a_style_height_without_an_override_is_cleared() {
    assert_eq!(first_cell_height(false, |c| c.style = Some(styled(0.18, CellStylePropertyFlags::NONE))), None);
}
