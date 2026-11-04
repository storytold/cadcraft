//! ACAD_PROXY_ENTITY draws its proxy graphics (#2646): read from DXF with the table indices of the
//! file, measured in the extents, and written back unchanged.

use cadcraft_doc::color::Color;
use cadcraft_doc::{EntityKind, Space};
use cadcraft_io::{read_dxf, write_dxf};

/// A proxy graphics chunk of type `kind`, its data padded to four bytes.
fn chunk(kind: u32, data: &[u8]) -> Vec<u8> {
    let mut d = data.to_vec();
    d.resize(d.len().div_ceil(4) * 4, 0);
    let mut out = ((8 + d.len()) as u32).to_le_bytes().to_vec();
    out.extend(kind.to_le_bytes());
    out.extend(d);
    out
}

/// A point in the XY plane as three doubles.
fn pt(x: f64, y: f64) -> Vec<u8> {
    [x, y, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// A balloon: layer 1 of the file's LAYER table, colour 1, a circle, a leader and the number.
fn balloon() -> Vec<u8> {
    let mut circle = pt(175.0, 115.0);
    circle.extend(6.0f64.to_le_bytes());
    circle.extend([0.0f64, 0.0, 1.0].iter().flat_map(|v| v.to_le_bytes()));
    let mut leader = 2u32.to_le_bytes().to_vec();
    leader.extend(pt(170.8, 110.8));
    leader.extend(pt(150.0, 90.0));
    let mut text = pt(172.9, 112.0);
    text.extend([0.0f64, 0.0, 1.0].iter().flat_map(|v| v.to_le_bytes()));
    text.extend(pt(1.0, 0.0));
    text.extend(b"1\0\0\0");
    text.extend(1i32.to_le_bytes());
    text.extend(0i32.to_le_bytes());
    for v in [6.0f64, 1.0, 0.0, 0.0] {
        text.extend(v.to_le_bytes());
    }
    text.extend([0u8; 20]);
    text.extend(b"isocp.shx\0\0\0\0\0\0\0");
    let chunks = [chunk(16, &1u32.to_le_bytes()), chunk(14, &1u32.to_le_bytes()), chunk(2, &circle), chunk(6, &leader), chunk(11, &text)];
    let body = chunks.concat();
    let mut out = ((8 + body.len()) as u32).to_le_bytes().to_vec();
    out.extend((chunks.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

/// A DXF whose LAYER table holds 0 and BALLOONS only (CADCraft keeps Defpoints at index 1 of
/// its own table), a text style on isocp.shx, a line and one proxy balloon.
fn file() -> String {
    let graphics = balloon();
    let hex: Vec<String> = graphics.chunks(127).map(|c| c.iter().map(|b| format!("{b:02X}")).collect()).collect();
    let mut tags: Vec<(i32, String)> = vec![(0, "SECTION".into()), (2, "CLASSES".into())];
    for (c, v) in [(0, "CLASS"), (1, "SYNBALLOON"), (2, "SynBalloon"), (3, "SyntheticProxyApp"), (90, "0"), (280, "0"), (281, "1"), (0, "ENDSEC")] {
        tags.push((c, v.into()));
    }
    for (c, v) in [
        (0, "SECTION"),
        (2, "TABLES"),
        (0, "TABLE"),
        (2, "LAYER"),
        (0, "LAYER"),
        (5, "10"),
        (2, "0"),
        (62, "7"),
        (0, "LAYER"),
        (5, "11"),
        (2, "BALLOONS"),
        (62, "3"),
        (0, "ENDTAB"),
        (0, "TABLE"),
        (2, "STYLE"),
        (0, "STYLE"),
        (5, "12"),
        (2, "ISO"),
        (3, "isocp.shx"),
        (0, "ENDTAB"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "ENTITIES"),
        (0, "LINE"),
        (5, "20"),
        (8, "0"),
        (10, "50"),
        (20, "40"),
        (11, "150"),
        (21, "90"),
        (0, "ACAD_PROXY_ENTITY"),
        (5, "21"),
        (100, "AcDbEntity"),
        (8, "0"),
        (100, "AcDbProxyEntity"),
        (90, "498"),
        (91, "500"),
        (95, "0"),
        (70, "0"),
    ] {
        tags.push((c, v.into()));
    }
    tags.push((92, graphics.len().to_string()));
    tags.extend(hex.into_iter().map(|h| (310, h)));
    for (c, v) in [(93, "0"), (94, "0"), (0, "ENDSEC"), (0, "EOF")] {
        tags.push((c, v.into()));
    }
    tags.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

#[test]
fn a_proxy_draws_its_graphics_with_the_files_table_indices() {
    let d = read_dxf(file().as_bytes()).expect("read");
    let proxy = d.model.iter().find(|e| matches!(e.kind, EntityKind::Unknown(_))).expect("proxy");
    let EntityKind::Unknown(u) = &proxy.kind else { unreachable!() };
    let kinds: Vec<(&str, &str, Color)> = u.graphics.iter().map(|g| (g.kind.dxf_name(), g.common.layer.as_str(), g.common.color)).collect();
    // Layer index 1 is BALLOONS, the file's second layer, not CADCraft's Defpoints.
    assert_eq!(kinds, [("CIRCLE", "BALLOONS", Color::Index(1)), ("LINE", "BALLOONS", Color::Index(1)), ("TEXT", "BALLOONS", Color::Index(1))]);
    assert!(matches!(&u.graphics[2].kind, EntityKind::Text(t) if t.value == "1" && t.style == "ISO"));
    // The extents reach the balloon (the line alone ends at 150, 90).
    let b = d.extents(&Space::Model);
    assert!(b.max.x >= 181.0 && b.max.y >= 121.0, "{b:?}");
}

#[test]
fn a_proxy_is_written_back_unchanged_and_draws_again() {
    let d = read_dxf(file().as_bytes()).expect("read");
    let again = read_dxf(write_dxf(&d).as_bytes()).expect("reopen");
    let graphics = |d: &cadcraft_doc::Drawing| {
        d.model.iter().find_map(|e| match &e.kind {
            EntityKind::Unknown(u) => Some((u.proxy_graphics_bytes(), u.graphics.len())),
            _ => None,
        })
    };
    assert_eq!(graphics(&again), graphics(&d));
    assert_eq!(graphics(&d).map(|g| g.1), Some(3));
    // The saved LAYER table keeps the file's order, so index 1 is still BALLOONS (CADCraft's
    // Defpoints comes after it) and the reopened graphics stay on that layer.
    let layers = |d: &cadcraft_doc::Drawing| -> Vec<String> {
        d.model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::Unknown(u) => Some(u.graphics.iter().map(|g| g.common.layer.clone()).collect()),
                _ => None,
            })
            .unwrap_or_default()
    };
    assert_eq!(layers(&again), ["BALLOONS"; 3]);
    let names: Vec<&str> = again.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["0", "BALLOONS", "Defpoints"]);
}

/// A proxy text takes the first style on its font. With ISO and then Standard both on
/// isocp.shx, it is ISO, and stays ISO after a save: CADCraft's own Standard, which the file's
/// replaces, would otherwise come first in the saved STYLE table.
#[test]
fn a_proxy_text_keeps_its_style_after_a_save() {
    let iso = "0\nSTYLE\n5\n12\n2\nISO\n3\nisocp.shx\n";
    let src = file().replacen(iso, &format!("{iso}0\nSTYLE\n5\n13\n2\nStandard\n3\nisocp.shx\n"), 1);
    let style = |d: &cadcraft_doc::Drawing| {
        d.model.iter().find_map(|e| match &e.kind {
            EntityKind::Unknown(u) => u.graphics.iter().find_map(|g| match &g.kind {
                EntityKind::Text(t) => Some(t.style.clone()),
                _ => None,
            }),
            _ => None,
        })
    };
    let d = read_dxf(src.as_bytes()).expect("read");
    assert_eq!(style(&d).as_deref(), Some("ISO"));
    let again = read_dxf(write_dxf(&d).as_bytes()).expect("reopen");
    assert_eq!(style(&again).as_deref(), Some("ISO"), "after a save");
}
