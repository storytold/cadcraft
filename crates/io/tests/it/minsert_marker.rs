//! A block reference repeated in rows or columns (MINSERT) is written with the subclass marker
//! `AcDbMInsertBlock`, as AutoCAD writes it; other readers (BricsCAD) draw only the first copy of
//! an array marked `AcDbBlockReference`. A plain INSERT keeps `AcDbBlockReference` (#3127).

use std::sync::Arc;

use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, Insert, Line, Space};
use cadcraft_geom::Vec3;

fn drawing(refs: &[Insert]) -> Drawing {
    let mut d = Drawing::new_metric();
    let mut block = Block::new("MP");
    block.entities.push(Entity::new(d.new_handle(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(2.5, 5.0, 0.0) })));
    d.blocks.insert(block.name.clone(), Arc::new(block));
    for r in refs {
        d.add(&Space::Model, Common::default(), EntityKind::Insert(r.clone())).unwrap();
    }
    d
}

/// The subclass markers (group 100) of each INSERT record in the DXF text, in file order.
fn insert_markers(text: &str) -> Vec<Vec<String>> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let pairs: Vec<(&str, &str)> = lines.chunks(2).filter_map(|c| Some((*c.first()?, *c.get(1)?))).collect();
    let mut out = Vec::new();
    let mut cur: Option<Vec<String>> = None;
    for (code, value) in pairs {
        if code == "0" {
            out.extend(cur.take());
            if value == "INSERT" {
                cur = Some(Vec::new());
            }
        } else if code == "100"
            && let Some(m) = cur.as_mut()
        {
            m.push(value.to_string());
        }
    }
    out.extend(cur);
    out
}

fn inserts(d: &Drawing) -> Vec<Insert> {
    d.model.iter().filter_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.clone()) } else { None }).collect()
}

#[test]
fn minsert_is_written_with_the_minsert_marker() {
    let columns = Insert { block: "MP".into(), cols: 6, col_spacing: 2.5, ..Insert::default() };
    let rows = Insert { block: "MP".into(), insert: Vec3::new(0.0, 20.0, 0.0), rows: 3, row_spacing: 5.0, ..Insert::default() };
    let text = cadcraft_io::write_dxf(&drawing(&[columns, rows]));
    let markers = insert_markers(&text);
    assert_eq!(markers.len(), 2, "{markers:?}");
    for m in &markers {
        assert_eq!(m, &["AcDbEntity", "AcDbMInsertBlock"], "an arrayed reference is an AcDbMInsertBlock");
    }
}

#[test]
fn plain_insert_keeps_the_block_reference_marker() {
    let plain = Insert { block: "MP".into(), insert: Vec3::new(30.0, 0.0, 0.0), ..Insert::default() };
    let text = cadcraft_io::write_dxf(&drawing(&[plain]));
    assert_eq!(insert_markers(&text), [["AcDbEntity", "AcDbBlockReference"]]);
}

#[test]
fn minsert_array_reads_back_from_its_own_dxf() {
    let refs = [
        Insert { block: "MP".into(), cols: 6, col_spacing: 2.5, ..Insert::default() },
        Insert { block: "MP".into(), insert: Vec3::new(0.0, 20.0, 0.0), cols: 2, rows: 3, col_spacing: 4.0, row_spacing: 5.0, ..Insert::default() },
        Insert { block: "MP".into(), insert: Vec3::new(30.0, 0.0, 0.0), ..Insert::default() },
    ];
    let text = cadcraft_io::write_dxf(&drawing(&refs));
    let back = inserts(&cadcraft_io::read_dxf(text.as_bytes()).unwrap());
    let got: Vec<_> = back.iter().map(|i| (i.block.as_str(), i.insert, i.cols, i.rows, i.col_spacing, i.row_spacing)).collect();
    let want: Vec<_> = refs.iter().map(|i| (i.block.as_str(), i.insert, i.cols, i.rows, i.col_spacing, i.row_spacing)).collect();
    assert_eq!(got, want);
}
