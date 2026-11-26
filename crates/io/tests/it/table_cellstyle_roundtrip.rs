//! Table cell styles, row/cell styles and formulas survive a DXF round trip (#534); other readers
//! get the documented TABLESTYLE groups and a block drawn with the styles.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::*;
use cadcraft_geom::Vec3;
use cadcraft_io::SaveOptions;
use craft_dxf::Version;

#[test]
fn modern_table_cell_values_have_reference_groups() {
    for (rows, cols, merged) in [(3, 2, false), (4, 3, true)] {
        let mut d = Drawing::new_imperial();
        let mut cells: Vec<Vec<TableCell>> = (0..rows)
            .map(|row| (0..cols).map(|col| TableCell { text: format!("{}{}", (b'A' + col as u8) as char, row + 1), ..Default::default() }).collect())
            .collect();
        if merged {
            cells[0][0].text = "Title".into();
            cells[0][0].merged = Some((1, cols as u32));
            cells[0][1].text = "CoveredText".repeat(30);
        }
        let table = Table { col_widths: vec![2.0; cols], row_heights: vec![0.5; rows], cells, title: merged, ..Table::default() };
        d.add(&Space::Model, Common::default(), EntityKind::Table(table.clone())).unwrap();
        if !merged {
            let bytes = cadcraft_io::write_with(&d, "t.dxf", &SaveOptions { version: Some(Version::R2018), binary: true }).unwrap();
            assert_eq!(first_table(&cadcraft_io::read(&bytes, "t.dxf").unwrap()).cells, table.cells);
        }
        for version in [Version::R2007, Version::R2010, Version::R2013, Version::R2018] {
            let bytes = cadcraft_io::write_with(&d, "t.dxf", &SaveOptions { version: Some(version), binary: false }).unwrap();
            let text = String::from_utf8(bytes.clone()).unwrap();
            let lines: Vec<_> = text.lines().map(str::trim).collect();
            let pairs: Vec<_> = lines.as_chunks::<2>().0.iter().map(|p| (p[0], p[1])).collect();
            let start = pairs.iter().position(|p| *p == ("0", "ACAD_TABLE")).unwrap();
            let end = start + 1 + pairs[start + 1..].iter().position(|p| p.0 == "0").unwrap();
            let values: Vec<_> = pairs[start..end].iter().filter(|p| p.0 == "302").map(|p| p.1).collect();
            let expected: Vec<_> = table
                .cells
                .iter()
                .enumerate()
                .flat_map(|(row, cells)| {
                    cells.iter().enumerate().map(move |(col, cell)| if merged && row == 0 && col > 0 { "" } else { cell.text.as_str() })
                })
                .collect();
            assert_eq!(values, expected);
            assert_eq!(pairs[start..end].iter().filter(|p| **p == ("301", "CELL_VALUE")).count(), rows * cols);
            assert_eq!(pairs[start..end].iter().filter(|p| **p == ("304", "ACVALUE_END")).count(), rows * cols);
            let value_start = start + pairs[start..end].iter().position(|p| *p == ("301", "CELL_VALUE")).unwrap();
            let value_end = value_start + pairs[value_start..end].iter().position(|p| *p == ("304", "ACVALUE_END")).unwrap();
            let cell_start = start + pairs[start..end].iter().position(|p| *p == ("171", "1")).unwrap();
            assert!(pairs[cell_start..value_start].contains(&("92", "0")));
            let first_text = if merged { "Title" } else { "A1" };
            assert_eq!(
                &pairs[value_start..=value_end],
                &[
                    ("301", "CELL_VALUE"),
                    ("93", "4"),
                    ("90", "4"),
                    ("1", first_text),
                    ("94", "0"),
                    ("300", ""),
                    ("302", first_text),
                    ("304", "ACVALUE_END")
                ]
            );
            if merged {
                assert_eq!(
                    &pairs[cell_start..=value_end],
                    &[
                        ("171", "1"),
                        ("172", "0"),
                        ("173", "0"),
                        ("174", "0"),
                        ("175", "3"),
                        ("176", "1"),
                        ("91", "0"),
                        ("92", "0"),
                        ("178", "0"),
                        ("145", "0.0"),
                        ("301", "CELL_VALUE"),
                        ("93", "4"),
                        ("90", "4"),
                        ("1", "Title"),
                        ("94", "0"),
                        ("300", ""),
                        ("302", "Title"),
                        ("304", "ACVALUE_END")
                    ]
                );
                let covered_start = cell_start + 1 + pairs[cell_start + 1..end].iter().position(|pair| *pair == ("171", "1")).unwrap();
                let covered_end = covered_start + pairs[covered_start..end].iter().position(|pair| *pair == ("304", "ACVALUE_END")).unwrap();
                assert_eq!(
                    &pairs[covered_start..=covered_end],
                    &[
                        ("171", "1"),
                        ("172", "0"),
                        ("173", "1"),
                        ("174", "0"),
                        ("175", "0"),
                        ("176", "0"),
                        ("91", "0"),
                        ("92", "0"),
                        ("178", "0"),
                        ("145", "0.0"),
                        ("301", "CELL_VALUE"),
                        ("93", "4"),
                        ("90", "4"),
                        ("1", ""),
                        ("94", "0"),
                        ("300", ""),
                        ("302", ""),
                        ("304", "ACVALUE_END")
                    ]
                );
            }
            let mut expected_cells = table.cells.clone();
            if merged {
                for cell in &mut expected_cells[0][1..] {
                    cell.text.clear();
                }
            }
            assert_eq!(first_table(&cadcraft_io::read(&bytes, "t.dxf").unwrap()).cells, expected_cells);
        }
        if merged {
            let bytes = cadcraft_io::write_with(&d, "t.dxf", &SaveOptions { version: Some(Version::R2004), binary: false }).unwrap();
            let text = String::from_utf8(bytes).unwrap();
            let lines: Vec<_> = text.lines().map(str::trim).collect();
            let pairs: Vec<_> = lines.as_chunks::<2>().0.iter().map(|pair| (pair[0], pair[1])).collect();
            let start = pairs.iter().position(|pair| *pair == ("0", "ACAD_TABLE")).unwrap();
            let end = start + 1 + pairs[start + 1..].iter().position(|pair| pair.0 == "0").unwrap();
            let mut values = Vec::new();
            let mut value = String::new();
            let cell_start = start + pairs[start..end].iter().position(|pair| *pair == ("171", "1")).unwrap();
            for &(code, chunk) in &pairs[cell_start..end] {
                if code == "2" || code == "1" {
                    value.push_str(chunk);
                    if code == "1" {
                        values.push(std::mem::take(&mut value));
                    }
                }
            }
            assert_eq!(values, table.cells.iter().flatten().map(|cell| cell.text.clone()).collect::<Vec<_>>());
        }
    }
}

