//! A dimension CADCraft keeps as read (LARGE_RADIAL_DIMENSION, ARC_DIMENSION) draws its `*D`
//! block, as AutoCAD does, else the proxy graphics in its AcDbEntity part (#3438); saving keeps
//! the block, which AutoCAD needs to read the dimension (#1161).

use cadcraft_doc::{EntityKind, Space};
use cadcraft_io::{read_dxf, write_dxf};

fn chunk(kind: u32, data: &[u8]) -> Vec<u8> {
    let mut d = data.to_vec();
    d.resize(d.len().div_ceil(4) * 4, 0);
    let mut out = ((8 + d.len()) as u32).to_le_bytes().to_vec();
    out.extend(kind.to_le_bytes());
    out.extend(d);
    out
}

fn v3(x: f64, y: f64, z: f64) -> Vec<u8> {
    [x, y, z].iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// POLYLINE_WITH_NORMAL: the count, the points and the normal.
fn line_with_normal(a: (f64, f64), b: (f64, f64)) -> Vec<u8> {
    let mut d = 2u32.to_le_bytes().to_vec();
    d.extend(v3(a.0, a.1, 0.0));
    d.extend(v3(b.0, b.1, 0.0));
    d.extend(v3(0.0, 0.0, 1.0));
    chunk(32, &d)
}

/// An ARC of radius 500 and a LARGE_RADIAL_DIMENSION on it whose proxy graphics are its jogged
/// leader and the text "R500" (TEXT2), as in the issue's example; `block: true` adds its `*D1`
/// block, which also holds the arrowhead (a SOLID), as an exporter's picture may not.
fn file(block: bool) -> String {
    let mut text = v3(255.0, 245.0, 0.0);
    text.extend(v3(0.0, 0.0, 1.0));
    text.extend(v3(1.0, 0.0, 0.0));
    text.extend(b"R500\0\0\0\0");
    text.extend(4i32.to_le_bytes());
    text.extend(0i32.to_le_bytes());
    for v in [15.0f64, 1.0, 0.0, 0.0] {
        text.extend(v.to_le_bytes());
    }
    text.extend([0u8; 20]);
    text.extend(b"txt\0\0\0\0\0");
    let chunks = [
        line_with_normal((250.0, 150.0), (290.0, 190.0)),
        line_with_normal((290.0, 190.0), (300.0, 230.0)),
        line_with_normal((300.0, 230.0), (353.553, 353.553)),
        chunk(11, &text),
    ];
    let body = chunks.concat();
    let mut g = ((8 + body.len()) as u32).to_le_bytes().to_vec();
    g.extend((chunks.len() as u32).to_le_bytes());
    g.extend(body);
    let mut t: Vec<(i32, String)> = Vec::new();
    if block {
        for (c, v) in [
            (0, "SECTION"),
            (2, "BLOCKS"),
            (0, "BLOCK"),
            (8, "0"),
            (2, "*D1"),
            (70, "1"),
            (10, "0"),
            (20, "0"),
            (3, "*D1"),
            (0, "SOLID"),
            (8, "0"),
            (10, "353.553"),
            (20, "353.553"),
            (11, "335"),
            (21, "345"),
            (12, "345"),
            (22, "335"),
            (13, "345"),
            (23, "335"),
            (0, "LINE"),
            (8, "0"),
            (10, "250"),
            (20, "150"),
            (11, "353.553"),
            (21, "353.553"),
            (0, "TEXT"),
            (8, "0"),
            (10, "255"),
            (20, "245"),
            (40, "15"),
            (1, "R500"),
            (0, "ENDBLK"),
            (0, "ENDSEC"),
        ] {
            t.push((c, v.into()));
        }
    }
    for (c, v) in [(0, "SECTION"), (2, "ENTITIES"), (0, "ARC"), (5, "40"), (8, "0"), (10, "0"), (20, "0"), (40, "500"), (50, "20"), (51, "70")] {
        t.push((c, v.into()));
    }
    for (c, v) in [(0, "LARGE_RADIAL_DIMENSION"), (5, "41"), (100, "AcDbEntity"), (8, "0")] {
        t.push((c, v.into()));
    }
    t.push((92, g.len().to_string()));
    t.extend(g.chunks(127).map(|c| (310, c.iter().map(|b| format!("{b:02X}")).collect())));
    for (c, v) in [
        (100, "AcDbDimension"),
        (2, "*D1"),
        (10, "353.553"),
        (20, "353.553"),
        (70, "9"),
        (100, "AcDbRadialDimensionLarge"),
        (13, "250"),
        (23, "150"),
        (14, "295"),
        (24, "210"),
        (15, "353.553"),
        (25, "353.553"),
        (40, "0.785398"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ] {
        t.push((c, v.into()));
    }
    t.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

fn graphics_kinds(d: &cadcraft_doc::Drawing) -> Vec<String> {
    d.model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Unknown(u) if u.dxf_type == "LARGE_RADIAL_DIMENSION" => {
                Some(u.graphics.iter().map(|g| g.kind.dxf_name().to_string()).collect())
            }
            _ => None,
        })
        .expect("kept as read")
}

/// With its `*D` block the dimension draws the block (with the arrowhead), not its proxy
/// graphics, and saving keeps the block.
#[test]
fn a_kept_dimension_draws_its_block_and_saving_keeps_it() {
    let d = read_dxf(file(true).as_bytes()).expect("read");
    // A reference to the block, which holds the arrowhead the proxy graphics lack.
    assert_eq!(graphics_kinds(&d), ["INSERT"]);
    // ByBlock throughout, so the block's ByBlock objects take the dimension's own.
    let reference = d.model.iter().find_map(|e| match &e.kind {
        EntityKind::Unknown(u) => u.graphics.first().map(|g| g.common.clone()),
        _ => None,
    });
    assert!(
        reference.is_some_and(|c| c.transparency == cadcraft_doc::Transparency::ByBlock && c.plot_style == "ByBlock" && c.layer == "0"),
        "the reference passes the dimension's traits on"
    );
    let b = d.extents(&Space::Model);
    assert!(b.max.x >= 353.0 && b.min.x <= 250.0, "{b:?}");
    let again = read_dxf(write_dxf(&d).as_bytes()).expect("reopen");
    assert!(again.block("*D1").is_some_and(|b| b.entities.len() == 3));
    assert_eq!(graphics_kinds(&again), ["INSERT"]);
}

/// The example from #1161: an ARC_DIMENSION and its `*D1` block survive a save.
#[test]
fn an_arc_length_dimensions_block_survives_a_save() {
    let lines = [
        "0",
        "SECTION",
        "2",
        "BLOCKS",
        "0",
        "BLOCK",
        "8",
        "0",
        "2",
        "*D1",
        "70",
        "1",
        "10",
        "0",
        "20",
        "0",
        "30",
        "0",
        "3",
        "*D1",
        "0",
        "LINE",
        "8",
        "0",
        "10",
        "320",
        "20",
        "150",
        "11",
        "330",
        "21",
        "175",
        "0",
        "ENDBLK",
        "0",
        "ENDSEC",
        "0",
        "SECTION",
        "2",
        "ENTITIES",
        "0",
        "ARC",
        "8",
        "0",
        "10",
        "300",
        "20",
        "150",
        "40",
        "20",
        "50",
        "0",
        "51",
        "90",
        "0",
        "ARC_DIMENSION",
        "5",
        "41",
        "100",
        "AcDbEntity",
        "8",
        "0",
        "100",
        "AcDbDimension",
        "2",
        "*D1",
        "10",
        "333.8",
        "20",
        "169.5",
        "11",
        "327.6",
        "21",
        "177.6",
        "70",
        "5",
        "42",
        "31.4159",
        "100",
        "AcDbArcDimension",
        "13",
        "320",
        "23",
        "150",
        "14",
        "300",
        "24",
        "170",
        "15",
        "300",
        "25",
        "150",
        "40",
        "0",
        "41",
        "1.5708",
        "0",
        "ENDSEC",
        "0",
        "EOF",
    ];
    let d = read_dxf((lines.join("\n") + "\n").as_bytes()).expect("read");
    let again = read_dxf(write_dxf(&d).as_bytes()).expect("reopen");
    assert!(again.block("*D1").is_some_and(|b| b.entities.len() == 1));
    // Drawn from the block: its line reaches (330, 175).
    let b = again.extents(&Space::Model);
    assert!(b.max.x >= 330.0 && b.max.y >= 175.0, "{b:?}");
}

/// A kept dimension: its record, naming block `block`, at handle `handle`.
fn kept_dim(handle: &str, block: &str) -> String {
    format!(
        "0\nARC_DIMENSION\n5\n{handle}\n100\nAcDbEntity\n8\n0\n100\nAcDbDimension\n2\n{block}\n10\n0\n20\n0\n70\n5\n100\nAcDbArcDimension\n13\n0\n23\n0\n14\n1\n24\n0\n15\n0\n25\n0\n40\n0\n41\n1\n"
    )
}

fn block(name: &str, body: &str) -> String {
    format!("0\nBLOCK\n8\n0\n2\n{name}\n70\n1\n10\n0\n20\n0\n3\n{name}\n{body}0\nENDBLK\n")
}

/// A dimension whose block holds another kept dimension draws through it: the inner one draws
/// its own block, here a line to (40, 30).
#[test]
fn a_dimension_inside_a_dimension_block_draws_too() {
    let blocks = block("*D2", "0\nLINE\n8\n0\n10\n0\n20\n0\n11\n40\n21\n30\n") + &block("*D1", &kept_dim("51", "*D2"));
    let text = format!("0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{}0\nENDSEC\n0\nEOF\n", kept_dim("50", "*D1"));
    let d = read_dxf(text.as_bytes()).expect("read");
    let b = d.extents(&Space::Model);
    assert!(b.max.x >= 40.0 && b.max.y >= 30.0, "{b:?}");
}

/// 25 blocks, each with two kept dimensions naming the one before: reading doesn't copy the
/// contents (2^25 lines), and measuring stays within the block depth limit.
#[test]
fn nested_dimension_blocks_are_not_multiplied() {
    let mut blocks = block("*D0", "0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n");
    let mut h = 0x100;
    for i in 1..25 {
        let mut body = String::new();
        for _ in 0..2 {
            body += &kept_dim(&format!("{h:X}"), &format!("*D{}", i - 1));
            h += 1;
        }
        blocks += &block(&format!("*D{i}"), &body);
    }
    let text = format!("0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{}0\nENDSEC\n0\nEOF\n", kept_dim("50", "*D24"));
    let start = std::time::Instant::now();
    let d = read_dxf(text.as_bytes()).expect("read");
    let graphics: usize = d
        .blocks
        .values()
        .flat_map(|b| b.entities.iter())
        .map(|e| match &e.kind {
            EntityKind::Unknown(u) => u.graphics.len(),
            _ => 0,
        })
        .sum();
    assert_eq!(graphics, 48, "one reference per kept dimension");
    let _ = d.extents(&Space::Model);
    assert!(start.elapsed().as_secs() < 10);
}

/// Without a block it draws its proxy graphics.
#[test]
fn a_jogged_radius_dimension_draws_the_picture_it_was_saved_with() {
    let d = read_dxf(file(false).as_bytes()).expect("read");
    let dim = d.model.iter().find_map(|e| match &e.kind {
        EntityKind::Unknown(u) if u.dxf_type == "LARGE_RADIAL_DIMENSION" => Some(u.clone()),
        _ => None,
    });
    let dim = dim.expect("kept as read");
    let kinds: Vec<&str> = dim.graphics.iter().map(|g| g.kind.dxf_name()).collect();
    assert_eq!(kinds, ["LINE", "LINE", "LINE", "TEXT"]);
    assert!(matches!(&dim.graphics[3].kind, EntityKind::Text(t) if t.value == "R500"));
    // The extents reach the leader's start at (250, 150), inside the arc's own box.
    let b = d.extents(&Space::Model);
    assert!(b.min.x <= 250.0 && b.min.y <= 150.0, "{b:?}");
}

/// A `*D` block whose base point isn't the origin: its contents are in the dimension's
/// coordinates, so it draws where they are, as a DIMENSION's block does.
#[test]
fn a_dimension_block_with_a_base_point_draws_in_place() {
    let dxf = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n8\n0\n2\n*D1\n70\n1\n10\n100\n20\n50\n3\n*D1\n0\nLINE\n8\n0\n10\n110\n20\n60\n11\n120\n21\n70\n0\nENDBLK\n0\nENDSEC\n\
0\nSECTION\n2\nENTITIES\n0\nLARGE_RADIAL_DIMENSION\n5\n41\n100\nAcDbEntity\n8\n0\n100\nAcDbDimension\n2\n*D1\n0\nENDSEC\n0\nEOF\n";
    let d = read_dxf(dxf.as_bytes()).expect("read");
    let b = d.extents(&Space::Model);
    assert!(
        (b.min.x - 110.0).abs() < 1e-9 && (b.min.y - 60.0).abs() < 1e-9 && (b.max.x - 120.0).abs() < 1e-9 && (b.max.y - 70.0).abs() < 1e-9,
        "{b:?}"
    );
}
