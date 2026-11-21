//! TOLERANCE entities (feature control frames, DXF Reference group codes 3, 10, 1, 11) are read,
//! drawn, written and read back; hostile frame strings never panic.

use cadcraft_doc::*;
use cadcraft_geom::Vec3;
use cadcraft_io::{read_dxf, write_dxf};

const FRAME: &str = "{\\Fgdt;j}%%v{\\Fgdt;n}0.1{\\Fgdt;m}%%vA%%vB{\\Fgdt;l}%%vC^J{\\Fgdt;j}%%v0.05%%vA";

fn dxf(text: &str, dir: (&str, &str)) -> String {
    format!(
        "0\nSECTION\n2\nENTITIES\n0\nTOLERANCE\n5\n2A\n8\nGDT\n100\nAcDbEntity\n100\nAcDbFcf\n3\nStandard\n10\n5\n20\n6\n30\n0\n1\n{text}\n11\n{}\n21\n{}\n31\n0\n0\nENDSEC\n0\nEOF\n",
        dir.0, dir.1
    )
}

fn tolerances(d: &Drawing) -> Vec<(Common, Tolerance)> {
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Tolerance(t) => Some((e.common.clone(), t.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn tolerance_reads_draws_and_roundtrips() {
    let d = read_dxf(dxf(FRAME, ("0", "1")).as_bytes()).expect("read");
    let ts = tolerances(&d);
    assert_eq!(ts.len(), 1, "read as a TOLERANCE, not kept as unknown data");
    let (common, t) = &ts[0];
    assert_eq!(common.layer, "GDT");
    assert_eq!((t.insert, t.dir, t.style.as_str(), t.text.as_str()), (Vec3::new(5.0, 6.0, 0.0), Vec3::new(0.0, 1.0, 0.0), "Standard", FRAME));
    assert!((t.rotation() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);

    // Drawn: boxes, symbols and text, turned a quarter turn about the insertion point: the frame
    // grows along +Y from it, its first line straddles x = 5 and the second lies at larger x.
    let list = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default());
    assert!(list.prims.len() > 20, "{} prims", list.prims.len());
    let b = d.extents(&Space::Model);
    let (h, _) = t.text_size(&d);
    assert!((b.min.y - 6.0).abs() < 1e-9 && b.max.y > 6.0 + 8.0 * h, "{b:?}");
    assert!((b.min.x - (5.0 - h)).abs() < 1e-9 && (b.max.x - (5.0 + 3.0 * h)).abs() < 1e-9, "{b:?}");
    assert!((list.bounds.min.y - 6.0).abs() < 1e-6 && list.bounds.max.x <= 5.0 + 3.0 * h + 1e-6, "{:?}", list.bounds);

    let again = read_dxf(write_dxf(&d).as_bytes()).expect("reopen");
    assert_eq!(tolerances(&again), ts);
}

#[test]
fn missing_direction_defaults_to_x() {
    let text = dxf("A", ("0", "0"));
    let d = read_dxf(text.as_bytes()).expect("read");
    assert_eq!(tolerances(&d)[0].1.dir, Vec3::new(1.0, 0.0, 0.0));
}

#[test]
fn hostile_tolerance_does_not_panic() {
    let huge = "{\\Fgdt;j}%%v".repeat(200_000);
    let deep = "{".repeat(100_000);
    for (text, dir) in [
        ("{\\Fgdt;", ("1", "0")),
        ("{\\Fgdt", ("nan", "inf")),
        ("\\", ("1e308", "1e308")),
        ("%%", ("-0", "0")),
        ("^J^J^J^J^J^J^J^J^J^J^J^J", ("1", "0")),
        (huge.as_str(), ("1", "0")),
        (deep.as_str(), ("1", "0")),
        ("{\\H1e308x;\\Fgdt;jjjjjjjjjjjjjjjjjjjjjjjjjj}", ("1", "0")),
    ] {
        if let Ok(d) = read_dxf(dxf(text, dir).as_bytes()) {
            let _ = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default());
            let _ = d.extents(&Space::Model);
            let _ = read_dxf(write_dxf(&d).as_bytes());
        }
    }
}
