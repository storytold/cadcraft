//! The hatch patterns added to the library (ANGLE â€¦ TRIANG, the AR- block, brick, parquet and
//! roof patterns, ISO02W100 â€¦ ISO15W100): each draws, and a HATCH of each keeps its name and
//! its pattern lines through a DXF and a DWG save and reopen.
//!
//! Set `CADCRAFT_HATCH_SHEET` to a `.png` path to also save a labelled contact sheet.

use cadcraft_doc::library::{PatternLine, pattern};
use cadcraft_doc::*;
use cadcraft_geom::{PolyVertex, Vec2, Vec3};

const NEW: [&str; 48] = [
    "ANGLE",
    "BOX",
    "BRASS",
    "BRSTONE",
    "CLAY",
    "CORK",
    "DOLMIT",
    "FLEX",
    "GOST_GLASS",
    "GOST_GROUND",
    "GOST_WOOD",
    "GRATE",
    "GRAVEL",
    "HEX",
    "HOUND",
    "INSUL",
    "MUDST",
    "PLAST",
    "PLASTI",
    "SACNCR",
    "STARS",
    "STEEL",
    "SWAMP",
    "TRANS",
    "TRIANG",
    "AR-B816",
    "AR-B816C",
    "AR-B88",
    "AR-BRELM",
    "AR-BRSTD",
    "AR-HBONE",
    "AR-PARQ1",
    "AR-RROOF",
    "AR-RSHKE",
    "ISO02W100",
    "ISO03W100",
    "ISO04W100",
    "ISO05W100",
    "ISO06W100",
    "ISO07W100",
    "ISO08W100",
    "ISO09W100",
    "ISO10W100",
    "ISO11W100",
    "ISO12W100",
    "ISO13W100",
    "ISO14W100",
    "ISO15W100",
];

/// Side of one contact sheet cell, and of each test hatch.
const CELL: f64 = 10.0;

/// The largest repeat of a pattern's families (spacing or dash period).
fn repeat(lines: &[PatternLine]) -> f64 {
    lines.iter().map(|l| l.delta.1.abs().max(l.dashes.iter().map(|d| d.abs()).sum::<f64>())).fold(0.0, f64::max)
}

fn square(x: f64, y: f64, side: f64) -> HatchLoop {
    let v = [(x, y), (x + side, y), (x + side, y + side), (x, y + side)].into_iter().map(|(x, y)| PolyVertex::new(Vec2::new(x, y))).collect();
    HatchLoop { vertices: v, outer: true, sources: Vec::new() }
}

/// A hatch of library pattern `name` filling a square at (x, y), scaled to show a few repeats.
fn hatch(name: &str, x: f64, y: f64, side: f64) -> Hatch {
    let lines = pattern(name).map(|p| p.lines).unwrap_or_default();
    Hatch {
        pattern: name.into(),
        solid: false,
        loops: vec![square(x, y, side)],
        scale: side / (4.0 * repeat(&lines).max(1e-9)),
        angle: 0.0,
        associative: false,
        style: 0,
        elevation: 0.0,
        gradient: None,
        origin: Vec2::new(x, y),
        background: None,
        pattern_lines: Vec::new(),
    }
}

fn hatches(d: &Drawing) -> Vec<Hatch> {
    d.model.iter().filter_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some(h.clone()) } else { None }).collect()
}

fn near(a: &[PatternLine], b: &[PatternLine], tol: f64) -> bool {
    let close = |x: f64, y: f64| (x - y).abs() <= tol * (1.0 + y.abs());
    let ang = |x: f64, y: f64| ((x - y).rem_euclid(360.0) + 1e-6).rem_euclid(360.0) < 1e-5;
    a.len() == b.len()
        && a.iter().zip(b).all(|(p, q)| {
            ang(p.angle, q.angle)
                && close(p.origin.0, q.origin.0)
                && close(p.origin.1, q.origin.1)
                && close(p.delta.0, q.delta.0)
                && close(p.delta.1, q.delta.1)
                && p.dashes.len() == q.dashes.len()
                && p.dashes.iter().zip(&q.dashes).all(|(x, y)| close(*x, *y))
        })
}

