//! Draw order from DXF files (#439): a SORTENTSTABLE reorders its space by sort handle, and the
//! order CADCraft writes (the order of the entity records) reads back unchanged.

use cadcraft_doc::*;

fn line(h: &str, y: f64) -> String {
    format!("0\nLINE\n5\n{h}\n330\n1F\n8\n0\n10\n0\n20\n{y}\n30\n0\n11\n10\n21\n{y}\n31\n0\n")
}

fn dxf(objects: &str) -> String {
    let mut s = String::from("0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nBLOCK_RECORD\n0\nBLOCK_RECORD\n5\n1F\n2\n*Model_Space\n0\nENDTAB\n0\nENDSEC\n");
    s.push_str("0\nSECTION\n2\nENTITIES\n");
    s.push_str(&line("A1", 0.0));
    s.push_str(&line("A2", 1.0));
    s.push_str(&line("A3", 2.0));
    s.push_str("0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n");
    s.push_str(objects);
    s.push_str("0\nENDSEC\n0\nEOF\n");
    s
}

fn order(d: &Drawing) -> Vec<String> {
    d.model.iter().map(|e| e.handle.hex()).collect()
}

#[test]
fn sortentstable_sets_the_draw_order() {
    // A1 is drawn by sort handle A4 (over A2 and A3); A3 by 10 (under everything).
    let table = "0\nSORTENTSTABLE\n5\nB0\n330\nB1\n100\nAcDbSortentsTable\n330\n1F\n331\nA1\n5\nA4\n331\nA3\n5\n10\n";
    let d = cadcraft_io::read_dxf(dxf(table).as_bytes()).unwrap();
    assert_eq!(order(&d), ["A3", "A2", "A1"]);
    // Written back in draw order, without a table, the order survives.
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    assert_eq!(order(&back), ["A3", "A2", "A1"]);
}

#[test]
fn broken_sortentstables_are_ignored() {
    for table in [
        "0\nSORTENTSTABLE\n5\nB0\n331\nA1\n5\nA4\n",
        "0\nSORTENTSTABLE\n100\nAcDbSortentsTable\n330\nFFFF\n331\nA1\n5\nA4\n",
        "0\nSORTENTSTABLE\n100\nAcDbSortentsTable\n330\n1F\n331\nzz\n5\nA4\n331\nA2\n5\n\n5\nA9\n",
        "0\nSORTENTSTABLE\n100\nAcDbSortentsTable\n",
    ] {
        let d = cadcraft_io::read_dxf(dxf(table).as_bytes()).unwrap();
        assert_eq!(order(&d), ["A1", "A2", "A3"], "{table:?}");
    }
}

/// A block whose SORTENTSTABLE leaves out the circle (it sorts by its own handle) draws the
/// wipeout, the circle, then the two lines (#2410).
fn block_dxf() -> String {
    block_dxf_with("331\n25B\n5\n25D\n331\n25D\n5\n25E\n331\n25E\n5\n25B\n")
}

fn block_dxf_with(entries: &str) -> String {
    let rec = |h: &str, n: &str| format!("0\nBLOCK_RECORD\n5\n{h}\n2\n{n}\n");
    let mut s = String::from("0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nBLOCK_RECORD\n");
    s.push_str(&rec("1F", "*Model_Space"));
    s.push_str(&rec("2A", "MASKED_B"));
    s.push_str("0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n5\n2B\n330\n2A\n8\n0\n2\nMASKED_B\n70\n0\n10\n0\n20\n0\n30\n0\n");
    let ent = |kind: &str, h: &str, body: &str| format!("0\n{kind}\n5\n{h}\n330\n2A\n8\n0\n{body}");
    s.push_str(&ent("LINE", "25B", "10\n0\n20\n0\n30\n0\n11\n10\n21\n0\n31\n0\n"));
    s.push_str(&ent("CIRCLE", "25C", "10\n5\n20\n5\n30\n0\n40\n2\n"));
    s.push_str(&ent("LINE", "25D", "10\n5\n20\n-5\n30\n0\n11\n5\n21\n5\n31\n0\n"));
    s.push_str(&ent(
        "WIPEOUT",
        "25E",
        "10\n0\n20\n0\n30\n0\n11\n1\n21\n0\n31\n0\n12\n0\n22\n1\n32\n0\n14\n-0.5\n24\n-0.5\n14\n9.5\n24\n-0.5\n14\n9.5\n24\n9.5\n14\n-0.5\n24\n9.5\n",
    ));
    s.push_str("0\nENDBLK\n5\n2C\n330\n2A\n8\n0\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n");
    s.push_str("0\nSORTENTSTABLE\n5\nB0\n330\nB1\n100\nAcDbSortentsTable\n330\n2A\n");
    s.push_str(entries);
    s.push_str("0\nENDSEC\n0\nEOF\n");
    s
}