#[test]
fn formatted_cell_text_reads_without_typed_text() {
    // Synthetic DXF with one complete value and one value split across chunks.
    let dxf = "0\nSECTION\n2\nENTITIES\n0\nACAD_TABLE\n100\nAcDbTable\n91\n1\n92\n2\n171\n1\n301\nCELL_VALUE\n302\nSingle\n304\nACVALUE_END\n171\n1\n301\nCELL_VALUE\n303\nFirst \n302\nsecond\n304\nACVALUE_END\n0\nENDSEC\n0\nEOF\n";
    let table = first_table(&cadcraft_io::read(dxf.as_bytes(), "table.dxf").unwrap());
    assert_eq!(table.cells[0][0].text, "Single");
    assert_eq!(table.cells[0][1].text, "First second");
}

#[test]
fn legacy_table_cell_text_stays_in_legacy_groups() {
    let (drawing, table) = sample();
    for version in [Version::R2000, Version::R2004] {
        let bytes = cadcraft_io::write_with(&drawing, "t.dxf", &SaveOptions { version: Some(version), binary: false }).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        let lines: Vec<_> = text.lines().map(str::trim).collect();
        let pairs: Vec<_> = lines.as_chunks::<2>().0.iter().map(|pair| (pair[0], pair[1])).collect();
        let start = pairs.iter().position(|pair| *pair == ("0", "ACAD_TABLE")).unwrap();
        let end = start + 1 + pairs[start + 1..].iter().position(|pair| pair.0 == "0").unwrap();
        assert!(!pairs[start..end].iter().any(|pair| matches!(pair.0, "301" | "302" | "303" | "304")));
        let values: Vec<_> = pairs[start..end].iter().filter(|pair| pair.0 == "1").map(|pair| pair.1).collect();
        assert_eq!(values, table.cells.iter().flatten().map(|cell| cell.text.as_str()).collect::<Vec<_>>());
        assert_eq!(first_table(&cadcraft_io::read(&bytes, "t.dxf").unwrap()).cells, table.cells);
    }
}

#[test]
fn long_modern_cell_text_roundtrips() {
    let mut d = Drawing::new_imperial();
    let value = "éabc".repeat(140);
    let table = Table {
        col_widths: vec![2.0],
        row_heights: vec![0.5],
        cells: vec![vec![TableCell { text: value.clone(), ..Default::default() }]],
        ..Table::default()
    };
    d.add(&Space::Model, Common::default(), EntityKind::Table(table)).unwrap();
    for version in [Version::R2007, Version::R2018] {
        let bytes = cadcraft_io::write_with(&d, "t.dxf", &SaveOptions { version: Some(version), binary: false }).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        let lines: Vec<_> = text.lines().collect();
        let pairs: Vec<_> = lines.as_chunks::<2>().0.iter().map(|pair| (pair[0].trim(), pair[1])).collect();
        let start = pairs.iter().position(|pair| *pair == ("301", "CELL_VALUE")).unwrap();
        let end = start + pairs[start..].iter().position(|pair| *pair == ("304", "ACVALUE_END")).unwrap();
        let formatted: String = pairs[start..end].iter().filter(|pair| matches!(pair.0, "302" | "303")).map(|pair| pair.1).collect();
        assert_eq!(formatted, value);
        assert_eq!(first_table(&cadcraft_io::read(&bytes, "t.dxf").unwrap()).cells[0][0].text, value);
    }
}

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
    let lines: Vec<_> = text.lines().map(str::trim).collect();
    let pairs: Vec<_> = lines.as_chunks::<2>().0.iter().map(|pair| (pair[0], pair[1])).collect();
    let start = pairs.iter().position(|pair| *pair == ("2", "BLOCKS")).unwrap();
    let end = start + pairs[start..].iter().position(|pair| *pair == ("0", "ENDSEC")).unwrap();
    assert_eq!(pairs[start..end].iter().filter(|pair| **pair == ("1", "Item")).count(), 2, "one header per table part in the block");
}
