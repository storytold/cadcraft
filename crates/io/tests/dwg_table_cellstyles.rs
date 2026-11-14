//! A table style's custom cell styles survive a DWG save as they do a DXF one (#3132).
#![cfg(not(target_arch = "wasm32"))]
// FreeBSD ARM64 has no native DWG bridge (#2526).
#![cfg(not(all(target_os = "freebsd", target_arch = "aarch64")))]

use cadcraft_doc::{CellStyle, Common, Drawing, EntityKind, Space, Table, TableCell, TableStyle};
use cadcraft_io::{SaveOptions, Version};

fn drawing() -> Drawing {
    let mut d = Drawing::new_imperial();
    let mut st = TableStyle { name: "Costs".into(), text_height: 0.2, ..TableStyle::default() };
    let mut mine = st.cell_style(CellStyle::DATA);
    mine.name = "MINE".into();
    mine.text_height = 0.05;
    mine.align = 4;
    st.set_cell_style(mine);
    d.table_styles.push(st);
    let cell = |t: &str| TableCell { text: t.into(), ..TableCell::default() };
    let mut cells = vec![vec![cell("a"), cell("b")], vec![cell("c"), cell("d")]];
    cells[1][0].style = "MINE".into();
    let t = Table {
        style: "Costs".into(),
        text_height: 0.2,
        title: false,
        header: false,
        row_heights: vec![0.4, 0.4],
        col_widths: vec![2.0, 2.0],
        cells,
        ..Table::default()
    };
    d.add(&Space::Model, Common::default(), EntityKind::Table(t)).unwrap();
    d
}

fn save(d: &Drawing, v: Version) -> Drawing {
    let bytes = cadcraft_io::write_with(d, "t.dwg", &SaveOptions { version: Some(v), binary: false }).unwrap();
    cadcraft_io::read(&bytes, "t.dwg").unwrap()
}

#[test]
fn custom_cell_styles_roundtrip_through_dwg() {
    let d = drawing();
    let want = d.table_styles.iter().find(|s| s.name == "Costs").unwrap().clone();
    for v in [Version::R2004, Version::R2007, Version::R2010, Version::R2013, Version::R2018] {
        let back = save(&d, v);
        let st = back.table_styles.iter().find(|s| s.name == "Costs").expect("table style kept");
        assert!(st.has_cell_style("MINE"), "{v:?}: cell styles after DWG: {:?}", st.cell_styles);
        assert_eq!(st.cell_styles, want.cell_styles, "{v:?}");
        // And a second save keeps them.
        let again = save(&back, v);
        let st = again.table_styles.iter().find(|s| s.name == "Costs").unwrap();
        assert_eq!(st.cell_styles, want.cell_styles, "{v:?}: second save");
        // Plain styles stay plain.
        let std = again.table_styles.iter().find(|s| s.name == "Standard").unwrap();
        assert!(std.cell_styles.is_empty(), "{v:?}");
        // The table's cell still names the style.
        let t = again.model.iter().find_map(|e| if let EntityKind::Table(t) = &e.kind { Some(t) } else { None }).unwrap();
        assert_eq!(t.cells[1][0].style, "MINE", "{v:?}");
    }
}