fn block_kinds(d: &Drawing) -> Vec<&'static str> {
    let b = d.block("MASKED_B").unwrap();
    b.entities
        .iter()
        .map(|e| match &e.kind {
            EntityKind::Wipeout(_) => "wipeout",
            EntityKind::Circle(_) => "circle",
            EntityKind::Line(l) if l.b.y == l.a.y => "hline",
            EntityKind::Line(_) => "vline",
            _ => "other",
        })
        .collect()
}

#[test]
fn a_block_entity_left_out_of_the_table_sorts_by_its_own_handle() {
    let d = cadcraft_io::read_dxf(block_dxf().as_bytes()).unwrap();
    assert_eq!(block_kinds(&d), ["wipeout", "circle", "hline", "vline"]);
}

#[test]
fn the_block_draw_order_survives_dxf_and_dwg() {
    let d = cadcraft_io::read_dxf(block_dxf().as_bytes()).unwrap();
    let dxf = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    assert_eq!(block_kinds(&dxf), ["wipeout", "circle", "hline", "vline"], "DXF");
    let dwg = cadcraft_io::read(&cadcraft_io::write(&d, "t.dwg").unwrap(), "t.dwg").unwrap();
    assert_eq!(block_kinds(&dwg), ["wipeout", "circle", "hline", "vline"], "DWG");
}

/// The table itself (not CADCraft's reordered records) goes through the DWG bridge (#2410).
#[test]
fn a_partial_block_table_survives_the_dwg_bridge() {
    let dwg = cadcraft_dwg::dxf_to_dwg(block_dxf().as_bytes()).unwrap();
    let dxf = cadcraft_dwg::dwg_to_dxf(&dwg).unwrap();
    let d = cadcraft_io::read_dxf(&dxf).unwrap();
    assert_eq!(block_kinds(&d), ["wipeout", "circle", "hline", "vline"]);
}

/// A sort handle of 0 draws the entity last, in a DXF and across the DWG bridge (#2439).
#[test]
fn a_zero_sort_handle_draws_last() {
    // The horizontal line is keyed 0 (last); the vertical line 25E; the wipeout 25B; the circle its own 25C.
    let dxf = block_dxf_with("331\n25B\n5\n0\n331\n25D\n5\n25E\n331\n25E\n5\n25B\n");
    let want = ["wipeout", "circle", "vline", "hline"];
    assert_eq!(block_kinds(&cadcraft_io::read_dxf(dxf.as_bytes()).unwrap()), want, "DXF");
    let dwg = cadcraft_dwg::dxf_to_dwg(dxf.as_bytes()).unwrap();
    let back = cadcraft_io::read_dxf(&cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap();
    assert_eq!(block_kinds(&back), want, "DWG");
}

/// Two entities left out of the table each sort by their own handle, so they land between the
/// listed ones (a single fallback key for every unlisted entity would put them side by side)
/// in a DXF and across the DWG bridge (#2439).
#[test]
fn two_unlisted_block_entities_each_sort_by_their_own_handle() {
    // Listed: the horizontal line 25B and the vertical line 25D, each under its own handle.
    // Unlisted: the circle 25C and the wipeout 25E.
    let dxf = block_dxf_with("331\n25B\n5\n25B\n331\n25D\n5\n25D\n");
    let want = ["hline", "circle", "vline", "wipeout"];
    assert_eq!(block_kinds(&cadcraft_io::read_dxf(dxf.as_bytes()).unwrap()), want, "DXF");
    let dwg = cadcraft_dwg::dxf_to_dwg(dxf.as_bytes()).unwrap();
    let back = cadcraft_io::read_dxf(&cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap();
    assert_eq!(block_kinds(&back), want, "DWG");
}