#[test]
fn every_new_pattern_is_in_the_library_and_draws() {
    for name in NEW {
        let p = pattern(name).unwrap_or_else(|| panic!("{name} missing"));
        assert!(!p.lines.is_empty(), "{name}");
        let mut d = Drawing::new_imperial();
        d.add(&Space::Model, Common::default(), EntityKind::Hatch(hatch(name, 0.0, 0.0, CELL))).expect("add");
        let png = cadcraft_io::png(&d, &Space::Model, 160, 160).expect("png");
        let img = tiny_skia::Pixmap::decode_png(&png).expect("decode");
        let px = img.pixels();
        let bg = px[0];
        let ink = px.iter().filter(|p| **p != bg).count();
        assert!(ink > 200, "{name}: {ink} drawn pixels");
    }
}

#[test]
fn new_patterns_keep_name_and_lines_through_dxf_and_dwg() {
    let mut d = Drawing::new_imperial();
    for (k, name) in NEW.iter().enumerate() {
        let h = Hatch { origin: Vec2::ZERO, scale: 1.0, ..hatch(name, (k % 8) as f64 * 12.0, (k / 8) as f64 * 12.0, CELL) };
        d.add(&Space::Model, Common::default(), EntityKind::Hatch(h)).expect("add");
    }
    let dxf = cadcraft_io::read(&cadcraft_io::write(&d, "x.dxf").expect("dxf"), "x.dxf").expect("read dxf");
    let dwg = cadcraft_io::read(&cadcraft_io::write(&d, "x.dwg").expect("dwg"), "x.dwg").expect("read dwg");
    for (what, back) in [("dxf", &dxf), ("dwg", &dwg)] {
        let hs = hatches(back);
        assert_eq!(hs.len(), NEW.len(), "{what}");
        for (h, name) in hs.iter().zip(NEW) {
            assert_eq!(h.pattern, name, "{what}");
            assert!(!h.solid, "{what} {name}");
            let lib = pattern(name).map(|p| p.lines).unwrap_or_default();
            assert!(near(&h.pattern_lines, &lib, 1e-6), "{what} {name}: {:?}\nvs {:?}", h.pattern_lines, lib);
        }
        // Saved again, the file's definition is kept as read.
        let again = cadcraft_io::read(&cadcraft_io::write(back, "y.dxf").expect("resave"), "y.dxf").expect("reread");
        for (h, name) in hatches(&again).iter().zip(NEW) {
            assert_eq!(h.pattern, name, "{what} resaved");
            assert!(near(&h.pattern_lines, &pattern(name).map(|p| p.lines).unwrap_or_default(), 1e-6), "{what} resaved {name}");
        }
    }
}

#[test]
fn contact_sheet() {
    let Some(path) = std::env::var_os("CADCRAFT_HATCH_SHEET") else { return };
    let mut d = Drawing::new_imperial();
    let cols = 8;
    let pitch = CELL * 1.25;
    for (k, name) in NEW.iter().enumerate() {
        let (x, y) = ((k % cols) as f64 * pitch, -((k / cols) as f64) * pitch * 1.15);
        d.add(&Space::Model, Common::default(), EntityKind::Hatch(hatch(name, x, y, CELL))).expect("add");
        let outline = [(0.0, 0.0), (CELL, 0.0), (CELL, CELL), (0.0, CELL), (0.0, 0.0)];
        for w in outline.windows(2) {
            let line = Line { a: Vec3::new(x + w[0].0, y + w[0].1, 0.0), b: Vec3::new(x + w[1].0, y + w[1].1, 0.0) };
            d.add(&Space::Model, Common::default(), EntityKind::Line(line)).expect("add");
        }
        let t = Text {
            insert: Vec3::new(x, y - 1.0, 0.0),
            align_pt: None,
            height: 0.8,
            value: (*name).into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: HAlign::Left,
            valign: VAlign::Baseline,
        };
        d.add(&Space::Model, Common::default(), EntityKind::Text(t)).expect("add");
    }
    let png = cadcraft_io::png(&d, &Space::Model, 2400, 2000).expect("png");
    std::fs::write(path, png).expect("write sheet");
}
