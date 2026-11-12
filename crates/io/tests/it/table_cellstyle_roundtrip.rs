//! Table cell styles, row/cell styles and formulas survive a DXF round trip (#534); other readers
//! get the documented TABLESTYLE groups and a block drawn with the styles.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::*;
use cadcraft_geom::Vec3;

fn sample() -> (Drawing, Table) {
    let mut d = Drawing::new_imperial();
    let mut st = TableStyle { name: "Costs".into(), text_height: 0.2, margin: 0.05, ..TableStyle::default() };
    let mut header = st.cell_style(CellStyle::HEADER);
    header.fill = Some(Color::Index(2));
    header.text_color = Color::Index(5);
    header.borders.bottom.lineweight = Lineweight::Mm100(50);
    st.set_cell_style(header);
    let mut totals = st.cell_style(CellStyle::DATA);
    totals.name = "Totals".into();
    totals.fill = Some(Color::True(Rgb(10, 200, 30)));
    totals.align = 6;
    totals.format = CellFormat::Decimal;
    totals.precision = 3;
    totals.margin_h = 0.12;
    totals.borders.inside_v.visible = false;
    st.set_cell_style(totals);
    d.table_styles.push(st);
    let cell = |t: &str| TableCell { text: t.into(), ..Default::default() };
    let mut cells =
        vec![vec![cell("Costs"), cell("")], vec![cell("Item"), cell("Cost")], vec![cell("a"), cell("1")], vec![cell("Total"), cell("1.000")]];
    cells[0][0].merged = Some((1, 2));
    cells[3][1].formula = Some("=Sum(B3:B3)".into());
    cells[2][1].style = "Totals".into();
    let t = Table {
        insert: Vec3::new(1.0, 10.0, 0.0),
        col_widths: vec![3.0, 2.0],
        row_heights: vec![0.6, 0.4, 0.4, 0.4],
        cells,
        style: "Costs".into(),
        text_height: 0.2,
        title: true,
        header: true,
        row_styles: vec![String::new(), String::new(), String::new(), "Totals".into()],
        ..Table::default()
    };
    d.add(&Space::Model, Common::default(), EntityKind::Table(t.clone())).unwrap();
    (d, t)
}

fn first_table(d: &Drawing) -> Table {
    d.model.iter().find_map(|e| if let EntityKind::Table(t) = &e.kind { Some(t.clone()) } else { None }).unwrap()
}

#[test]
fn cell_styles_rows_and_formulas_roundtrip() {
    let (d, t) = sample();
    let bytes = cadcraft_io::write(&d, "t.dxf").unwrap();
    let back = cadcraft_io::read(&bytes, "t.dxf").unwrap();
    let st = back.table_styles.iter().find(|s| s.name == "Costs").unwrap();
    assert_eq!(st, d.table_styles.iter().find(|s| s.name == "Costs").unwrap());
    assert_eq!(first_table(&back), t);
    // And again (our own file read back writes the same).
    let again = cadcraft_io::read(&cadcraft_io::write(&back, "t.dxf").unwrap(), "t.dxf").unwrap();
    assert_eq!(first_table(&again), t);
    // Plain styles stay plain.
    let std = back.table_styles.iter().find(|s| s.name == "Standard").unwrap();
    assert!(std.cell_styles.is_empty());
}

#[test]
fn documented_groups_carry_the_builtin_styles() {
    let (d, _) = sample();
    let text = String::from_utf8(cadcraft_io::write(&d, "t.dxf").unwrap()).unwrap();
    // Without our xdata a reader still gets the header fill and colours from the groups.
    let foreign = text.replace("CELLSTYLES", "SOMETHING_ELSE");
    let back = cadcraft_io::read(foreign.as_bytes(), "t.dxf").unwrap();
    let st = back.table_styles.iter().find(|s| s.name == "Costs").unwrap();
    let h = st.cell_style(CellStyle::HEADER);
    assert_eq!((h.fill, h.text_color, h.borders.bottom.lineweight), (Some(Color::Index(2)), Color::Index(5), Lineweight::Mm100(50)));
    // The custom style is ours only.
    assert!(!st.has_cell_style("Totals"));
}

#[test]
fn table_block_draws_fills_and_styles() {
    let (d, _) = sample();
    let text = String::from_utf8(cadcraft_io::write(&d, "t.dxf").unwrap()).unwrap();
    // The (code, value) pairs of the BLOCKS section.
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let pairs: Vec<(&str, &str)> = lines.chunks(2).filter_map(|c| if let [a, b] = c { Some((*a, *b)) } else { None }).collect();
    let start = pairs.iter().position(|p| *p == ("2", "BLOCKS")).unwrap();
    let end = start + pairs.iter().skip(start).position(|p| *p == ("0", "ENDSEC")).unwrap();
    let blocks = &pairs[start..end];
    // Two header cells, the custom cell B3 and the two Totals cells.
    assert_eq!(blocks.iter().filter(|p| **p == ("0", "SOLID")).count(), 5);
    // The Totals value is right-aligned (attachment 6).
    let at = blocks.iter().position(|p| *p == ("1", "1.000")).unwrap();
    let mtext = blocks[..at].iter().rposition(|p| *p == ("0", "MTEXT")).unwrap();
    assert!(blocks[mtext..at].contains(&("71", "6")));
}

#[test]
fn hostile_table_xdata_is_ignored() {
    let (d, _) = sample();
    let text = String::from_utf8(cadcraft_io::write(&d, "t.dxf").unwrap()).unwrap();
    // Broken JSON in both payloads: the file still opens, styles fall back to the groups.
    let broken = text.replace("{\"v\":1", "{\"v\":[[[");
    let back = cadcraft_io::read(broken.as_bytes(), "t.dxf").unwrap();
    let t = first_table(&back);
    assert!(t.row_styles.is_empty());
    assert_eq!(t.cells[3][1].text, "1.000");
}

#[test]
fn breaks_and_data_links_roundtrip_and_the_block_draws_the_parts() {
    let (mut d, mut t) = sample();
    t.breaks =
        Some(TableBreak { enabled: true, height: 1.5, spacing: 0.5, offsets: vec![cadcraft_geom::Vec2::new(0.0, -1.0)], ..TableBreak::default() });
    t.data_link = Some(DataLink { path: "costs.csv".into(), start_row: 1 });
    let h = d.model.iter().next().unwrap().handle;
    d.modify_entity(h, |e| e.kind = EntityKind::Table(t.clone())).unwrap();
    let bytes = cadcraft_io::write(&d, "t.dxf").unwrap();
    let back = cadcraft_io::read(&bytes, "t.dxf").unwrap();
    assert_eq!(first_table(&back), t);
    // Two parts: the block repeats the header row (Item / Cost).
    let text = String::from_utf8(bytes).unwrap();
    assert_eq!(text.lines().filter(|l| l.trim() == "Item").count(), 3, "two in the block, one in the table cells");
}
